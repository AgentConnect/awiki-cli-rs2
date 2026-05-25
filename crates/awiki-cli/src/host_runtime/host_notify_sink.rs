use crate::host_runtime::hermes_host_notify::{new_hermes_host_notify_sink, HermesHostNotifySink};
use crate::host_runtime::host_notify::HostNotificationEvent;
use crate::host_runtime::listener::HostNotifyStatus;
use crate::host_runtime::openclaw_host_notify::{
    new_openclaw_host_notify_sink, OpenClawHostNotifySink,
};
use crate::workspace_config::Resolved;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

pub trait HostNotifySink {
    fn notify(&self, event: &HostNotificationEvent) -> anyhow::Result<()>;
    fn close(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Default)]
pub struct NoopHostNotifySink;

impl HostNotifySink for NoopHostNotifySink {
    fn notify(&self, _event: &HostNotificationEvent) -> anyhow::Result<()> {
        Ok(())
    }

    fn close(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct LogHostNotifySink;

impl HostNotifySink for LogHostNotifySink {
    fn notify(&self, event: &HostNotificationEvent) -> anyhow::Result<()> {
        let raw = serde_json::to_string(event)?;
        eprintln!("host notification {raw}");
        Ok(())
    }

    fn close(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct FileHostNotifySink {
    file: Mutex<Option<File>>,
}

pub fn new_file_host_notify_sink(path: &str) -> anyhow::Result<FileHostNotifySink> {
    if path.trim().is_empty() {
        anyhow::bail!("host notify file sink requires a file path");
    }
    let path = Path::new(path);
    let dir = host_notify_sink_dir(path);
    create_host_notify_sink_dir(dir)?;
    let file = open_host_notify_sink_file(path)?;
    Ok(FileHostNotifySink {
        file: Mutex::new(Some(file)),
    })
}

impl HostNotifySink for FileHostNotifySink {
    fn notify(&self, event: &HostNotificationEvent) -> anyhow::Result<()> {
        let mut raw = serde_json::to_vec(event)?;
        raw.push(b'\n');
        let mut guard = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("write host notify event: lock poisoned"))?;
        let Some(file) = guard.as_mut() else {
            anyhow::bail!("write host notify event: file is closed");
        };
        file.write_all(&raw)
            .map_err(|err| anyhow::anyhow!("write host notify event: {err}"))?;
        file.sync_all()
            .map_err(|err| anyhow::anyhow!("write host notify event: {err}"))
    }

    fn close(&self) -> anyhow::Result<()> {
        let mut guard = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("close host notify sink file: lock poisoned"))?;
        let Some(file) = guard.take() else {
            return Ok(());
        };
        drop(file);
        Ok(())
    }
}

#[derive(Debug)]
pub enum HostNotifySinkImpl {
    Noop(NoopHostNotifySink),
    Log(LogHostNotifySink),
    File(FileHostNotifySink),
    OpenClaw(OpenClawHostNotifySink),
    Hermes(HermesHostNotifySink),
}

impl HostNotifySink for HostNotifySinkImpl {
    fn notify(&self, event: &HostNotificationEvent) -> anyhow::Result<()> {
        match self {
            HostNotifySinkImpl::Noop(sink) => sink.notify(event),
            HostNotifySinkImpl::Log(sink) => sink.notify(event),
            HostNotifySinkImpl::File(sink) => sink.notify(event),
            HostNotifySinkImpl::OpenClaw(sink) => sink.notify(event),
            HostNotifySinkImpl::Hermes(sink) => sink.notify(event),
        }
    }

    fn close(&self) -> anyhow::Result<()> {
        match self {
            HostNotifySinkImpl::Noop(sink) => sink.close(),
            HostNotifySinkImpl::Log(sink) => sink.close(),
            HostNotifySinkImpl::File(sink) => sink.close(),
            HostNotifySinkImpl::OpenClaw(sink) => sink.close(),
            HostNotifySinkImpl::Hermes(sink) => sink.close(),
        }
    }
}

pub fn new_host_notify_sink(
    resolved: &Resolved,
) -> anyhow::Result<(HostNotifySinkImpl, HostNotifyStatus)> {
    let config = super::resolve(resolved).host_notify;
    let status = HostNotifyStatus {
        enabled: config.enabled,
        sink: config.sink.clone(),
        file_path: config.file_path.clone(),
        hook_url: config
            .openclaw
            .as_ref()
            .map(|config| config.hook_url.clone())
            .unwrap_or_default(),
        agent_id: config
            .openclaw
            .as_ref()
            .map(|config| config.agent_id.clone())
            .unwrap_or_default(),
        hook_name: config
            .openclaw
            .as_ref()
            .map(|config| config.hook_name.clone())
            .unwrap_or_default(),
        notify_url: config
            .hermes
            .as_ref()
            .map(|config| config.notify_url.clone())
            .unwrap_or_default(),
        last_error: String::new(),
    };
    if !config.enabled {
        return Ok((HostNotifySinkImpl::Noop(NoopHostNotifySink), status));
    }
    match config.sink.as_str() {
        "noop" => Ok((HostNotifySinkImpl::Noop(NoopHostNotifySink), status)),
        "log" => Ok((HostNotifySinkImpl::Log(LogHostNotifySink), status)),
        "file" => {
            let sink = new_file_host_notify_sink(&config.file_path)?;
            Ok((HostNotifySinkImpl::File(sink), status))
        }
        "hermes" => {
            let Some(hermes_config) = config.hermes.as_ref() else {
                anyhow::bail!("hermes host notify requires runtime.host_notify.hermes.notify_url");
            };
            let sink = new_hermes_host_notify_sink(resolved, hermes_config)?;
            Ok((HostNotifySinkImpl::Hermes(sink), status))
        }
        "openclaw" => {
            let sink = new_openclaw_host_notify_sink(resolved)?;
            let runtime_settings = super::effective_openclaw_settings(resolved);
            Ok((
                HostNotifySinkImpl::OpenClaw(sink),
                HostNotifyStatus {
                    hook_url: runtime_settings.hook_url,
                    ..status
                },
            ))
        }
        sink => anyhow::bail!("unsupported host notify sink {sink:?}"),
    }
}

fn create_host_notify_sink_dir(path: &Path) -> anyhow::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    set_dir_builder_mode(&mut builder, 0o700);
    builder
        .create(path)
        .map_err(|err| anyhow::anyhow!("create host notify sink dir: {err}"))
}

fn host_notify_sink_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn open_host_notify_sink_file(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    set_file_open_mode(&mut options, 0o600);
    options
        .open(path)
        .map_err(|err| anyhow::anyhow!("open host notify sink file: {err}"))
}

#[cfg(unix)]
fn set_dir_builder_mode(builder: &mut DirBuilder, mode: u32) {
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(mode);
}

#[cfg(not(unix))]
fn set_dir_builder_mode(_builder: &mut DirBuilder, _mode: u32) {}

#[cfg(unix)]
fn set_file_open_mode(options: &mut OpenOptions, mode: u32) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(mode);
}

#[cfg(not(unix))]
fn set_file_open_mode(_options: &mut OpenOptions, _mode: u32) {}
