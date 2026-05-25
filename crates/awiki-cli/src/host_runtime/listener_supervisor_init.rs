#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupervisorInitInputs {
    pub runtime_mode: String,
    pub installed: bool,
    pub boot_id: String,
    pub pid_file: String,
    pub log_file: String,
    pub status_file: String,
    pub socket_path: String,
    pub pid: i64,
    pub started_at: String,
    pub host_notify_status: HostNotifyInitStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostNotifyInitStatus {
    pub enabled: bool,
    pub sink: String,
    pub file_path: String,
    pub hook_url: String,
    pub agent_id: String,
    pub hook_name: String,
    pub notify_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorInitialStatus {
    pub mode: String,
    pub installed: bool,
    pub running: bool,
    pub boot_id: String,
    pub pid_file: String,
    pub log_file: String,
    pub status_file: String,
    pub socket_path: String,
    pub pid: i64,
    pub started_at: String,
    pub host_notify: HostNotifyInitStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorInitAction {
    OpenStore,
    EnsureStoreSchema,
    CloseStore,
    ResolveRuntimeBootID,
    ResolveRuntimePaths,
    NewHostNotifySink,
    CloseHostNotifySink,
    NewRemoteClient,
    NewIdentityManager,
    InitSessionsMap,
    InitLocalNotificationsMap,
    BuildSupervisorStatus(SupervisorInitialStatus),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorInitDecision {
    ReturnSupervisor,
    ReturnError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorInitPlan {
    pub actions: Vec<SupervisorInitAction>,
    pub decision: SupervisorInitDecision,
}

pub fn supervisor_init_plan(
    inputs: SupervisorInitInputs,
    open_store_error: Option<&str>,
    ensure_schema_error: Option<&str>,
    boot_id_error: Option<&str>,
    paths_error: Option<&str>,
    host_notify_error: Option<&str>,
    remote_client_error: Option<&str>,
) -> SupervisorInitPlan {
    let mut actions = vec![SupervisorInitAction::OpenStore];
    if let Some(error) = open_store_error {
        return return_error(actions, error);
    }

    actions.push(SupervisorInitAction::EnsureStoreSchema);
    if let Some(error) = ensure_schema_error {
        actions.push(SupervisorInitAction::CloseStore);
        return return_error(actions, error);
    }

    actions.push(SupervisorInitAction::ResolveRuntimeBootID);
    if let Some(error) = boot_id_error {
        actions.push(SupervisorInitAction::CloseStore);
        return return_error(actions, error);
    }

    actions.push(SupervisorInitAction::ResolveRuntimePaths);
    if let Some(error) = paths_error {
        actions.push(SupervisorInitAction::CloseStore);
        return return_error(actions, error);
    }

    actions.push(SupervisorInitAction::NewHostNotifySink);
    if let Some(error) = host_notify_error {
        actions.push(SupervisorInitAction::CloseStore);
        return return_error(actions, error);
    }

    actions.push(SupervisorInitAction::NewRemoteClient);
    if let Some(error) = remote_client_error {
        actions.push(SupervisorInitAction::CloseStore);
        actions.push(SupervisorInitAction::CloseHostNotifySink);
        return return_error(actions, error);
    }

    actions.push(SupervisorInitAction::NewIdentityManager);
    actions.push(SupervisorInitAction::InitSessionsMap);
    actions.push(SupervisorInitAction::InitLocalNotificationsMap);
    actions.push(SupervisorInitAction::BuildSupervisorStatus(
        SupervisorInitialStatus {
            mode: inputs.runtime_mode,
            installed: inputs.installed,
            running: true,
            boot_id: inputs.boot_id,
            pid_file: inputs.pid_file,
            log_file: inputs.log_file,
            status_file: inputs.status_file,
            socket_path: inputs.socket_path,
            pid: inputs.pid,
            started_at: inputs.started_at,
            host_notify: inputs.host_notify_status,
        },
    ));

    SupervisorInitPlan {
        actions,
        decision: SupervisorInitDecision::ReturnSupervisor,
    }
}

fn return_error(actions: Vec<SupervisorInitAction>, error: &str) -> SupervisorInitPlan {
    SupervisorInitPlan {
        actions,
        decision: SupervisorInitDecision::ReturnError(error.to_string()),
    }
}
