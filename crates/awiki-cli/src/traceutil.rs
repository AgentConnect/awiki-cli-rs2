use std::cell::RefCell;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const TIMING_ENV_KEY: &str = "AWIKI_CLI_TRACE_TIMING";

thread_local! {
    static CURRENT_RUN: RefCell<Option<Run>> = RefCell::new(None);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    pub name: String,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackEvent {
    pub stage: String,
    pub cause: String,
}

#[derive(Debug, Default)]
struct RunState {
    phases: Vec<Phase>,
    fallbacks: Vec<FallbackEvent>,
}

#[derive(Debug, Clone)]
pub struct Run {
    command: String,
    started: Instant,
    enabled: bool,
    state: Arc<Mutex<RunState>>,
}

#[derive(Debug)]
pub struct PhaseFinish {
    state: Option<Arc<Mutex<RunState>>>,
    index: usize,
    started: Instant,
}

pub fn enabled() -> bool {
    matches!(
        std::env::var(TIMING_ENV_KEY)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

pub fn set_current(run: Option<Run>) {
    CURRENT_RUN.with(|current| {
        *current.borrow_mut() = run;
    });
}

pub fn current() -> Option<Run> {
    CURRENT_RUN.with(|current| current.borrow().clone())
}

pub fn take_current() -> Option<Run> {
    CURRENT_RUN.with(|current| current.borrow_mut().take())
}

pub fn emit_current<W: Write>(writer: &mut W) -> io::Result<()> {
    let Some(run) = current() else {
        return Ok(());
    };
    run.emit(writer)
}

pub fn start_phase(name: &str) -> PhaseFinish {
    current()
        .map(|run| run.start_phase(name))
        .unwrap_or_else(PhaseFinish::inactive)
}

pub fn rpc_phase(operation: &str) -> PhaseFinish {
    current()
        .map(|run| run.rpc_phase(operation))
        .unwrap_or_else(PhaseFinish::inactive)
}

pub fn local_db_phase(operation: &str) -> PhaseFinish {
    current()
        .map(|run| run.local_db_phase(operation))
        .unwrap_or_else(PhaseFinish::inactive)
}

pub fn ensure_jwt_phase(operation: &str) -> PhaseFinish {
    current()
        .map(|run| run.ensure_jwt_phase(operation))
        .unwrap_or_else(PhaseFinish::inactive)
}

pub fn handle_lookup_phase(operation: &str) -> PhaseFinish {
    current()
        .map(|run| run.handle_lookup_phase(operation))
        .unwrap_or_else(PhaseFinish::inactive)
}

pub fn mark_fallback(stage: &str, cause: Option<&str>) {
    if let Some(run) = current() {
        run.mark_fallback(stage, cause);
    }
}

impl Run {
    pub fn new(command: &str) -> Self {
        Self {
            command: command.trim().to_string(),
            started: Instant::now(),
            enabled: enabled(),
            state: Arc::new(Mutex::new(RunState::default())),
        }
    }

    pub fn start_phase(&self, name: &str) -> PhaseFinish {
        if !self.enabled {
            return PhaseFinish {
                state: None,
                index: 0,
                started: Instant::now(),
            };
        }
        let mut state = self.state.lock().expect("trace state lock");
        let index = state.phases.len();
        state.phases.push(Phase {
            name: name.trim().to_string(),
            duration_ms: 0,
        });
        PhaseFinish {
            state: Some(Arc::clone(&self.state)),
            index,
            started: Instant::now(),
        }
    }

    pub fn rpc_phase(&self, operation: &str) -> PhaseFinish {
        self.start_phase(&phase_name("business_rpc", operation))
    }

    pub fn local_db_phase(&self, operation: &str) -> PhaseFinish {
        self.start_phase(&phase_name("local_db", operation))
    }

    pub fn ensure_jwt_phase(&self, operation: &str) -> PhaseFinish {
        self.start_phase(&phase_name("ensure_jwt", operation))
    }

    pub fn handle_lookup_phase(&self, operation: &str) -> PhaseFinish {
        self.start_phase(&phase_name("handle_lookup", operation))
    }

    pub fn mark_fallback(&self, stage: &str, cause: Option<&str>) {
        if !self.enabled {
            return;
        }
        let mut state = self.state.lock().expect("trace state lock");
        state.fallbacks.push(FallbackEvent {
            stage: stage.trim().to_string(),
            cause: cause.unwrap_or_default().to_string(),
        });
    }

    pub fn emit<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let state = self.state.lock().expect("trace state lock");
        emit_pretty(
            writer,
            &self.command,
            &state.phases,
            &state.fallbacks,
            self.started.elapsed().as_millis() as i64,
        )
    }

    pub fn emit_to_string(&self) -> io::Result<String> {
        let mut buffer = Vec::new();
        self.emit(&mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }

    pub fn phases(&self) -> Vec<Phase> {
        self.state.lock().expect("trace state lock").phases.clone()
    }

    pub fn fallbacks(&self) -> Vec<FallbackEvent> {
        self.state
            .lock()
            .expect("trace state lock")
            .fallbacks
            .clone()
    }
}

impl PhaseFinish {
    pub fn inactive() -> Self {
        Self {
            state: None,
            index: 0,
            started: Instant::now(),
        }
    }

    pub fn finish(&mut self) {
        let Some(state) = &self.state else {
            return;
        };
        let mut state = state.lock().expect("trace state lock");
        if let Some(phase) = state.phases.get_mut(self.index) {
            phase.duration_ms = self.started.elapsed().as_millis() as i64;
        }
    }
}

pub fn phase_name(name: &str, detail: &str) -> String {
    let name = name.trim();
    let detail = sanitize_phase_detail(detail);
    match (name.is_empty(), detail.is_empty()) {
        (true, _) => detail,
        (_, true) => name.to_string(),
        (false, false) => format!("{name}:{detail}"),
    }
}

pub fn sanitize_phase_detail(detail: &str) -> String {
    let mut value = detail.trim().trim_matches('/').replace('/', ".");
    value = value.split_whitespace().collect::<Vec<_>>().join("_");
    value = value.replace("_.", "_").replace("._", "_");
    value.trim_matches(['.', '_']).to_string()
}

fn emit_pretty<W: Write>(
    writer: &mut W,
    command: &str,
    phases: &[Phase],
    fallbacks: &[FallbackEvent],
    total_ms: i64,
) -> io::Result<()> {
    writeln!(writer, "[awiki-cli 耗时追踪]")?;
    if !command.is_empty() {
        writeln!(writer, "命令: {command}")?;
    }
    writeln!(writer, "总耗时: {}", format_duration_ms(total_ms))?;
    if !phases.is_empty() {
        writeln!(writer, "阶段:")?;
        for (index, phase) in phases.iter().enumerate() {
            writeln!(
                writer,
                "  {:2}. {}: {}",
                index + 1,
                humanize_phase_name(&phase.name),
                format_duration_ms(phase.duration_ms)
            )?;
        }
        writeln!(
            writer,
            "说明: 各阶段按开始顺序展示，阶段之间可能重叠，因此不会等于总耗时相加。"
        )?;
    }
    if !fallbacks.is_empty() {
        writeln!(writer, "回退:")?;
        for fallback in fallbacks {
            let mut line = humanize_text(&fallback.stage);
            if !fallback.cause.trim().is_empty() {
                line.push_str(" (");
                line.push_str(&fallback.cause);
                line.push(')');
            }
            writeln!(writer, "  - {line}")?;
        }
    }
    Ok(())
}

pub fn humanize_phase_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return "未命名阶段".to_string();
    }
    let mut parts = name.splitn(2, ':');
    let group = phase_group_label(parts.next().unwrap_or_default());
    let detail = parts.next().unwrap_or_default().trim();
    if detail.is_empty() {
        group
    } else {
        format!("{group} / {}", humanize_text(detail))
    }
}

pub fn phase_group_label(group: &str) -> String {
    match group.trim() {
        "business_rpc" => "远端 RPC".to_string(),
        "local_db" => "本地数据库".to_string(),
        "ensure_jwt" => "JWT 续期".to_string(),
        "handle_lookup" => "Handle 解析".to_string(),
        "resolve_config" => "解析配置".to_string(),
        "update_check" => "检查更新".to_string(),
        "update_registry_fetch" => "请求更新源".to_string(),
        "npm_upgrade_install" => "npm 升级安装".to_string(),
        "workspace_upgrade" => "工作区升级".to_string(),
        "bridge_health_probe" => "本地桥健康检查".to_string(),
        "bridge_call" => "调用本地桥".to_string(),
        "contact_sync" => "联系人同步".to_string(),
        other => humanize_text(other),
    }
}

pub fn humanize_text(value: &str) -> String {
    let value = value.trim();
    if let Some(translated) = known_humanized_text(value) {
        return translated.to_string();
    }
    let value = value.replace('.', " ").replace('_', " ");
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        "空".to_string()
    } else {
        value
    }
}

