use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::security::runtime_token::current_time_millis;
use crate::{
    runtime::{RuntimeConversationScopeKind, RuntimeTask},
    DaemonState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Work {
    pub task: RuntimeTask,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub run_id: String,
    pub expires_at_ms: i64,
    pub request: Value,
    pub response: Option<Value>,
    #[serde(default)]
    pub interaction: Option<super::questions::QuestionInteraction>,
    #[serde(default)]
    pub end_reason: Option<String>,
}

impl Question {
    pub fn pending(&self) -> bool {
        self.response.is_none() && self.end_reason.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub key: String,
    pub agent_did: String,
    pub controller_scope_key: String,
    pub scope: String,
    pub conversation_id: Option<String>,
    pub revision: u64,
    pub native_session_id: Option<String>,
    #[serde(default)]
    pub native_created_at_ms: Option<i64>,
    #[serde(default)]
    pub restoring: bool,
    pub model: Option<String>,
    #[serde(default)]
    pub selected_model: Option<String>,
    #[serde(default)]
    pub configuration_version: u8,
    #[serde(default)]
    pub model_configuration_ready: bool,
    #[serde(default)]
    pub model_catalog_updated_at_ms: Option<i64>,
    pub capabilities: Value,
    pub options: Value,
    pub group: bool,
    pub active: Option<Work>,
    pub waiting: Option<Work>,
    pub waiting_paused: bool,
    pub stopping: bool,
    pub execute_after_stop: bool,
    pub context_lost: bool,
    pub last_task: Value,
    #[serde(default)]
    pub history: Vec<Value>,
    pub text: String,
    pub tools: Vec<Value>,
    #[serde(default)]
    pub omitted_tool_count: usize,
    pub questions: Vec<Question>,
    #[serde(default)]
    pub interaction_error: Option<String>,
    #[serde(default)]
    pub error_details: Option<Value>,
    pub last_run_id: Option<String>,
    #[serde(skip)]
    pending_records: Vec<super::task_records::TaskRecord>,
}

impl Session {
    pub fn new(task: &RuntimeTask) -> Self {
        Self::for_scope(
            &task.agent_did,
            &task.controller_scope_key,
            task.conversation_scope.scope_key(),
            task.conversation_id.clone(),
            task.conversation_scope.kind() == RuntimeConversationScopeKind::GroupVisible,
        )
    }
    pub fn for_scope(
        agent: &str,
        controller: &str,
        scope: String,
        conversation_id: Option<String>,
        group: bool,
    ) -> Self {
        Self {
            key: session_key(agent, controller, &scope),
            agent_did: agent.to_owned(),
            controller_scope_key: controller.to_owned(),
            scope,
            conversation_id,
            revision: 0,
            native_session_id: None,
            native_created_at_ms: None,
            restoring: false,
            model: None,
            selected_model: None,
            configuration_version: 1,
            model_configuration_ready: false,
            model_catalog_updated_at_ms: None,
            capabilities: json!({}),
            options: json!([]),
            group,
            active: None,
            waiting: None,
            waiting_paused: false,
            stopping: false,
            execute_after_stop: false,
            context_lost: false,
            last_task: Value::Null,
            history: vec![],
            text: String::new(),
            tools: vec![],
            omitted_tool_count: 0,
            questions: vec![],
            interaction_error: None,
            error_details: None,
            last_run_id: None,
            pending_records: vec![],
        }
    }
    pub fn submit(&mut self, work: Work) -> Result<bool> {
        if work.task.agent_did != self.agent_did
            || work.task.controller_scope_key != self.controller_scope_key
            || work.task.conversation_scope.scope_key() != self.scope
        {
            bail!("conversation_mismatch");
        }
        if self.context_lost {
            bail!("context_reset_required");
        }
        // Direct transport aliases may change after an authoritative DID
        // rotation. Native context is bound to the verified stable scope above.
        self.conversation_id = work.task.conversation_id.clone();
        if self.active.is_some() || self.waiting.is_some() {
            if self.group {
                bail!("group_busy");
            }
            if self.waiting.is_some() {
                bail!("waiting_slot_full");
            }
            self.waiting = Some(work);
            return Ok(false);
        }
        self.start(work);
        Ok(true)
    }
    fn start(&mut self, work: Work) {
        self.last_run_id = Some(work.run_id.clone());
        self.active = Some(work);
        self.stopping = false;
        self.restoring = false;
        self.execute_after_stop = false;
        self.waiting_paused = false;
        self.text.clear();
        self.tools.clear();
        self.omitted_tool_count = 0;
        self.questions.clear();
        self.interaction_error = None;
        self.error_details = None;
    }
    pub fn active_run(&self, run_id: &str) -> bool {
        self.active.as_ref().is_some_and(|w| w.run_id == run_id)
    }
    pub fn complete(&mut self, run_id: &str, outcome: &str) -> Result<Option<Work>> {
        if !self.active_run(run_id) {
            bail!("stale_task");
        }
        let work = self.active.take().context("missing_task")?;
        self.close_questions(match outcome {
            "cancelled" => "task_stopped",
            "interrupted" => "task_interrupted",
            "failed" => "task_failed",
            _ => "task_ended",
        });
        let record = super::task_records::TaskRecord::capture(self, &work, outcome, true);
        self.last_task = record.summary();
        self.pending_records.push(record);
        self.record_last_task();
        self.questions.clear();
        self.restoring = false;
        let proceed = (outcome == "finished" && !self.stopping)
            || (matches!(outcome, "cancelled" | "finished") && self.execute_after_stop);
        self.stopping = false;
        self.execute_after_stop = false;
        self.waiting_paused = !proceed && self.waiting.is_some();
        if proceed && !self.context_lost {
            if let Some(next) = self.waiting.take() {
                self.waiting_paused = false;
                self.start(next.clone());
                return Ok(Some(next));
            }
        }
        Ok(None)
    }
    pub fn command(
        &mut self,
        action: &str,
        args: &Value,
        sender: &str,
        now: i64,
    ) -> Result<Option<Work>> {
        if action == "answer" {
            let work = self.active.as_ref().context("stale_task")?;
            if work.task.requester_did != sender {
                bail!("not_task_requester");
            }
            if self.stopping {
                bail!("stale_question");
            }
            let run_id = args["run_id"].as_str().context("run_id_required")?;
            let id = args["question_id"]
                .as_str()
                .context("question_id_required")?;
            let question = self
                .questions
                .iter_mut()
                .find(|q| q.id == id && q.run_id == run_id)
                .context("stale_question")?;
            if question.expires_at_ms <= now || !question.pending() || work.run_id != run_id {
                bail!("stale_question");
            }
            let response = super::questions::validate_response(question, args)?;
            question.end_reason = match response["action"].as_str() {
                Some("decline") => Some("user_skipped".into()),
                Some("cancel") => Some("user_dismissed".into()),
                _ => None,
            };
            question.response = Some(response);
            return Ok(None);
        }
        if self.group && matches!(action, "stop" | "execute_waiting" | "cancel_waiting") {
            bail!("unsupported_in_group");
        }
        match action {
            "stop" => {
                if !self
                    .active
                    .as_ref()
                    .is_some_and(|w| Some(w.run_id.as_str()) == args["run_id"].as_str())
                {
                    bail!("stale_task");
                }
                self.stopping = true;
                self.close_questions("task_stopped");
                self.waiting_paused = self.waiting.is_some();
            }
            "cancel_waiting" | "execute_waiting" => {
                if !self
                    .waiting
                    .as_ref()
                    .is_some_and(|w| Some(w.run_id.as_str()) == args["run_id"].as_str())
                {
                    bail!("stale_waiting_task");
                }
                if action == "cancel_waiting" {
                    let waiting = self.waiting.take().unwrap();
                    let record = super::task_records::TaskRecord::capture(
                        self,
                        &waiting,
                        "cancelled",
                        false,
                    );
                    self.last_task = record.summary();
                    self.pending_records.push(record);
                    self.record_last_task();
                    self.waiting_paused = false;
                    self.execute_after_stop = false;
                } else if self.active.is_some() {
                    self.stopping = true;
                    self.close_questions("task_stopped");
                    self.execute_after_stop = true;
                } else {
                    if self.context_lost {
                        bail!("context_reset_required");
                    }
                    let waiting = self.waiting.take().unwrap();
                    self.waiting_paused = false;
                    self.start(waiting.clone());
                    return Ok(Some(waiting));
                }
            }
            "set_model" => {
                if self.active.is_some() || self.waiting.is_some() {
                    bail!("session_busy");
                }
                let model = args["model_id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 512)
                    .context("model_id_required")?;
                if !model_options(&self.options)
                    .iter()
                    .any(|v| v["id"] == model)
                {
                    bail!("model_not_advertised");
                }
                self.selected_model = Some(model.to_owned());
                self.configuration_version = 1;
            }
            "reset_context" => {
                if self.active.is_some() {
                    bail!("session_busy");
                }
                if !self.context_lost || args["confirmed"].as_bool() != Some(true) {
                    bail!("context_reset_confirmation_required");
                }
                self.native_session_id = None;
                self.native_created_at_ms = None;
                self.context_lost = false;
            }
            _ => bail!("unknown_acp_command"),
        }
        Ok(None)
    }
    pub fn snapshot(&self) -> Value {
        let item = |w: &Work| json!({"run_id":w.run_id,"task_id":w.task.task_id,"source_message_id":w.task.correlation().source_message_id,"requester_did":w.task.requester_did});
        json!({"schema":"awiki.acp.session.v1","session_key":self.key,"agent_did":self.agent_did,
            "conversation_id":self.conversation_id,"revision":self.revision,"group":self.group,
            "task_history_available":true,
            "active":self.active.as_ref().map(item),"waiting":self.waiting.as_ref().map(item),
            "waiting_paused":self.waiting_paused,"stopping":self.stopping,"restoring":self.restoring,"context_lost":self.context_lost,
            "last_task":self.last_task,"history":self.history,"output_run_id":self.last_run_id,"text":self.text,"tools":self.tools,"omitted_tool_count":self.omitted_tool_count,
            "error_code":self.interaction_error,"error_details":self.error_details,
            "questions":self.questions.iter().map(|q| super::task_records::question_record(q,self.active.is_none())).collect::<Vec<_>>(),
            "capabilities":self.capabilities,"models":model_options(&self.options),"model_id":self.model,"selected_model_id":self.model_selection(),"model_configuration_ready":self.model_configuration_ready,
            "model_refresh_supported":!self.group,"model_catalog_updated_at_ms":self.model_catalog_updated_at_ms})
    }
    pub fn model_selection(&self) -> Option<String> {
        if self.configuration_version == 0 {
            self.model.clone()
        } else {
            self.selected_model.clone()
        }
    }
    pub fn update_configuration(&mut self, options: Value) {
        self.model_configuration_ready = true;
        // Before effective-model reporting, `model` represented the selected
        // intent. Preserve that intent once when opening an older session.
        if self.configuration_version == 0 {
            self.selected_model = self.model.clone();
            self.configuration_version = 1;
        }
        self.model = super::models::current_model(&options);
        self.update_catalog(options);
    }
    /// A catalog read does not confirm a model switch or replace session intent.
    pub fn update_catalog(&mut self, options: Value) {
        self.model_catalog_updated_at_ms = current_time_millis().ok();
        self.options = options;
    }
    fn close_questions(&mut self, reason: &str) {
        for question in &mut self.questions {
            if question.pending() {
                question.end_reason = Some(reason.into());
            }
        }
    }
    fn record_last_task(&mut self) {
        if self.history.len() == 200 {
            self.history.remove(0);
        }
        self.history.push(self.last_task.clone());
    }
}

pub fn session_key(agent: &str, owner_scope: &str, scope: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(agent, owner_scope, scope)).expect("string tuple"))
    )
}

