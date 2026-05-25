use crate::durable_fs;
use crate::workspace_config::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const ROUTES_FILE_NAME: &str = "openclaw.host-notify.routes.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub channel: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub routes: Vec<Route>,
}

pub fn routes_path(paths: &Paths) -> String {
    let state_dir = paths.state_dir.trim();
    let base = if state_dir.is_empty() {
        Path::new(&paths.workspace_home_dir).join("runtime")
    } else {
        PathBuf::from(state_dir)
    };
    base.join(ROUTES_FILE_NAME).to_string_lossy().into_owned()
}

pub fn resolve_route_input(channel: &str, to: &str, session_key: &str) -> anyhow::Result<Route> {
    let channel = channel.trim();
    let to = to.trim();
    let session_key = session_key.trim();
    match (
        !session_key.is_empty(),
        !channel.is_empty() || !to.is_empty(),
    ) {
        (true, true) => anyhow::bail!("provide either --session-key or --channel/--to, not both"),
        (true, false) => parse_session_key(session_key),
        (false, _) if channel.is_empty() || to.is_empty() => {
            anyhow::bail!("route requires either --session-key or both --channel and --to")
        }
        (false, _) => normalize_route(Route {
            channel: channel.to_string(),
            to: to.to_string(),
        }),
    }
}

pub fn parse_session_key(session_key: &str) -> anyhow::Result<Route> {
    let session_key = session_key.trim();
    if session_key.is_empty() {
        anyhow::bail!("session key is required");
    }
    let parts: Vec<&str> = session_key.split(':').collect();
    if parts.len() < 5 {
        anyhow::bail!("unsupported session key format {session_key:?}");
    }
    if parts[0] != "agent" {
        anyhow::bail!("unsupported session key prefix {session_key:?}");
    }
    if !matches!(parts[3], "direct" | "group" | "channel" | "room") {
        anyhow::bail!("unsupported session key route type {:?}", parts[3]);
    }
    normalize_route(Route {
        channel: parts[2].to_string(),
        to: parts[4..].join(":"),
    })
}

pub fn normalize_route(route: Route) -> anyhow::Result<Route> {
    let normalized = Route {
        channel: route.channel.trim().to_ascii_lowercase(),
        to: route.to.trim().to_string(),
    };
    if normalized.channel.is_empty() {
        anyhow::bail!("route channel is required");
    }
    if normalized.to.is_empty() {
        anyhow::bail!("route target is required");
    }
    Ok(normalized)
}

pub fn load_routes(paths: &Paths) -> anyhow::Result<Vec<Route>> {
    let registry_path = routes_path(paths);
    let raw = match fs::read(&registry_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(anyhow::anyhow!("read route registry: {err}")),
    };
    let registry: Registry = serde_json::from_slice(&raw)
        .map_err(|err| anyhow::anyhow!("parse route registry: {err}"))?;
    normalize_routes(registry.routes)
        .map_err(|err| anyhow::anyhow!("normalize route registry entry: {err}"))
}

pub fn add_route(paths: &Paths, route: Route) -> anyhow::Result<(Route, bool, Vec<Route>)> {
    let normalized = normalize_route(route)?;
    let mut routes = load_routes(paths)?;
    if routes
        .iter()
        .any(|existing| route_key(existing) == route_key(&normalized))
    {
        return Ok((normalized, false, routes));
    }
    routes.push(normalized.clone());
    write_routes(paths, &routes)?;
    Ok((normalized, true, routes))
}

pub fn remove_route(paths: &Paths, route: Route) -> anyhow::Result<(Route, bool, Vec<Route>)> {
    let normalized = normalize_route(route)?;
    let routes = load_routes(paths)?;
    let mut removed = false;
    let mut filtered = Vec::with_capacity(routes.len());
    for existing in &routes {
        if route_key(&existing) == route_key(&normalized) {
            removed = true;
            continue;
        }
        filtered.push(existing.clone());
    }
    if !removed {
        return Ok((normalized, false, routes));
    }
    write_routes(paths, &filtered)?;
    Ok((normalized, true, filtered))
}

