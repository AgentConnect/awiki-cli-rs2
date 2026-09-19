//! Durable execution details, separate from the current session's live snapshot.
use super::store::{Question, Session, Work};
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub schema: String,
    pub session_key: String,
    pub agent_did: String,
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub group: bool,
    pub run_id: String,
    pub task_id: String,
    pub source_message_id: String,
    pub requester_did: String,
    pub revision: u64,
    pub state: String,
    pub model_id: Option<String>,
    pub text: String,
    pub tools: Vec<Value>,
    pub omitted_tool_count: usize,
    pub questions: Vec<Value>,
    pub error_code: Option<String>,
    pub error_details: Option<Value>,
    pub delivery: Delivery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delivery {
    pub state: String,
    pub message_id: Option<String>,
}

pub fn terminal(state: &str) -> bool {
    matches!(state, "finished" | "cancelled" | "failed" | "interrupted")
}

impl TaskRecord {
    pub fn capture(session: &Session, work: &Work, state: &str, include_output: bool) -> Self {
        Self {
            schema: "awiki.acp.task.v1".into(),
            session_key: session.key.clone(),
            agent_did: session.agent_did.clone(),
            conversation_id: work.task.conversation_id.clone(),
            group: session.group,
            run_id: work.run_id.clone(),
            task_id: work.task.task_id.clone(),
            source_message_id: work.task.correlation().source_message_id,
            requester_did: work.task.requester_did.clone(),
            revision: session.revision,
            state: state.into(),
            model_id: session.model.clone(),
            text: if include_output {
                session.text.clone()
            } else {
                String::new()
            },
            tools: if include_output {
                session.tools.clone()
            } else {
                vec![]
            },
            omitted_tool_count: if include_output {
                session.omitted_tool_count
            } else {
                0
            },
            questions: if include_output {
                session
                    .questions
                    .iter()
                    .map(|q| question_record(q, terminal(state)))
                    .collect()
            } else {
                vec![]
            },
            error_code: if include_output {
                session.interaction_error.clone()
            } else {
                None
            },
            error_details: if include_output {
                session.error_details.clone()
            } else {
                None
            },
            delivery: Delivery {
                state: if state == "finished" {
                    "pending"
                } else {
                    "none"
                }
                .into(),
                message_id: None,
            },
        }
    }

    pub fn summary(&self) -> Value {
        json!({"run_id":self.run_id,"task_id":self.task_id,"source_message_id":self.source_message_id,
            "requester_did":self.requester_did,"state":self.state,"revision":self.revision,
            "model_id":self.model_id,"error_code":self.error_code,"error_details":self.error_details,
            "delivery":self.delivery})
    }
}

pub(super) fn question_record(question: &Question, closed: bool) -> Value {
    let status = match question
        .response
        .as_ref()
        .and_then(|v| v["action"].as_str())
    {
        Some("accept") => "answered",
        Some("decline") => "skipped",
        Some("cancel") => "closed",
        _ if question.end_reason.as_deref() == Some("expired") => "expired",
        _ if question.end_reason.is_some() => "closed",
        _ if closed => "closed",
        _ => "pending",
    };
    let mut value = json!({"id":question.id,"run_id":question.run_id,"expires_at_ms":question.expires_at_ms,
        "request":question.request,"response":question.response,"status":status,"end_reason":question.end_reason});
    if let Some(interaction) = &question.interaction {
        let metadata = serde_json::to_value(interaction).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .extend(metadata.as_object().unwrap().clone());
        value["interaction_version"] = json!(2);
    }
    value
}

pub(super) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS acp_task_records (
        run_id TEXT PRIMARY KEY, session_key TEXT NOT NULL, revision INTEGER NOT NULL, data TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS acp_task_records_session ON acp_task_records(session_key);")?;
    Ok(())
}

pub fn load(db: &Connection, run: &str) -> Result<Option<TaskRecord>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT data FROM acp_task_records WHERE run_id=?1",
            [run],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(Into::into)
}