pub use super::models::model_options;

pub fn validate_answer(request: &Value, answer: &Value) -> Result<()> {
    super::questions::validate_schema(request)?;
    match answer["action"].as_str() {
        Some("decline" | "cancel") => return Ok(()),
        Some("accept") => {}
        _ => bail!("invalid_answer_action"),
    }
    let content = answer["content"]
        .as_object()
        .context("answer_content_required")?;
    let schema = &request["requestedSchema"];
    if let Some(required) = schema["required"].as_array() {
        for field in required {
            if !content.contains_key(field.as_str().context("invalid_question_schema")?) {
                bail!("answer_required_field");
            }
        }
    }
    let properties = schema["properties"]
        .as_object()
        .context("unsupported_question_schema")?;
    for (key, value) in content {
        let p = properties.get(key).context("unknown_answer_field")?;
        let valid = match p["type"].as_str() {
            Some("string") => value.as_str().is_some_and(|s| {
                s.len() <= 16_384
                    && p["minLength"]
                        .as_u64()
                        .is_none_or(|n| s.chars().count() as u64 >= n)
                    && p["maxLength"]
                        .as_u64()
                        .is_none_or(|n| s.chars().count() as u64 <= n)
            }),
            Some("boolean") => value.is_boolean(),
            Some("integer") => value.is_i64() || value.is_u64(),
            Some("number") => value.is_number(),
            Some("array") => value
                .as_array()
                .is_some_and(|a| a.len() <= 64 && a.iter().all(Value::is_string)),
            _ => false,
        };
        if !valid {
            bail!("invalid_answer_type");
        }
        if let Some(choices) = p["enum"].as_array() {
            if !choices.contains(value) {
                bail!("invalid_answer_choice");
            }
        }
        if let Some(choices) = p["oneOf"].as_array() {
            if !choices.iter().any(|v| v["const"] == *value) {
                bail!("invalid_answer_choice");
            }
        }
        if let Some(choices) = p["items"]["enum"].as_array() {
            if value
                .as_array()
                .is_some_and(|a| a.iter().any(|v| !choices.contains(v)))
            {
                bail!("invalid_answer_choice");
            }
        }
        if let Some(number) = value.as_f64() {
            if p["minimum"].as_f64().is_some_and(|min| number < min)
                || p["maximum"].as_f64().is_some_and(|max| number > max)
            {
                bail!("answer_out_of_range");
            }
        }
        super::questions::validate_property(p, value)?;
    }
    Ok(())
}

