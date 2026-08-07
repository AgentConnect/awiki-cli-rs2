use super::*;

impl DaemonState {
    /// Return the closed, identity-free aggregate exposed by status/ready.
    ///
    /// V2 readiness is true only when every currently active VNext Agent has
    /// reached that checkpoint. A missing per-Agent probe row is deliberately
    /// counted as false so one healthy Agent cannot mask another failed Agent.
    pub fn load_sync_probe(&self) -> Result<DaemonSyncProbe> {
        let connection = self.connection()?;
        let (active_count, negotiated_count, bootstrap_count, reconcile_count) = connection
            .query_row(
                r#"
SELECT
    COUNT(*),
    COALESCE(SUM(CASE WHEN probe.v2_subprotocol_negotiated = 1 THEN 1 ELSE 0 END), 0),
    COALESCE(SUM(CASE WHEN probe.v2_bootstrap_completed = 1 THEN 1 ELSE 0 END), 0),
    COALESCE(SUM(CASE WHEN probe.last_reconcile_protocol = 'sync_v2' THEN 1 ELSE 0 END), 0)
FROM agent_definition definition
LEFT JOIN agent_device_identity active
  ON active.agent_did = definition.agent_did
 AND active.identity_status = 'active'
LEFT JOIN agent_sync_probe probe
  ON probe.agent_did = active.agent_did
WHERE definition.status = 'active'
"#,
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )?;
        let legacy_sync_used = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_sync_probe WHERE legacy_sync_used = 1)",
            [],
            |row| row.get::<_, i64>(0),
        )? != 0;
        let all_negotiated = active_count > 0 && negotiated_count == active_count;
        let all_reconciled =
            active_count > 0 && bootstrap_count == active_count && reconcile_count == active_count;
        let probe = DaemonSyncProbe {
            v2_subprotocol_negotiated: all_negotiated,
            v2_bootstrap_completed: all_reconciled,
            last_reconcile_protocol: all_reconciled.then(|| "sync_v2".to_owned()),
            legacy_sync_used,
        };
        validate_sync_probe(&probe)?;
        Ok(probe)
    }

    pub fn mark_v2_subprotocol_negotiated(&self, agent_did: &str) -> Result<()> {
        self.update_agent_sync_probe(agent_did, "v2_subprotocol_negotiated")
    }

    /// Clear the process-local negotiation checkpoint for every Agent.
    ///
    /// A successful WebSocket subprotocol negotiation only proves the current
    /// Daemon process/session. Bootstrap and HTTP reconcile checkpoints remain
    /// durable, but a new foreground boot must negotiate V2 again.
    pub fn reset_v2_subprotocol_negotiation_for_boot(&self) -> Result<usize> {
        let changed = self.connection()?.execute(
            r#"
UPDATE agent_sync_probe
SET v2_subprotocol_negotiated = 0,
    updated_at_ms = ?1
WHERE v2_subprotocol_negotiated != 0
"#,
            rusqlite::params![current_time_millis()?],
        )?;
        Ok(changed)
    }

    /// Clear one Agent's current-session negotiation checkpoint after its
    /// realtime session disconnects or ends.
    pub fn clear_v2_subprotocol_negotiated(&self, agent_did: &str) -> Result<()> {
        let agent_did = agent_did.trim();
        if agent_did.is_empty() {
            bail!("agent_did must not be empty");
        }
        self.connection()?.execute(
            r#"
UPDATE agent_sync_probe
SET v2_subprotocol_negotiated = 0,
    updated_at_ms = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, current_time_millis()?],
        )?;
        Ok(())
    }

    pub fn mark_sync_v2_reconcile_completed(&self, agent_did: &str) -> Result<()> {
        self.update_agent_sync_probe(agent_did, "v2_reconcile_completed")
    }

    pub fn mark_legacy_sync_used(&self, agent_did: &str) -> Result<()> {
        self.update_agent_sync_probe(agent_did, "legacy_sync_used")
    }

    fn update_agent_sync_probe(&self, agent_did: &str, event: &str) -> Result<()> {
        let agent_did = agent_did.trim();
        if agent_did.is_empty() {
            bail!("agent_did must not be empty");
        }
        let (negotiated, bootstrap, protocol, legacy) = match event {
            "v2_subprotocol_negotiated" => (1_i64, 0_i64, None, 0_i64),
            "v2_reconcile_completed" => (0_i64, 1_i64, Some("sync_v2"), 0_i64),
            "legacy_sync_used" => (0_i64, 0_i64, None, 1_i64),
            _ => bail!("unsupported Agent sync probe event"),
        };
        self.connection()?.execute(
            r#"
INSERT INTO agent_sync_probe (
    agent_did, v2_subprotocol_negotiated, v2_bootstrap_completed,
    last_reconcile_protocol, legacy_sync_used, updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT(agent_did) DO UPDATE SET
    v2_subprotocol_negotiated = MAX(
        agent_sync_probe.v2_subprotocol_negotiated,
        excluded.v2_subprotocol_negotiated
    ),
    v2_bootstrap_completed = MAX(
        agent_sync_probe.v2_bootstrap_completed,
        excluded.v2_bootstrap_completed
    ),
    last_reconcile_protocol = COALESCE(
        excluded.last_reconcile_protocol,
        agent_sync_probe.last_reconcile_protocol
    ),
    legacy_sync_used = MAX(agent_sync_probe.legacy_sync_used, excluded.legacy_sync_used),
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                agent_did,
                negotiated,
                bootstrap,
                protocol,
                legacy,
                current_time_millis()?
            ],
        )?;
        Ok(())
    }
}

fn validate_sync_probe(probe: &DaemonSyncProbe) -> Result<()> {
    if probe
        .last_reconcile_protocol
        .as_deref()
        .is_some_and(|protocol| protocol != "sync_v2")
    {
        bail!("unsupported daemon sync reconcile protocol");
    }
    if probe.v2_bootstrap_completed != (probe.last_reconcile_protocol.as_deref() == Some("sync_v2"))
    {
        bail!("completed v2 bootstrap and reconcile protocol must advance together");
    }
    Ok(())
}
