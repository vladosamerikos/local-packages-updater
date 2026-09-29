pub mod checkers;
pub mod operations;
use checkers::{Checkers, CheckResult};
use operations::{Coordinator, PendingCheck};
use nix::{sys::signal::{kill, Signal}, unistd::Pid};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs::{self, OpenOptions}, io::Write, path::{Path, PathBuf}, process::Stdio, sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex as StdMutex}, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::{process::Command, time::{sleep, timeout}, sync::broadcast};
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const UPDATE_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const STALE_AFTER: u64 = 12 * 60 * 60;
const PERIODIC_CHECK: Duration = Duration::from_secs(12 * 60 * 60);
const INITIAL_CHECK_DELAY: Duration = Duration::from_secs(90);


#[derive(Clone, Copy)]
struct AppSpec {
    id: &'static str,
    name: &'static str,
    update_script: &'static str,
}

const APPS: [AppSpec; 2] = [
    AppSpec {
        id: "codex-desktop",
        name: "Codex Desktop",
        update_script: "apps/codex-desktop/update.sh",
    },
    AppSpec {
        id: "spotifast",
        name: "Spotifast",
        update_script: "apps/spotifast/update.sh",
    },
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    id: String,
    name: String,
    installed: Option<String>,
    latest: Option<String>,
    pub update_available: bool,
    pub state: String,
    error: Option<String>,
    notice: Option<String>,
    checked_at: Option<u64>,
}