pub fn write_routes(paths: &Paths, routes: &[Route]) -> anyhow::Result<()> {
    let registry_path = routes_path(paths);
    let parent = Path::new(&registry_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    create_route_registry_dir(parent)?;
    let normalized = normalize_routes(routes.to_vec())?;
    let mut raw = serde_json::to_vec_pretty(&Registry { routes: normalized })
        .map_err(|err| anyhow::anyhow!("marshal route registry: {err}"))?;
    raw.push(b'\n');
    write_atomic_file(Path::new(&registry_path), &raw, 0o600)
        .map_err(|err| anyhow::anyhow!("write route registry: {err}"))
}

fn normalize_routes(routes: Vec<Route>) -> anyhow::Result<Vec<Route>> {
    let mut normalized = Vec::with_capacity(routes.len());
    let mut seen = BTreeSet::new();
    for route in routes {
        let item = normalize_route(route)?;
        let key = route_key(&item);
        if seen.insert(key) {
            normalized.push(item);
        }
    }
    Ok(normalized)
}

fn route_key(route: &Route) -> String {
    format!("{}\0{}", route.channel, route.to)
}

fn write_atomic_file(path: &Path, content: &[u8], mode: u32) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let (mut temp_file, temp_path) = create_temp_route_file(parent)?;
    let mut cleanup = TempCleanup::new(temp_path.clone());

    temp_file
        .write_all(content)
        .map_err(|err| anyhow::anyhow!("write temp route registry: {err}"))?;
    temp_file
        .sync_all()
        .map_err(|err| anyhow::anyhow!("sync temp route registry: {err}"))?;
    drop(temp_file);
    set_file_mode(&temp_path, mode)?;
    fs::rename(&temp_path, path).map_err(|err| anyhow::anyhow!("replace route registry: {err}"))?;
    cleanup.keep();
    durable_fs::sync_directory(parent)
        .map_err(|err| anyhow::anyhow!("sync route registry dir: {err}"))?;
    Ok(())
}

fn create_route_registry_dir(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)
            .map_err(|err| anyhow::anyhow!("create route registry dir: {err}"))?;
        set_dir_mode(path, 0o700)?;
    }
    Ok(())
}

fn create_temp_route_file(parent: &Path) -> anyhow::Result<(File, PathBuf)> {
    for attempt in 0..100 {
        let path = parent.join(temp_route_name(attempt));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(anyhow::anyhow!("create temp route registry: {err}")),
        }
    }
    anyhow::bail!("create temp route registry: too many temporary name collisions")
}

fn temp_route_name(attempt: u32) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(".routes-{}-{}-{attempt}.tmp", std::process::id(), nonce)
}

#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| anyhow::anyhow!("chmod route registry dir: {err}"))
}

#[cfg(not(unix))]
fn set_dir_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| anyhow::anyhow!("chmod temp route registry: {err}"))
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

struct TempCleanup {
    path: PathBuf,
    cleanup: bool,
}

impl TempCleanup {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            cleanup: true,
        }
    }

    fn keep(&mut self) {
        self.cleanup = false;
    }
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolve_route_input_supports_channel_and_to() {
        let route = resolve_route_input(" FeiShu ", " ou_123 ", "").expect("route");
        assert_eq!(route.channel, "feishu");
        assert_eq!(route.to, "ou_123");
    }

    #[test]
    fn resolve_route_input_supports_session_key() {
        let route =
            resolve_route_input("", "", "agent:main:telegram:direct:123456").expect("route");
        assert_eq!(route.channel, "telegram");
        assert_eq!(route.to, "123456");
    }

    #[test]
    fn add_and_remove_route_persist_registry() {
        let paths = test_paths();
        let (route, added, routes) = add_route(
            &paths,
            Route {
                channel: "feishu".to_string(),
                to: "ou_123".to_string(),
            },
        )
        .expect("add route");
        assert!(added);
        assert_eq!(route.channel, "feishu");
        assert_eq!(route.to, "ou_123");
        assert_eq!(routes.len(), 1);

        let (_, added, routes) = add_route(
            &paths,
            Route {
                channel: "Feishu".to_string(),
                to: "ou_123".to_string(),
            },
        )
        .expect("duplicate add route");
        assert!(!added);
        assert_eq!(routes.len(), 1);

        let loaded = load_routes(&paths).expect("load routes");
        assert_eq!(loaded.len(), 1);

        let (_, removed, routes) = remove_route(
            &paths,
            Route {
                channel: "feishu".to_string(),
                to: "ou_123".to_string(),
            },
        )
        .expect("remove route");
        assert!(removed);
        assert!(routes.is_empty());
    }

    #[test]
    fn load_routes_missing_file_returns_empty() {
        let paths = test_paths();
        let routes = load_routes(&paths).expect("load routes");
        assert!(routes.is_empty());
        assert!(!Path::new(&routes_path(&paths)).exists());
    }

    fn test_paths() -> Paths {
        let root = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-openclaw-routes-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = fs::create_dir_all(&root);
        Paths {
            workspace_home_dir: path_string(&root),
            root_dir: path_string(&root),
            config_dir: path_string(&root),
            data_dir: path_string(&root.join("data")),
            state_dir: path_string(&root.join("runtime")),
            cache_dir: path_string(&root.join("cache")),
            logs_dir: path_string(&root.join("logs")),
            config_file: path_string(&root.join("config.yaml")),
            identity_dir: path_string(&root.join("identities")),
            database_file: path_string(&root.join("data").join("awiki.db")),
            legacy_credentials_dir: path_string(&root.join("credentials")),
            legacy_data_dir: path_string(&root.join("legacy-data")),
        }
    }

    fn path_string(path: &PathBuf) -> String {
        path.to_string_lossy().into_owned()
    }
}