pub(crate) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS acp_probes (profile_id TEXT PRIMARY KEY, report TEXT NOT NULL, checked_at_ms INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS acp_attachments (agent_did TEXT NOT NULL, message_id TEXT NOT NULL, items TEXT NOT NULL, PRIMARY KEY(agent_did,message_id));
        CREATE TABLE IF NOT EXISTS acp_sessions (session_key TEXT PRIMARY KEY, agent_did TEXT NOT NULL, data TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS acp_events (event_id TEXT PRIMARY KEY, session_key TEXT NOT NULL, run_id TEXT NOT NULL, snapshot TEXT NOT NULL, sent INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS acp_commands (command_id TEXT NOT NULL, agent_did TEXT NOT NULL, request TEXT NOT NULL, result TEXT NOT NULL, PRIMARY KEY(agent_did,command_id));")?;
    let columns = db
        .prepare("PRAGMA table_info(acp_events)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|column| column == "event_kind") {
        db.execute(
            "ALTER TABLE acp_events ADD COLUMN event_kind TEXT NOT NULL DEFAULT 'snapshot'",
            [],
        )?;
    }
    super::task_records::initialize(db)?;
    Ok(())
}

pub fn load(state: &DaemonState, key: &str) -> Result<Session> {
    let data: String = state
        .connection()?
        .query_row(
            "SELECT data FROM acp_sessions WHERE session_key=?1",
            [key],
            |r| r.get(0),
        )
        .context("acp_session_not_found")?;
    Ok(serde_json::from_str(&data)?)
}

pub fn mutate<T>(
    state: &DaemonState,
    key: &str,
    seed: Option<Session>,
    f: impl FnOnce(&mut Session) -> Result<T>,
) -> Result<T> {
    let mut db = state.connection()?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let row: Option<String> = tx
        .query_row(
            "SELECT data FROM acp_sessions WHERE session_key=?1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    let mut session = match &row {
        Some(row) => serde_json::from_str(row)?,
        None => seed.context("acp_session_not_found")?,
    };
    let result = f(&mut session)?;
    if row.as_deref() != Some(serde_json::to_string(&session)?.as_str()) {
        save(&tx, &mut session)?;
    }
    tx.commit()?;
    Ok(result)
}

/// Serialize model completion against stop/execute-waiting commands. A stop
/// accepted first prevents final delivery; a committed final makes stop stale.
pub fn finish_with_final(
    state: &DaemonState,
    key: &str,
    record: &crate::state::RuntimeFinalOutboxRecord,
) -> Result<(bool, Option<Work>)> {
    let mut db = state.connection()?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let raw: String = tx.query_row(
        "SELECT data FROM acp_sessions WHERE session_key=?1",
        [key],
        |row| row.get(0),
    )?;
    let mut session: Session = serde_json::from_str(&raw)?;
    if !session.active_run(&record.run_id) {
        bail!("stale_task");
    }
    if session.agent_did != record.agent_did
        || session.controller_scope_key != record.controller_scope_key
        || session.conversation_id != record.conversation_id
    {
        bail!("conversation_mismatch");
    }
    let cancelled = session.stopping;
    if !cancelled {
        DaemonState::upsert_runtime_final_outbox_pending_in(&tx, record)?;
    }
    let next = session.complete(
        &record.run_id,
        if cancelled { "cancelled" } else { "finished" },
    )?;
    save(&tx, &mut session)?;
    tx.commit()?;
    Ok((cancelled, next))
}

pub(super) fn save(db: &Connection, s: &mut Session) -> Result<()> {
    s.revision += 1;
    for mut record in std::mem::take(&mut s.pending_records) {
        record.revision = s.revision;
        if s.last_task["run_id"] == record.run_id {
            s.last_task = record.summary();
        }
        for entry in &mut s.history {
            if entry["run_id"] == record.run_id {
                *entry = record.summary();
            }
        }
        super::task_records::persist(db, &record)?;
    }
    if let Some(active) = &s.active {
        super::task_records::persist(
            db,
            &super::task_records::TaskRecord::capture(
                s,
                active,
                if s.stopping { "stopping" } else { "running" },
                true,
            ),
        )?;
    }
    if let Some(waiting) = &s.waiting {
        super::task_records::persist(
            db,
            &super::task_records::TaskRecord::capture(
                s,
                waiting,
                if s.waiting_paused {
                    "paused"
                } else {
                    "waiting"
                },
                false,
            ),
        )?;
    }
    db.execute("INSERT INTO acp_sessions(session_key,agent_did,data) VALUES(?1,?2,?3) ON CONFLICT(session_key) DO UPDATE SET data=excluded.data",params![s.key,s.agent_did,serde_json::to_string(s)?])?;
    if let Some(run) = &s.last_run_id {
        // A snapshot is complete: retain the newest undelivered revision rather
        // than queueing quadratic copies of every streamed text fragment.
        db.execute(
            "DELETE FROM acp_events WHERE session_key=?1 AND sent=0 AND event_kind='snapshot'",
            [&s.key],
        )?;
        db.execute(
            "INSERT INTO acp_events(event_id,session_key,run_id,snapshot) VALUES(?1,?2,?3,?4)",
            params![
                format!("acp:{}:{}", s.key, s.revision),
                s.key,
                run,
                serde_json::to_string(&s.snapshot())?
            ],
        )?;
    }
    Ok(())
}

pub fn control(
    state: &DaemonState,
    agent: &str,
    sender: &str,
    command_id: &str,
    args: &Value,
) -> Result<(Value, Option<Work>)> {
    let key = args["session_key"]
        .as_str()
        .context("session_key_required")?;
    let mut db = state.connection()?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let fingerprint = serde_json::to_string(&(sender, args))?;
    if let Some((prior, result)) = tx
        .query_row(
            "SELECT request,result FROM acp_commands WHERE agent_did=?1 AND command_id=?2",
            params![agent, command_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        if prior != fingerprint {
            bail!("command_id_conflict");
        }
        return Ok((serde_json::from_str(&result)?, None));
    }
    let raw: String = tx
        .query_row(
            "SELECT data FROM acp_sessions WHERE session_key=?1 AND agent_did=?2",
            params![key, agent],
            |r| r.get(0),
        )
        .context("acp_session_not_found")?;
    let mut session: Session = serde_json::from_str(&raw)?;
    let profile = state.load_runtime_agent_profile(agent)?;
    if session.controller_scope_key != profile.controller_scope_key {
        bail!("controller_scope_changed")
    }
    // Task/question identities fence controls during streaming; a newer text
    // revision must not invalidate a stop or an answer to that same question.
    // Session-wide mutations still require the exact observed revision.
    let action = args["action"].as_str().context("action_required")?;
    if matches!(action, "set_model" | "reset_context")
        && args["revision"].as_u64() != Some(session.revision)
    {
        bail!("stale_revision");
    }
    if args["revision"]
        .as_u64()
        .is_none_or(|v| v > session.revision)
    {
        bail!("stale_revision");
    }
    let work = session.command(
        args["action"].as_str().context("action_required")?,
        args,
        sender,
        current_time_millis()?,
    )?;
    if session.stopping {
        if let Some(active) = &session.active {
            tx.execute(
                "UPDATE runtime_rpc_tokens SET revoked_at_ms=?1 WHERE run_id=?2",
                params![current_time_millis()?, active.run_id],
            )?;
        }
    }
    if action == "cancel_waiting" {
        tx.execute(
            "UPDATE runtime_run SET status='failed' WHERE run_id=?1 AND status='pending'",
            [args["run_id"].as_str().context("run_id_required")?],
        )?;
    }
    save(&tx, &mut session)?;
    let result = session.snapshot();
    tx.execute(
        "INSERT INTO acp_commands(command_id,agent_did,request,result) VALUES(?1,?2,?3,?4)",
        params![
            command_id,
            agent,
            fingerprint,
            serde_json::to_string(&result)?
        ],
    )?;
    tx.commit()?;
    Ok((result, work))
}

pub fn recover(state: &DaemonState) -> Result<usize> {
    revoke_active_tokens(state)?;
    let db = state.connection()?;
    let keys = db
        .prepare("SELECT session_key FROM acp_sessions")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut count = 0;
    for key in keys {
        let s = load(state, &key)?;
        if s.active.is_none() && s.waiting.is_none() {
            continue;
        }
        mutate(state, &key, None, |s| {
            s.close_questions("task_interrupted");
            if let Some(active) = s.active.take() {
                let record =
                    super::task_records::TaskRecord::capture(s, &active, "interrupted", true);
                s.last_task = record.summary();
                s.pending_records.push(record);
                s.record_last_task();
            }
            s.waiting_paused = s.waiting.is_some();
            s.stopping = false;
            s.restoring = false;
            s.execute_after_stop = false;
            s.questions.clear();
            Ok(())
        })?;
        count += 1;
    }
    Ok(count)
}

fn revoke_active_tokens(state: &DaemonState) -> Result<()> {
    state.connection()?.execute(
        "UPDATE runtime_rpc_tokens SET revoked_at_ms=?1 WHERE revoked_at_ms IS NULL AND run_id IN (SELECT run_id FROM runtime_run WHERE runtime_plugin_id='acp')",
        [current_time_millis()?],
    )?;
    Ok(())
}

/// Daemon shutdown cancels ACP work and pauses waiting items before joining
/// foreground workers. Legacy runtimes retain their existing shutdown path.
pub fn request_shutdown(state: &DaemonState) -> Result<()> {
    let db = state.connection()?;
    let keys = db
        .prepare("SELECT session_key FROM acp_sessions")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for key in keys {
        mutate(state, &key, None, |session| {
            if session.active.is_some() {
                session.stopping = true;
                session.close_questions("task_interrupted");
                session.execute_after_stop = false;
                session.waiting_paused = session.waiting.is_some();
            }
            Ok(())
        })?;
    }
    revoke_active_tokens(state)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
