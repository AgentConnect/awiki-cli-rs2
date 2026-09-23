//! One-way retirement of pre-ACP runtimes. History and native data remain intact.
use anyhow::{bail, Result};
use rusqlite::{Connection, OptionalExtension};

use super::DaemonState;

pub const LEGACY_RUNTIME_DISABLED: &str = "legacy_runtime_disabled_recreate_required";

pub(crate) fn is_legacy_runtime(plugin: &str) -> bool {
    matches!(
        plugin,
        "runtime.hermes"
            | "hermes"
            | "generic-cli"
            | "runtime.cli.codex"
            | "runtime.cli.claude-code"
            | "runtime.cli.gemini-cli"
    )
}

pub(super) fn initialize(db: &Connection) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS runtime_retirement (
        agent_did TEXT PRIMARY KEY,
        runtime_profile_id TEXT NOT NULL,
        original_plugin_id TEXT NOT NULL,
        reason TEXT NOT NULL,
        retired_at_ms INTEGER NOT NULL
    );",
    )?;
    let now = crate::security::runtime_token::current_time_millis()?;
    tx.execute("INSERT OR IGNORE INTO runtime_retirement
        SELECT agent_did,runtime_profile_id,runtime_plugin_id,?1,?2 FROM runtime_profile
        WHERE agent_did IS NOT NULL AND runtime_plugin_id IN
        ('runtime.hermes','hermes','generic-cli','runtime.cli.codex','runtime.cli.claude-code','runtime.cli.gemini-cli')",
        rusqlite::params![LEGACY_RUNTIME_DISABLED, now])?;
    // Apply the fence on every open: older metadata migration or inventory
    // replay cannot reactivate retired work. No final/message body is deleted.
    for table in ["agent_definition", "runtime_profile"] {
        tx.execute(
            &format!(
                "UPDATE {table} SET status='retired',updated_at=?1
            WHERE agent_did IN (SELECT agent_did FROM runtime_retirement)
            AND status NOT IN ('archived','retired')"
            ),
            [now],
        )?;
    }
    for table in ["cli_runtime_profile", "hermes_profiles"] {
        tx.execute(
            &format!(
                "UPDATE {table} SET status='retired',updated_at_ms=?1
            WHERE runtime_profile_id IN (SELECT runtime_profile_id FROM runtime_retirement)
            AND status NOT IN ('archived','retired')"
            ),
            [now],
        )?;
    }
    tx.execute(
        "UPDATE runtime_run SET status='failed',completed_at=?1,updated_at=?1,
        completed_at_ms=?1,updated_at_ms=?1
        WHERE agent_did IN (SELECT agent_did FROM runtime_retirement)
        AND status IN ('pending','running')",
        [now],
    )?;
    tx.execute(
        "UPDATE runtime_task SET status='failed',updated_at_ms=?1
        WHERE agent_did IN (SELECT agent_did FROM runtime_retirement)
        AND status IN ('created','pending','running')",
        [now],
    )?;
    tx.execute(
        "UPDATE runtime_rpc_tokens SET revoked_at=?1,revoked_at_ms=?1
        WHERE agent_did IN (SELECT agent_did FROM runtime_retirement) AND revoked_at_ms IS NULL",
        [now],
    )?;
    tx.execute(
        "UPDATE runtime_retry_queue SET status='cancelled',updated_at_ms=?1
        WHERE agent_did IN (SELECT agent_did FROM runtime_retirement)
        AND status IN ('queued','running')",
        [now],
    )?;
    tx.execute(
        "UPDATE cli_route_message_queue SET status='cancelled',updated_at_ms=?1,
        last_error_code=?2,last_error_summary='Legacy runtime retired; recreate the Agent'
        WHERE agent_did IN (SELECT agent_did FROM runtime_retirement)
        AND status IN ('queued','running')",
        rusqlite::params![now, LEGACY_RUNTIME_DISABLED],
    )?;
    tx.execute(
        "UPDATE runtime_final_outbox SET status='failed_terminal',updated_at_ms=?1,
        last_error_code=?2,last_error_summary='Legacy runtime retired; delivery is fenced'
        WHERE agent_did IN (SELECT agent_did FROM runtime_retirement)
        AND status IN ('pending','sending')",
        rusqlite::params![now, LEGACY_RUNTIME_DISABLED],
    )?;
    tx.execute(
        "UPDATE app_personal_agent_binding SET status='personal_agent_retired',
        revoked_at_ms=?1,updated_at_ms=?1
        WHERE runtime_agent_did IN (SELECT agent_did FROM runtime_retirement)
        AND revoked_at_ms IS NULL",
        [now],
    )?;
    tx.execute("UPDATE message_sync_outbox SET status='failed_terminal',updated_at_ms=?1,
        last_error_code=?2,last_error_summary='Legacy runtime retired; delivery is fenced'
        WHERE status IN ('pending','sending') AND json_valid(payload_json)
        AND json_extract(payload_json,'$.runtime_agent_did') IN (SELECT agent_did FROM runtime_retirement)",
        rusqlite::params![now, LEGACY_RUNTIME_DISABLED])?;
    tx.commit()?;
    Ok(())
}

impl DaemonState {
    pub fn runtime_retirement_reason(&self, agent_did: &str) -> Result<Option<String>> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT reason FROM runtime_retirement WHERE agent_did=?1",
                [agent_did],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn require_runtime_not_retired(&self, agent_did: &str) -> Result<()> {
        if self.runtime_retirement_reason(agent_did)?.is_some() {
            bail!(LEGACY_RUNTIME_DISABLED);
        }
        Ok(())
    }

    pub(crate) fn require_personal_binding_not_retired(&self, binding_id: &str) -> Result<()> {
        let retired: bool = self.connection()?.query_row(
            "SELECT EXISTS(
            SELECT 1 FROM app_personal_agent_binding b JOIN runtime_retirement r
            ON b.runtime_agent_did=r.agent_did WHERE b.binding_id=?1)",
            [binding_id],
            |r| r.get(0),
        )?;
        if retired {
            bail!(LEGACY_RUNTIME_DISABLED);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "runtime_retirement_tests.rs"]
mod tests;
