use super::row_mappers::*;
use super::*;

impl DaemonState {
    pub fn upsert_runtime_agent_profile(&self, profile: &RuntimeAgentProfile) -> Result<()> {
        self.upsert_runtime_agent_profile_with_handle(profile, &profile.agent_handle)
    }

    pub fn upsert_runtime_agent_profile_with_handle(
        &self,
        profile: &RuntimeAgentProfile,
        handle: &str,
    ) -> Result<()> {
        profile.validate()?;
        let (local_agent_db_path, message_db_path) = agent_data_paths(&profile.agent_did)?;
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
            r#"
INSERT INTO agent_definition (
    agent_did,
    handle,
    agent_kind,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, 'runtime', ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'default', ?10, ?11, 'active', ?12, ?12)
ON CONFLICT(agent_did) DO UPDATE SET
    controller_user_id = excluded.controller_user_id,
    controller_full_handle = excluded.controller_full_handle,
    controller_scope_key = excluded.controller_scope_key,
    controller_did = excluded.controller_did,
    handle = excluded.handle,
    agent_kind = excluded.agent_kind,
    runtime_plugin_id = excluded.runtime_plugin_id,
    runtime_profile_id = excluded.runtime_profile_id,
    workspace_id = excluded.workspace_id,
    local_agent_db_path = excluded.local_agent_db_path,
    message_db_path = excluded.message_db_path,
    status = 'active',
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                profile.agent_did,
                handle,
                profile.controller_user_id,
                profile.controller_full_handle,
                profile.controller_scope_key,
                profile.controller_did,
                profile.runtime_plugin_id,
                profile.runtime_profile_id,
                profile.workspace_id,
                local_agent_db_path,
                message_db_path,
                now,
            ],
        )?;
        connection.execute(
            r#"
INSERT INTO runtime_profile (
    runtime_profile_id,
    agent_did,
    runtime_plugin_id,
    display_name,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)
ON CONFLICT(runtime_profile_id) DO UPDATE SET
    agent_did = excluded.agent_did,
    runtime_plugin_id = excluded.runtime_plugin_id,
    display_name = excluded.display_name,
    status = 'active',
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                profile.runtime_profile_id,
                profile.agent_did,
                profile.runtime_plugin_id,
                profile.display_name,
                now,
            ],
        )?;
        if let (Some(workspace_id), Some(workspace_root), Some(workspace_mode)) = (
            profile.workspace_id.as_deref(),
            profile.workspace_root.as_ref(),
            profile.workspace_mode,
        ) {
            self.upsert_workspace_binding(
                &profile.agent_did,
                &profile.runtime_profile_id,
                &WorkspaceBindingConfig {
                    workspace_id: workspace_id.to_string(),
                    workspace_root: workspace_root.clone(),
                    workspace_mode,
                },
            )?;
        }
        Ok(())
    }

    pub fn upsert_agent_definition(&self, definition: &AgentDefinition) -> Result<()> {
        definition.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
            r#"
INSERT INTO agent_definition (
    agent_did,
    handle,
    agent_kind,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
ON CONFLICT(agent_did) DO UPDATE SET
    handle = excluded.handle,
    agent_kind = excluded.agent_kind,
    controller_user_id = excluded.controller_user_id,
    controller_full_handle = excluded.controller_full_handle,
    controller_scope_key = excluded.controller_scope_key,
    controller_did = excluded.controller_did,
    runtime_plugin_id = excluded.runtime_plugin_id,
    runtime_profile_id = excluded.runtime_profile_id,
    workspace_id = excluded.workspace_id,
    policy_id = excluded.policy_id,
    local_agent_db_path = excluded.local_agent_db_path,
    message_db_path = excluded.message_db_path,
    status = excluded.status,
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                definition.agent_did,
                definition.handle,
                definition.agent_kind.as_str(),
                definition.controller_user_id,
                definition.controller_full_handle,
                definition.controller_scope_key,
                definition.controller_did,
                definition.runtime_plugin_id,
                definition.runtime_profile_id,
                definition.workspace_id,
                definition.policy_id,
                definition.local_agent_db_path,
                definition.message_db_path,
                definition.status,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn update_controller_did_for_agent_family(
        &self,
        daemon_agent_did: &str,
        controller_did: &str,
    ) -> Result<usize> {
        if daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        if controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let mut updated = 0usize;
        updated += connection.execute(
            r#"
UPDATE agent_definition
SET controller_did = ?1,
    updated_at = ?2
WHERE agent_did = ?3
   OR agent_did IN (
       SELECT runtime_agent_did
       FROM runtime_daemon_binding
       WHERE daemon_agent_did = ?3
   )
"#,
            rusqlite::params![controller_did, now.to_string(), daemon_agent_did],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_daemon_binding
SET controller_did = ?1,
    updated_at_ms = ?2
WHERE daemon_agent_did = ?3
"#,
            rusqlite::params![controller_did, now, daemon_agent_did],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_task
SET controller_did = ?1,
    updated_at_ms = ?2
WHERE agent_did IN (
    SELECT runtime_agent_did
    FROM runtime_daemon_binding
    WHERE daemon_agent_did = ?3
)
  AND status IN ('pending', 'running')
"#,
            rusqlite::params![controller_did, now, daemon_agent_did],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_agent_create_request
SET controller_did = ?1,
    updated_at_ms = ?2
WHERE daemon_agent_did = ?3
"#,
            rusqlite::params![controller_did, now, daemon_agent_did],
        )?;
        updated += connection.execute(
            r#"
UPDATE agent_status_query_throttle
SET controller_did = ?1
WHERE daemon_agent_did = ?2
"#,
            rusqlite::params![controller_did, daemon_agent_did],
        )?;
        Ok(updated)
    }

    pub fn mark_agent_archived(&self, agent_did: &str) -> Result<usize> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        let connection = self.connection()?;
        let now_ms = current_time_millis()?;
        let now = now_ms.to_string();
        let mut updated = 0usize;
        updated += connection.execute(
            r#"
UPDATE agent_definition
SET status = 'archived',
    updated_at = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, now],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_profile
SET status = 'archived',
    updated_at = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, now],
        )?;
        updated += connection.execute(
            r#"
UPDATE workspace_binding
SET status = 'archived',
    updated_at = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, now],
        )?;
        updated += connection.execute(
            r#"
UPDATE cli_runtime_profile
SET status = 'archived',
    updated_at_ms = ?2
WHERE runtime_profile_id IN (
    SELECT runtime_profile_id
    FROM agent_definition
    WHERE agent_did = ?1
      AND runtime_profile_id IS NOT NULL
)
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        updated += connection.execute(
            r#"
UPDATE hermes_profiles
SET status = 'archived',
    updated_at_ms = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        updated += connection.execute(
            r#"
UPDATE hermes_native_sessions
SET status = 'archived',
    updated_at_ms = ?2
WHERE agent_did = ?1
  AND status = 'active'
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_task
SET status = 'archived',
    updated_at_ms = ?2
WHERE agent_did = ?1
  AND status IN ('created', 'pending', 'running')
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_retry_queue
SET status = 'archived',
    updated_at_ms = ?2
WHERE agent_did = ?1
  AND status IN ('queued', 'running')
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        Ok(updated)
    }

    pub fn upsert_cli_runtime_profile(&self, profile: &CliRuntimeProfileRecord) -> Result<()> {
        profile.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO cli_runtime_profile (
    runtime_profile_id,
    driver_id,
    binary_path,
    config_home,
    auth_mode,
    default_model,
    default_sandbox,
    default_workspace_mode,
    recipient_policy_json,
    driver_config_json,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
ON CONFLICT(runtime_profile_id) DO UPDATE SET
    driver_id = excluded.driver_id,
    binary_path = excluded.binary_path,
    config_home = excluded.config_home,
    auth_mode = excluded.auth_mode,
    default_model = excluded.default_model,
    default_sandbox = excluded.default_sandbox,
    default_workspace_mode = excluded.default_workspace_mode,
    recipient_policy_json = excluded.recipient_policy_json,
    driver_config_json = excluded.driver_config_json,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                profile.runtime_profile_id,
                profile.driver_id,
                profile
                    .binary_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                profile
                    .config_home
                    .as_ref()
                    .map(|path| path.display().to_string()),
                profile.auth_mode,
                profile.default_model,
                profile.default_sandbox,
                profile.default_workspace_mode.as_str(),
                profile.recipient_policy_json.to_string(),
                profile.driver_config_json.to_string(),
                profile.status,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_cli_runtime_profile(
        &self,
        runtime_profile_id: &str,
    ) -> Result<CliRuntimeProfileRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    runtime_profile_id,
    driver_id,
    binary_path,
    config_home,
    auth_mode,
    default_model,
    default_sandbox,
    default_workspace_mode,
    recipient_policy_json,
    driver_config_json,
    status
FROM cli_runtime_profile
WHERE runtime_profile_id = ?1
"#,
                [runtime_profile_id],
                cli_runtime_profile_from_row,
            )
            .with_context(|| format!("load CLI runtime profile {runtime_profile_id}"))
    }

    pub fn list_cli_runtime_profiles(&self) -> Result<Vec<CliRuntimeProfileRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    runtime_profile_id,
    driver_id,
    binary_path,
    config_home,
    auth_mode,
    default_model,
    default_sandbox,
    default_workspace_mode,
    recipient_policy_json,
    driver_config_json,
    status
FROM cli_runtime_profile
ORDER BY runtime_profile_id ASC
"#,
        )?;
        let rows = statement.query_map([], cli_runtime_profile_from_row)?;
        let mut profiles = Vec::new();
        for row in rows {
            profiles.push(row?);
        }
        Ok(profiles)
    }

    pub fn upsert_hermes_profile(&self, profile: &HermesProfileRecord) -> Result<()> {
        profile.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO hermes_profiles (
    agent_did,
    runtime_profile_id,
    hermes_profile,
    hermes_home,
    hermes_version,
    awiki_skills_version,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
ON CONFLICT(agent_did) DO UPDATE SET
    runtime_profile_id = excluded.runtime_profile_id,
    hermes_profile = excluded.hermes_profile,
    hermes_home = excluded.hermes_home,
    hermes_version = excluded.hermes_version,
    awiki_skills_version = excluded.awiki_skills_version,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                profile.agent_did,
                profile.runtime_profile_id,
                profile.hermes_profile,
                profile.hermes_home.display().to_string(),
                profile.hermes_version,
                profile.awiki_skills_version,
                profile.status,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_hermes_profile(&self, agent_did: &str) -> Result<HermesProfileRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    agent_did,
    runtime_profile_id,
    hermes_profile,
    hermes_home,
    hermes_version,
    awiki_skills_version,
    status
FROM hermes_profiles
WHERE agent_did = ?1
"#,
                [agent_did],
                |row| {
                    let hermes_home: String = row.get(3)?;
                    Ok(HermesProfileRecord {
                        agent_did: row.get(0)?,
                        runtime_profile_id: row.get(1)?,
                        hermes_profile: row.get(2)?,
                        hermes_home: PathBuf::from(hermes_home),
                        hermes_version: row.get(4)?,
                        awiki_skills_version: row.get(5)?,
                        status: row.get(6)?,
                    })
                },
            )
            .with_context(|| format!("load Hermes profile for agent {agent_did}"))
    }

    pub fn store_hermes_native_session(&self, session: &HermesNativeSessionRecord) -> Result<()> {
        session.validate()?;
        let connection = self.connection()?;
        connection
            .execute(
                r#"
INSERT INTO hermes_native_sessions (
    id,
    runtime_session_id,
    agent_did,
    agent_handle,
    runtime_profile_id,
    controller_scope_key,
    controller_did,
    session_actor_did,
    scope_kind,
    scope_key,
    conversation_id,
    route_key,
    hermes_profile,
    hermes_session_id,
    session_kind,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
ON CONFLICT(id) DO UPDATE SET
    runtime_session_id = excluded.runtime_session_id,
    agent_did = excluded.agent_did,
    agent_handle = excluded.agent_handle,
    runtime_profile_id = excluded.runtime_profile_id,
    controller_scope_key = excluded.controller_scope_key,
    controller_did = excluded.controller_did,
    session_actor_did = excluded.session_actor_did,
    scope_kind = excluded.scope_kind,
    scope_key = excluded.scope_key,
    conversation_id = excluded.conversation_id,
    route_key = excluded.route_key,
    hermes_profile = excluded.hermes_profile,
    hermes_session_id = excluded.hermes_session_id,
    session_kind = excluded.session_kind,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms
"#,
                rusqlite::params![
                    session.id,
                    session.runtime_session_id,
                    session.agent_did,
                    session.agent_handle,
                    session.runtime_profile_id,
                    session.controller_scope_key,
                    session.controller_did,
                    session.session_actor_did,
                    session.scope_kind,
                    session.scope_key,
                    session.conversation_id,
                    session.route_key,
                    session.hermes_profile,
                    session.hermes_session_id,
                    session.session_kind,
                    session.status,
                    session.created_at_ms,
                    session.updated_at_ms,
                ],
            )
            .with_context(|| format!("store Hermes native session {}", session.route_key))?;
        Ok(())
    }

    pub fn load_active_hermes_session_by_route(
        &self,
        route: &HermesSessionRoute,
    ) -> Result<Option<HermesNativeSessionRecord>> {
        route.validate()?;
        let route_key = route.route_key();
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    id,
    runtime_session_id,
    agent_did,
    agent_handle,
    runtime_profile_id,
    controller_scope_key,
    controller_did,
    session_actor_did,
    scope_kind,
    scope_key,
    conversation_id,
    route_key,
    hermes_profile,
    hermes_session_id,
    session_kind,
    status,
    created_at_ms,
    updated_at_ms
FROM hermes_native_sessions
WHERE route_key = ?1
  AND status = 'active'
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
        )?;
        let mut rows = statement.query([route_key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(hermes_native_session_from_row(row)?))
    }

    pub fn mark_hermes_session_status(&self, id: &str, status: &str) -> Result<()> {
        if id.trim().is_empty() {
            bail!("hermes native session id must not be empty");
        }
        if status.trim().is_empty() {
            bail!("hermes native session status must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE hermes_native_sessions
SET status = ?1,
    updated_at_ms = ?2
WHERE id = ?3
"#,
            rusqlite::params![status, current_time_millis()?, id],
        )?;
        if updated == 0 {
            bail!("Hermes native session does not exist: {id}");
        }
        Ok(())
    }

    pub fn reset_active_hermes_session_by_route(
        &self,
        route: &HermesSessionRoute,
    ) -> Result<usize> {
        route.validate()?;
        let route_key = route.route_key();
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE hermes_native_sessions
SET status = 'reset',
    updated_at_ms = ?1
WHERE route_key = ?2
  AND status = 'active'
"#,
            rusqlite::params![current_time_millis()?, route_key],
        )?;
        Ok(updated)
    }

    pub fn reset_active_hermes_sessions_for_runtime_controller_scope(
        &self,
        agent_did: &str,
        runtime_profile_id: &str,
        controller_scope_key: &str,
    ) -> Result<usize> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE hermes_native_sessions
SET status = 'reset',
    updated_at_ms = ?1
WHERE agent_did = ?2
  AND runtime_profile_id = ?3
  AND controller_scope_key = ?4
  AND status = 'active'
"#,
            rusqlite::params![
                current_time_millis()?,
                agent_did,
                runtime_profile_id,
                controller_scope_key,
            ],
        )?;
        Ok(updated)
    }

    pub fn upsert_runtime_daemon_binding(
        &self,
        runtime_agent_did: &str,
        daemon_agent_did: &str,
        controller_user_id: &str,
        controller_full_handle: &str,
        controller_scope_key: &str,
        controller_did: &str,
    ) -> Result<()> {
        let record = RuntimeDaemonBindingRecord {
            runtime_agent_did: runtime_agent_did.to_string(),
            daemon_agent_did: daemon_agent_did.to_string(),
            controller_user_id: controller_user_id.to_string(),
            controller_full_handle: controller_full_handle.to_string(),
            controller_scope_key: controller_scope_key.to_string(),
            controller_did: controller_did.to_string(),
            created_at_ms: current_time_millis()?,
            updated_at_ms: current_time_millis()?,
        };
        record.validate()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
INSERT INTO runtime_daemon_binding (
    runtime_agent_did,
    daemon_agent_did,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
ON CONFLICT(runtime_agent_did) DO UPDATE SET
    daemon_agent_did = excluded.daemon_agent_did,
    controller_user_id = excluded.controller_user_id,
    controller_full_handle = excluded.controller_full_handle,
    controller_scope_key = excluded.controller_scope_key,
    controller_did = excluded.controller_did,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                record.runtime_agent_did,
                record.daemon_agent_did,
                record.controller_user_id,
                record.controller_full_handle,
                record.controller_scope_key,
                record.controller_did,
                record.created_at_ms,
                record.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn load_runtime_daemon_binding(
        &self,
        runtime_agent_did: &str,
    ) -> Result<Option<RuntimeDaemonBindingRecord>> {
        if runtime_agent_did.trim().is_empty() {
            bail!("runtime_agent_did must not be empty");
        }
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    runtime_agent_did,
    daemon_agent_did,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    created_at_ms,
    updated_at_ms
FROM runtime_daemon_binding
WHERE runtime_agent_did = ?1
"#,
                [runtime_agent_did],
                |row| {
                    Ok(RuntimeDaemonBindingRecord {
                        runtime_agent_did: row.get(0)?,
                        daemon_agent_did: row.get(1)?,
                        controller_user_id: row.get(2)?,
                        controller_full_handle: row.get(3)?,
                        controller_scope_key: row.get(4)?,
                        controller_did: row.get(5)?,
                        created_at_ms: row.get(6)?,
                        updated_at_ms: row.get(7)?,
                    })
                },
            )
            .optional()
            .context("load runtime daemon binding")
    }

    pub fn runtime_agent_belongs_to_daemon_scope(
        &self,
        runtime_agent_did: &str,
        daemon_agent_did: &str,
        controller_scope_key: &str,
    ) -> Result<bool> {
        let Some(binding) = self.load_runtime_daemon_binding(runtime_agent_did)? else {
            return Ok(false);
        };
        Ok(binding.daemon_agent_did == daemon_agent_did
            && binding.controller_scope_key == controller_scope_key)
    }

    pub fn store_runtime_agent_create_request(
        &self,
        record: &RuntimeAgentCreateRequestRecord,
    ) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
INSERT INTO runtime_agent_create_request (
    daemon_agent_did,
    controller_scope_key,
    controller_did,
    client_request_id,
    runtime_agent_did,
    command_id,
    outcome_json,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
ON CONFLICT(daemon_agent_did, controller_scope_key, client_request_id) DO UPDATE SET
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                record.daemon_agent_did,
                record.controller_scope_key,
                record.controller_did,
                record.client_request_id,
                record.runtime_agent_did,
                record.command_id,
                record.outcome_json.to_string(),
                record.created_at_ms,
                record.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn load_runtime_agent_create_request(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        client_request_id: &str,
    ) -> Result<Option<RuntimeAgentCreateRequestRecord>> {
        if daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        if controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        if client_request_id.trim().is_empty() {
            bail!("client_request_id must not be empty");
        }
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    daemon_agent_did,
    controller_scope_key,
    controller_did,
    client_request_id,
    runtime_agent_did,
    command_id,
    outcome_json,
    created_at_ms,
    updated_at_ms
FROM runtime_agent_create_request
WHERE daemon_agent_did = ?1
  AND controller_scope_key = ?2
  AND client_request_id = ?3
"#,
                rusqlite::params![daemon_agent_did, controller_scope_key, client_request_id],
                runtime_agent_create_request_record_from_row,
            )
            .optional()
            .context("load runtime agent create request")
    }

    pub fn try_begin_control_command(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        command_id: &str,
        command: &str,
        message_id: &str,
        target_version: Option<&str>,
    ) -> Result<Option<ControlCommandStateRecord>> {
        for (field_name, value) in [
            ("daemon_agent_did", daemon_agent_did),
            ("controller_scope_key", controller_scope_key),
            ("command_id", command_id),
            ("command", command),
            ("message_id", message_id),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let inserted = connection.execute(
            r#"
INSERT OR IGNORE INTO control_command_state (
    daemon_agent_did,
    controller_scope_key,
    command_id,
    command,
    message_id,
    status,
    target_version,
    result_json,
    error_summary,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, 'in_progress', ?6, '{}', NULL, ?7, ?7)
"#,
            rusqlite::params![
                daemon_agent_did,
                controller_scope_key,
                command_id,
                command,
                message_id,
                target_version,
                now,
            ],
        )?;
        if inserted > 0 {
            Ok(None)
        } else {
            self.load_control_command_state(daemon_agent_did, controller_scope_key, command_id)
        }
    }

    pub fn load_control_command_state(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        command_id: &str,
    ) -> Result<Option<ControlCommandStateRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    daemon_agent_did,
    controller_scope_key,
    command_id,
    command,
    message_id,
    status,
    target_version,
    result_json,
    error_summary,
    created_at_ms,
    updated_at_ms
FROM control_command_state
WHERE daemon_agent_did = ?1
  AND controller_scope_key = ?2
  AND command_id = ?3
"#,
                rusqlite::params![daemon_agent_did, controller_scope_key, command_id],
                control_command_state_record_from_row,
            )
            .optional()
            .context("load control command state")
    }

    pub fn load_latest_control_command_state(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        command: &str,
        statuses: &[&str],
    ) -> Result<Option<ControlCommandStateRecord>> {
        for (field_name, value) in [
            ("daemon_agent_did", daemon_agent_did),
            ("controller_scope_key", controller_scope_key),
            ("command", command),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        let connection = self.connection()?;
        let mut sql = r#"
SELECT
    daemon_agent_did,
    controller_scope_key,
    command_id,
    command,
    message_id,
    status,
    target_version,
    result_json,
    error_summary,
    created_at_ms,
    updated_at_ms
FROM control_command_state
WHERE daemon_agent_did = ?1
  AND controller_scope_key = ?2
  AND command = ?3
"#
        .to_string();
        if !statuses.is_empty() {
            sql.push_str("  AND status IN (");
            sql.push_str(
                &std::iter::repeat("?")
                    .take(statuses.len())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            sql.push_str(")\n");
        }
        sql.push_str("ORDER BY updated_at_ms DESC, created_at_ms DESC LIMIT 1");

        let mut values: Vec<&dyn rusqlite::ToSql> =
            vec![&daemon_agent_did, &controller_scope_key, &command];
        for status in statuses {
            values.push(status);
        }
        connection
            .query_row(
                &sql,
                values.as_slice(),
                control_command_state_record_from_row,
            )
            .optional()
            .context("load latest control command state")
    }

    pub fn mark_control_command_state(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        command_id: &str,
        status: &str,
        result_json: Value,
        error_summary: Option<&str>,
    ) -> Result<()> {
        for (field_name, value) in [
            ("daemon_agent_did", daemon_agent_did),
            ("controller_scope_key", controller_scope_key),
            ("command_id", command_id),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        validate_control_command_status(status)?;
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE control_command_state
SET status = ?1,
    result_json = ?2,
    error_summary = ?3,
    updated_at_ms = ?4
WHERE daemon_agent_did = ?5
  AND controller_scope_key = ?6
  AND command_id = ?7
"#,
            rusqlite::params![
                status,
                result_json.to_string(),
                error_summary,
                current_time_millis()?,
                daemon_agent_did,
                controller_scope_key,
                command_id,
            ],
        )?;
        if updated == 0 {
            bail!("control command state does not exist: {command_id}");
        }
        Ok(())
    }

    pub fn reconcile_daemon_upgrade_commands(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        current_version: &str,
        latest_version: Option<&str>,
        needs_upgrade: bool,
    ) -> Result<()> {
        for (field_name, value) in [
            ("daemon_agent_did", daemon_agent_did),
            ("controller_scope_key", controller_scope_key),
            ("current_version", current_version),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        const UPGRADE_RECONCILE_FAILURE_GRACE_MS: i64 = 2 * 60 * 1000;
        let latest_version = latest_version
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let pending = connection
            .prepare(
                r#"
SELECT
    daemon_agent_did,
    controller_scope_key,
    command_id,
    command,
    message_id,
    status,
    target_version,
    result_json,
    error_summary,
    created_at_ms,
    updated_at_ms
FROM control_command_state
WHERE daemon_agent_did = ?1
  AND controller_scope_key = ?2
  AND command = 'daemon.upgrade'
  AND status IN ('in_progress', 'restart_scheduled')
ORDER BY created_at_ms ASC
"#,
            )
            .context("prepare pending daemon upgrade reconciliation")?
            .query_map(
                rusqlite::params![daemon_agent_did, controller_scope_key],
                control_command_state_record_from_row,
            )
            .context("query pending daemon upgrade reconciliation")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("read pending daemon upgrade reconciliation")?;

        for record in pending {
            let target = record
                .target_version
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let reached_target = match target {
                Some("latest") | None => !needs_upgrade,
                Some(target) => crate::upgrade::version_is_at_least(current_version, target),
            };
            if reached_target {
                self.mark_control_command_state(
                    daemon_agent_did,
                    controller_scope_key,
                    &record.command_id,
                    "succeeded",
                    serde_json::json!({
                        "command": "daemon.upgrade",
                        "daemon_agent_did": daemon_agent_did,
                        "status": "ready",
                        "version": current_version,
                        "latest_version": latest_version,
                        "reconciled": true,
                    }),
                    None,
                )?;
                continue;
            }

            let age_ms = now.saturating_sub(record.updated_at_ms);
            if age_ms < UPGRADE_RECONCILE_FAILURE_GRACE_MS {
                continue;
            }
            self.mark_control_command_state(
                daemon_agent_did,
                controller_scope_key,
                &record.command_id,
                "failed",
                serde_json::json!({
                    "command": "daemon.upgrade",
                    "daemon_agent_did": daemon_agent_did,
                    "status": "failed",
                    "version": current_version,
                    "latest_version": latest_version,
                    "reconciled": true,
                    "error_code": "upgrade_not_applied",
                }),
                Some("daemon upgrade did not reach the requested version"),
            )?;
        }
        Ok(())
    }

    pub fn list_runtime_agent_definitions_for_daemon(
        &self,
        daemon_agent_did: &str,
    ) -> Result<Vec<AgentDefinition>> {
        if daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    agent_definition.agent_did,
    agent_definition.handle,
    agent_definition.agent_kind,
    agent_definition.controller_user_id,
    agent_definition.controller_full_handle,
    agent_definition.controller_scope_key,
    agent_definition.controller_did,
    agent_definition.runtime_plugin_id,
    agent_definition.runtime_profile_id,
    agent_definition.workspace_id,
    agent_definition.policy_id,
    agent_definition.local_agent_db_path,
    agent_definition.message_db_path,
    agent_definition.status
FROM agent_definition
INNER JOIN runtime_daemon_binding
    ON runtime_daemon_binding.runtime_agent_did = agent_definition.agent_did
WHERE agent_definition.agent_kind = 'runtime'
  AND agent_definition.status = 'active'
  AND runtime_daemon_binding.daemon_agent_did = ?1
ORDER BY agent_definition.updated_at DESC, agent_definition.agent_did ASC
"#,
        )?;
        let rows = statement.query_map([daemon_agent_did], agent_definition_from_row)?;
        let mut definitions = Vec::new();
        for row in rows {
            definitions.push(row?);
        }
        Ok(definitions)
    }

    pub fn should_emit_agent_status_query_snapshot(
        &self,
        daemon_agent_did: &str,
        controller_did: &str,
        min_interval_ms: i64,
    ) -> Result<bool> {
        if daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        if controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        if min_interval_ms < 0 {
            bail!("min_interval_ms must not be negative");
        }
        let connection = self.connection()?;
        let last_snapshot_at_ms = connection
            .query_row(
                r#"
SELECT last_snapshot_at_ms
FROM agent_status_query_throttle
WHERE daemon_agent_did = ?1
  AND controller_did = ?2
"#,
                rusqlite::params![daemon_agent_did, controller_did],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let now = current_time_millis()?;
        if last_snapshot_at_ms.is_some_and(|last| now.saturating_sub(last) < min_interval_ms) {
            return Ok(false);
        }
        connection.execute(
            r#"
INSERT INTO agent_status_query_throttle (
    daemon_agent_did,
    controller_did,
    last_snapshot_at_ms
) VALUES (?1, ?2, ?3)
ON CONFLICT(daemon_agent_did, controller_did) DO UPDATE SET
    last_snapshot_at_ms = excluded.last_snapshot_at_ms
"#,
            rusqlite::params![daemon_agent_did, controller_did, now],
        )?;
        Ok(true)
    }

    pub fn count_active_hermes_sessions_for_agent(&self, agent_did: &str) -> Result<usize> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        let connection = self.connection()?;
        let count: i64 = connection.query_row(
            r#"
SELECT COUNT(*)
FROM hermes_native_sessions
WHERE agent_did = ?1
  AND status = 'active'
"#,
            [agent_did],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub fn load_agent_definition(&self, agent_did: &str) -> Result<AgentDefinition> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
WHERE agent_did = ?1
"#,
                [agent_did],
                agent_definition_from_row,
            )
            .with_context(|| format!("load agent definition {agent_did}"))
    }

    pub fn load_daemon_agent_by_handle(&self, handle: &str) -> Result<Option<AgentDefinition>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
WHERE agent_kind = 'daemon' AND handle = ?1
ORDER BY updated_at DESC
LIMIT 1
"#,
        )?;
        let mut rows = statement.query([handle])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(agent_definition_from_row(row)?))
    }

    pub fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>> {
        self.list_agent_definitions_by_kind(None)
    }

    pub fn list_runtime_agent_definitions(&self) -> Result<Vec<AgentDefinition>> {
        self.list_agent_definitions_by_kind(Some(AgentKind::Runtime))
    }

    pub fn load_runtime_agent_profile(&self, agent_did: &str) -> Result<RuntimeAgentProfile> {
        let definition = self.load_agent_definition(agent_did)?;
        if definition.agent_kind != AgentKind::Runtime {
            bail!("agent is not a runtime agent");
        }
        let runtime_profile_id = definition
            .runtime_profile_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .context("runtime agent is missing runtime_profile_id")?;
        let connection = self.connection()?;
        let mut profile = connection
            .query_row(
                r#"
SELECT
    runtime_profile_id,
    agent_did,
    runtime_plugin_id,
    display_name
FROM runtime_profile
WHERE runtime_profile_id = ?1
"#,
                [runtime_profile_id],
                |row| {
                    Ok(RuntimeAgentProfile {
                        runtime_profile_id: row.get(0)?,
                        agent_did: row.get(1)?,
                        agent_handle: definition.handle.clone(),
                        runtime_plugin_id: row.get(2)?,
                        display_name: row.get(3)?,
                        controller_user_id: definition.controller_user_id.clone(),
                        controller_full_handle: definition.controller_full_handle.clone(),
                        controller_scope_key: definition.controller_scope_key.clone(),
                        controller_did: definition.controller_did.clone(),
                        workspace_id: definition.workspace_id.clone(),
                        workspace_root: None,
                        workspace_mode: None,
                    })
                },
            )
            .context("load runtime profile")?;
        if let Some(workspace_id) = definition.workspace_id.as_deref() {
            let binding: (String, WorkspaceMode) = connection.query_row(
                r#"
SELECT workspace_root, workspace_mode
FROM workspace_binding
WHERE workspace_id = ?1
"#,
                [workspace_id],
                |row| {
                    let root: String = row.get(0)?;
                    let mode: String = row.get(1)?;
                    let mode = WorkspaceMode::parse(&mode).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            mode.len(),
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                err.to_string(),
                            )),
                        )
                    })?;
                    Ok((root, mode))
                },
            )?;
            profile.workspace_root = Some(PathBuf::from(binding.0));
            profile.workspace_mode = Some(binding.1);
        }
        profile.validate()?;
        Ok(profile)
    }

    fn list_agent_definitions_by_kind(
        &self,
        kind: Option<AgentKind>,
    ) -> Result<Vec<AgentDefinition>> {
        let connection = self.connection()?;
        let sql = match kind {
            Some(_) => {
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
WHERE agent_kind = ?1
  AND status = 'active'
ORDER BY updated_at DESC, agent_did ASC
"#
            }
            None => {
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
WHERE status = 'active'
ORDER BY updated_at DESC, agent_did ASC
"#
            }
        };
        let mut statement = connection.prepare(sql)?;
        let rows = match kind {
            Some(kind) => statement.query_map([kind.as_str()], agent_definition_from_row)?,
            None => statement.query_map([], agent_definition_from_row)?,
        };
        let mut definitions = Vec::new();
        for row in rows {
            definitions.push(row?);
        }
        Ok(definitions)
    }

    pub fn upsert_workspace_binding(
        &self,
        agent_did: &str,
        runtime_profile_id: &str,
        binding: &WorkspaceBindingConfig,
    ) -> Result<()> {
        binding.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
            r#"
INSERT INTO workspace_binding (
    workspace_id,
    agent_did,
    runtime_profile_id,
    workspace_root,
    workspace_mode,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6)
ON CONFLICT(workspace_id) DO UPDATE SET
    agent_did = excluded.agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    workspace_root = excluded.workspace_root,
    workspace_mode = excluded.workspace_mode,
    status = 'active',
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                binding.workspace_id,
                agent_did,
                runtime_profile_id,
                binding.workspace_root.display().to_string(),
                binding.workspace_mode.as_str(),
                now,
            ],
        )?;
        Ok(())
    }
}