impl AppStatus {
    fn new(spec: &AppSpec) -> Self {
        Self {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            installed: None,
            latest: None,
            update_available: false,
            state: "unchecked".to_string(),
            error: None,
            notice: None,
            checked_at: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredState {
    pub apps: Vec<AppStatus>,
    pub autostart_enabled: bool,
    pub autostart_initialized: bool,
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            apps: APPS.iter().map(AppStatus::new).collect(),
            autostart_enabled: false,
            autostart_initialized: false,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSnapshot {
    pub apps: Vec<AppStatus>,
    pub active_apps: Vec<String>,
    pub checking_all: bool,
    pub revision: u64,
}

pub struct AppState {
    data: StdMutex<StoredState>,
    state_file: PathBuf,
    coordinator: Coordinator,
    checkers: Checkers,
    publication: StdMutex<()>,
    revision: AtomicU64,
    events: broadcast::Sender<UiSnapshot>,
    resource_root: PathBuf,
    process_groups: StdMutex<HashSet<i32>>,
}

impl AppState {
    pub fn new(state_file: PathBuf, resource_root: PathBuf, mut data: StoredState) -> Result<Self, String> {
        for spec in APPS {
            if !data.apps.iter().any(|app| app.id == spec.id) {
                data.apps.push(AppStatus::new(&spec));
            }
        }
        data.apps.retain(|app| APPS.iter().any(|spec| app.id == spec.id));
        for app in &mut data.apps {
            // An interrupted operation must not leave the next session busy forever.
            if app.state == "checking" || app.state == "updating" {
                app.state = if app.checked_at.is_none() { "unchecked" }
                    else if app.update_available { "available" } else { "current" }.to_string();
            }
            if let Some(checked_at) = app.checked_at {
                if now_seconds().saturating_sub(checked_at) > STALE_AFTER {
                    app.state = "error".to_string();
                    app.error = Some("Comprobación caducada".to_string());
                }
            }
        }
        Ok(Self {
            data: StdMutex::new(data),
            state_file,
            coordinator: Coordinator::default(),
            checkers: Checkers::new()?,
            publication: StdMutex::new(()),
            revision: AtomicU64::new(0),
            events: broadcast::channel(64).0,
            resource_root,
            process_groups: StdMutex::new(HashSet::new()),
        })
    }

    pub fn snapshot(&self) -> UiSnapshot {
        let _publication = self.publication.lock().unwrap_or_else(|e| e.into_inner());
        self.snapshot_locked()
    }

    // The caller holds publication so responses cannot carry a revision from
    // a newer event alongside an older copy of the application data.
    fn snapshot_locked(&self) -> UiSnapshot {
        let (checking_all, active_apps) = self.coordinator.snapshot();
        UiSnapshot {
            apps: self.lock_data().apps.clone(),
            active_apps,
            checking_all,
            revision: self.revision.load(Ordering::Relaxed),
        }
    }

    fn lock_data(&self) -> std::sync::MutexGuard<'_, StoredState> {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn persist(&self) {
        let _publication = self.publication.lock().unwrap_or_else(|e| e.into_inner());
        self.write_state();
    }

    fn write_state(&self) {
        let snapshot = self.lock_data().clone();
        if let Err(error) = write_json_atomically(&self.state_file, &snapshot) {
            eprintln!("No se pudo guardar el estado: {error}");
        }
    }

    fn remember_process_group(&self, pid: i32) {
        self.process_groups.lock().unwrap_or_else(|e| e.into_inner()).insert(pid);
    }

    fn forget_process_group(&self, pid: i32) {
        self.process_groups.lock().unwrap_or_else(|e| e.into_inner()).remove(&pid);
    }

    pub fn stop_children(&self, force: bool) {
        let pids: Vec<i32> = self.process_groups.lock().unwrap_or_else(|e| e.into_inner()).iter().copied().collect();
        let signal = if force { Signal::SIGKILL } else { Signal::SIGTERM };
        for pid in pids {
            let _ = kill(Pid::from_raw(-pid), signal);
        }
    }
}


impl AppState {
    pub fn subscribe(&self) -> broadcast::Receiver<UiSnapshot> { self.events.subscribe() }
    pub fn set_autostart(&self, enabled: bool) { let mut d = self.lock_data(); d.autostart_enabled = enabled; d.autostart_initialized = true; drop(d); self.persist(); }
}

pub fn create(state_file: PathBuf, resource_root: PathBuf) -> Result<Arc<AppState>, String> {
    Ok(Arc::new(AppState::new(state_file.clone(), resource_root, load_state(&state_file))?))
}
pub async fn check_all(app: Arc<AppState>) -> Result<UiSnapshot, String> { check_all_inner(app.clone()).await?; Ok(app.snapshot()) }
pub async fn check_app(app: Arc<AppState>, id: String) -> Result<UiSnapshot, String> {
    let spec = *app_spec(&id).ok_or("Aplicación desconocida")?;
    let lease = app.coordinator.begin_app(spec.id)?;
    check_one(app.clone(), spec).await; drop(lease); publish_state(&app); Ok(app.snapshot())
}
pub async fn update_app(app: Arc<AppState>, id: String) -> Result<UiSnapshot, String> {
    let spec = *app_spec(&id).ok_or("Aplicación desconocida")?;
    let lease = app.coordinator.begin_app(spec.id)?;
    { let mut data = app.lock_data(); let status = data.apps.iter_mut().find(|item| item.id == spec.id).unwrap();
      if !status.update_available { return Err("No hay una actualización confirmada para esta aplicación".into()); }
      status.state = "updating".into(); status.error = None; status.notice = None; }
    publish_state(&app);
    if let Err(error) = install_and_confirm(&app, &spec).await { set_error(&app, spec.id, error); }
    drop(lease); publish_state(&app); Ok(app.snapshot())
}
async fn install_and_confirm(app: &Arc<AppState>, spec: &AppSpec) -> Result<(), String> {
    let candidate = read_check_result(app, spec).await?;
    if !candidate.update_available {
        apply_check_result(&mut app.lock_data(), spec.id, candidate);
        return Ok(());
    }
    run_script(app, spec.update_script, &candidate.latest, UPDATE_TIMEOUT).await?;
    let result = read_check_result(app, spec).await
        .map_err(|error| format!("No se pudo confirmar la versión instalada: {error}"))?;
    let advanced = result.installed != candidate.installed;
    let still_pending = result.update_available;
    {
        let state = app.clone();
        let mut data = state.lock_data();
        apply_check_result(&mut data, spec.id, result);
        let status = data.apps.iter_mut().find(|item| item.id == spec.id).expect("registro de aplicación");
        if !advanced || still_pending {
            status.state = "error".to_string();
            status.error = Some("La instalación terminó, pero no se confirmó una versión nueva".to_string());
        } else if spec.id == "codex-desktop" && codex_is_running() {
            status.notice = Some("Reinicia Codex Desktop para completar la actualización".to_string());
        }
    }
    Ok(())
}

async fn check_all_inner(app: Arc<AppState>) -> Result<(), String> {
    let ids: Vec<_> = APPS.iter().map(|spec| spec.id).collect();
    let batch = app.coordinator.begin_all(&ids)?;
    publish_state(&app);
    // All workers start before awaiting any result. In-flight individual checks
    // are joined through their completion ticket rather than run a second time.
    let mut workers = Vec::new();
    for (id, pending) in batch.checks {
        let handle = app.clone();
        workers.push((id, tokio::spawn(async move {
            match pending {
                PendingCheck::New(lease) => {
                    check_one(handle.clone(), *app_spec(id).expect("aplicación registrada")).await;
                    drop(lease);
                    publish_state(&handle);
                }
                PendingCheck::Existing(mut completion) => {
                    let _ = completion.wait_for(|finished| *finished).await;
                }
            }
        })));
    }
    for (id, worker) in workers {
        if worker.await.is_err() { set_error(&app, id, "La consulta se interrumpió. Vuelve a verificar.".to_string()); }
    }
    drop(batch.lease);
    publish_state(&app);
    Ok(())
}

async fn check_one(app: Arc<AppState>, spec: AppSpec) {
    {
        let state = app.clone();
        let mut data = state.lock_data();
        mark_checking(&mut data, std::slice::from_ref(&spec));
    }
    publish_state(&app);
    match read_check_result(&app, &spec).await {
        Ok(result) => {
            apply_check_result(&mut app.lock_data(), spec.id, result);
        }
        Err(error) => set_error(&app, spec.id, error),
    }
    publish_state(&app);
}

fn mark_checking(data: &mut StoredState, specs: &[AppSpec]) {
    for status in &mut data.apps {
        if specs.iter().any(|spec| spec.id == status.id) {
            status.state = "checking".to_string();
            status.error = None;
        }
    }
}

fn apply_check_result(data: &mut StoredState, id: &str, result: CheckResult) {
    if let Some(status) = data.apps.iter_mut().find(|item| item.id == id) {
        status.installed = Some(result.installed);
        status.latest = Some(result.latest);
        status.update_available = result.update_available;
        status.checked_at = Some(now_seconds());
        status.state = if status.update_available { "available" } else { "current" }.to_string();
        status.error = None;
    }
}

async fn read_check_result(app: &Arc<AppState>, spec: &AppSpec) -> Result<CheckResult, String> {
    let state = app.clone();
    timeout(CHECK_TIMEOUT, state.checkers.check(spec.id)).await
        .map_err(|_| "La consulta superó el tiempo límite. Vuelve a verificar.".to_string())?
}

async fn run_script(
    app: &Arc<AppState>,
    script: &str,
    target_version: &str,
    time_limit: Duration,
) -> Result<std::process::Output, String> {
    let path = script_path(app, script)?;
    if !path.is_file() {
        return Err("No se encontró el script de esta aplicación".to_string());
    }
    let mut command = Command::new("/usr/bin/bash");
    command.arg(path)
        .env("ACTUALIZACIONES_TARGET_VERSION", target_version)
        .current_dir("/")
        .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let child = command.spawn().map_err(|error| format!("No se pudo iniciar el proceso: {error}"))?;
    let pid = child.id().map(|value| value as i32);
    if let Some(pid) = pid {
        app.remember_process_group(pid);
    }

    let result = timeout(time_limit, child.wait_with_output()).await;
    match result {
        Ok(Ok(output)) => {
            if let Some(pid) = pid { app.forget_process_group(pid); }
            if output.status.success() {
                Ok(output)
            } else {
                let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
                eprintln!("Error al ejecutar {script}: {details}");
                Err(script_error_message(&details))
            }
        }
        Ok(Err(error)) => {
            if let Some(pid) = pid { app.forget_process_group(pid); }
            Err(format!("Falló la ejecución del proceso: {error}"))
        }
        Err(_) => {
            if let Some(pid) = pid {
                signal_process_group(pid, Signal::SIGTERM);
                sleep(Duration::from_millis(400)).await;
                signal_process_group(pid, Signal::SIGKILL);
                app.forget_process_group(pid);
            }
            Err("La operación superó el tiempo límite".to_string())
        }
    }
}

fn script_path(app: &Arc<AppState>, relative: &str) -> Result<PathBuf, String> {
    let path = app.resource_root.join(relative);
    if path.is_file() { Ok(path) } else { Err("No se encontró el script empaquetado".to_string()) }
}

fn app_spec(id: &str) -> Option<&'static AppSpec> {
    APPS.iter().find(|spec| spec.id == id)
}

fn codex_is_running() -> bool {
    let Ok(entries) = fs::read_dir("/proc") else { return false };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        fs::read_to_string(entry.path().join("comm"))
            .map(|command| command.trim() == "chatgpt")
            .unwrap_or(false)
    })
}