/// Called inside the same transaction that saves the session/command/final outbox.
pub(super) fn persist(db: &Connection, record: &TaskRecord) -> Result<()> {
    let prior = load(db, &record.run_id)?;
    if let Some(prior) = &prior {
        if prior.session_key != record.session_key {
            bail!("task_record_scope_mismatch");
        }
        if terminal(&prior.state) {
            return Ok(());
        }
    }
    db.execute(
        "INSERT INTO acp_task_records(run_id,session_key,revision,data) VALUES(?1,?2,?3,?4)
        ON CONFLICT(run_id) DO UPDATE SET revision=excluded.revision,data=excluded.data",
        params![
            record.run_id,
            record.session_key,
            record.revision,
            serde_json::to_string(record)?
        ],
    )?;
    if terminal(&record.state) {
        queue(db, record, &format!("acp-task:{}:terminal", record.run_id))?;
    } else {
        // Answers must survive the next streamed snapshot and a subsequent task.
        for question in &record.questions {
            if question["status"] == "pending" {
                continue;
            }
            let id = question["id"].as_str().unwrap_or_default();
            if prior.as_ref().is_some_and(|r| {
                r.questions
                    .iter()
                    .any(|q| q["id"] == id && q["status"] != "pending")
            }) {
                continue;
            }
            queue(
                db,
                record,
                &format!("acp-question:{}:{id}:terminal", record.run_id),
            )?;
        }
    }
    Ok(())
}

fn queue(db: &Connection, record: &TaskRecord, id: &str) -> Result<()> {
    db.execute("INSERT OR IGNORE INTO acp_events(event_id,session_key,run_id,snapshot,event_kind) VALUES(?1,?2,?3,?4,'task')",
        params![id, record.session_key, record.run_id, serde_json::to_string(record)?])?;
    Ok(())
}

/// Final-message delivery is independent of model completion; updating it never
/// starts another run and publishes another immutable reliable event.
pub(crate) fn delivery_in(
    db: &Connection,
    outbox_key: &str,
    state: &str,
    message_id: Option<&str>,
) -> Result<()> {
    let run: String = db.query_row(
        "SELECT run_id FROM runtime_final_outbox WHERE idempotency_key=?1",
        [outbox_key],
        |r| r.get(0),
    )?;
    let Some(mut record) = load(db, &run)? else {
        return Ok(());
    };
    if record.delivery.state == state {
        return Ok(());
    }
    if record.state != "finished" {
        bail!("task_delivery_state_mismatch");
    }
    record.delivery = Delivery {
        state: state.into(),
        message_id: message_id.map(str::to_owned),
    };
    record.revision += 1;
    db.execute(
        "UPDATE acp_task_records SET revision=?1,data=?2 WHERE run_id=?3",
        params![record.revision, serde_json::to_string(&record)?, run],
    )?;
    queue(db, &record, &format!("acp-task:{run}:delivery:{state}"))
}

pub fn page(db: &Connection, key: &str, before: Option<i64>, limit: usize) -> Result<Value> {
    page_for_sources(db, key, before, limit, None)
}

/// Fetch only records attached to the currently displayed Core message window.
/// A cursor stays within the authorized session and never identifies a route.
pub fn page_for_sources(
    db: &Connection,
    key: &str,
    before: Option<i64>,
    limit: usize,
    sources: Option<&[String]>,
) -> Result<Value> {
    let limit = limit.clamp(1, 20);
    let sources = sources.map(serde_json::to_string).transpose()?;
    let mut rows = db.prepare("SELECT rowid,data FROM acp_task_records WHERE session_key=?1 AND (?2 IS NULL OR rowid<?2) AND (?4 IS NULL OR json_extract(data,'$.source_message_id') IN (SELECT value FROM json_each(?4))) ORDER BY rowid DESC LIMIT ?3")?
        .query_map(params![key,before,limit as i64 + 1,sources], |row| Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Keep ordinary pages under 512 KiB. One existing task can be larger (the
    // same bound as its live event); never truncate its answers or text.
    let mut bytes = 0usize;
    let mut count = 0usize;
    for (_, raw) in rows.iter().take(limit) {
        if count > 0 && bytes + raw.len() > 512 * 1024 {
            break;
        }
        bytes += raw.len();
        count += 1;
    }
    let more = rows.len() > count;
    rows.truncate(count);
    let next = if more {
        rows.last().map(|row| row.0)
    } else {
        None
    };
    let records = rows
        .into_iter()
        .map(|(_, raw)| serde_json::from_str::<TaskRecord>(&raw))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"tasks":records,"next_cursor":next,"has_more":more}))
}