pub fn format_duration_ms(ms: i64) -> String {
    if ms < 1000 {
        format!("{ms} 毫秒")
    } else {
        format!("{:.3} 秒", ms as f64 / 1000.0)
    }
}

fn known_humanized_text(value: &str) -> Option<&'static str> {
    match value {
        "websocket_to_http" => Some("WebSocket 降级到 HTTP"),
        "handle_to_did" => Some("Handle 转 DID"),
        "did_to_handle" => Some("DID 转 Handle"),
        "target_resolve" => Some("目标解析"),
        "lookup" => Some("查询"),
        "resolve" => Some("解析"),
        "get_public_profile" => Some("获取公开资料"),
        "contact_sync_by_did" => Some("按 DID 同步联系人"),
        "handle_cache_lookup_by_handle" => Some("按 Handle 查询缓存"),
        "handle_cache_lookup_by_did" => Some("按 DID 查询缓存"),
        "handle_cache_write" => Some("写入 Handle 缓存"),
        "persist_direct_send" => Some("写入直聊发送结果"),
        "persist_inbox_messages" => Some("写入收件箱消息"),
        "persist_history_messages" => Some("写入历史消息"),
        "read_inbox_cache" => Some("读取收件箱缓存"),
        "read_unified_direct_inbox_cache" => Some("读取统一直聊收件箱缓存"),
        "read_mail_notification_cache" => Some("读取邮件通知缓存"),
        "read_history_cache" => Some("读取历史缓存"),
        "read_inbox_cache_by_peer_dids" => Some("按对端 DID 读取收件箱缓存"),
        "read_history_cache_by_peer_dids" => Some("按对端 DID 读取历史缓存"),
        "persist_group_send" => Some("写入群组发送结果"),
        "persist_group_snapshot" => Some("写入群组快照"),
        "persist_group_members" => Some("写入群组成员"),
        "persist_group_messages" => Some("写入群组消息"),
        "touch_group_cache" => Some("更新群组缓存"),
        "mark_group_left" => Some("标记群组离开"),
        "read_group_snapshot_cache" => Some("读取群组快照缓存"),
        "read_group_members_cache" => Some("读取群组成员缓存"),
        "read_group_messages_cache" => Some("读取群组消息缓存"),
        "read_group_inbox_cache" => Some("读取群组收件箱缓存"),
        "read_all_group_inbox_cache" => Some("读取全部群组收件箱缓存"),
        "message_fallback_refresh" => Some("消息回退时刷新 JWT"),
        "message_service_retry" => Some("消息服务重试前刷新 JWT"),
        "identity_refresh_token" => Some("身份刷新 Token"),
        "identity_bootstrap" => Some("身份服务启动鉴权"),
        "mail_bootstrap" => Some("邮件服务启动鉴权"),
        "content_bootstrap" => Some("内容服务启动鉴权"),
        "site_bootstrap" => Some("站点服务启动鉴权"),
        "message_bootstrap" => Some("消息服务启动鉴权"),
        _ => None,
    }
}