fn set_error(app: &Arc<AppState>, id: &str, message: String) {
    let state = app.clone();
    let mut data = state.lock_data();
    if let Some(status) = data.apps.iter_mut().find(|item| item.id == id) {
        status.state = "error".to_string();
        status.error = Some(concise(&message, 180));
    }
}

fn publish_state(app: &Arc<AppState>) {
    let state = app.clone();
    let _publication = state.publication.lock().unwrap_or_else(|e| e.into_inner());
    state.revision.fetch_add(1, Ordering::Relaxed);
    state.write_state();
    let _ = state.events.send(state.snapshot_locked());
}

fn load_state(path: &Path) -> StoredState {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<StoredState>(&bytes).ok()).unwrap_or_default()
}

fn write_json_atomically(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        let temporary = parent.join(format!("state.json.{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(temporary, path)?;
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        let temporary = parent.join(format!("state.json.{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(temporary, path)
    }
}

fn state_file_path() -> PathBuf {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|| std::env::temp_dir());
    root.join("actualizaciones/state.json")
}

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn concise(message: &str, max_chars: usize) -> String {
    let first_line = message.lines().find(|line| !line.trim().is_empty()).unwrap_or("Error desconocido").trim();
    first_line.chars().take(max_chars).collect()
}

fn script_error_message(details: &str) -> String {
    // The scripts append a useful explanation after curl or shell diagnostics.
    let message = details.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    if message.is_empty() || message.starts_with('/') || message.starts_with("curl:") {
        "No se pudo completar la operación. Vuelve a intentarlo.".to_string()
    } else {
        concise(message, 180)
    }
}


pub fn signal_process_group(pid: i32, signal: Signal) { let _ = kill(Pid::from_raw(-pid), signal); }
