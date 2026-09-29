#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nix::{sys::signal::{kill, Signal}, unistd::Pid};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{atomic::{AtomicBool, Ordering}, Mutex as StdMutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    path::BaseDirectory,
    tray::{TrayIconBuilder},
    AppHandle, Emitter, Manager, State, WebviewUrl,
};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tokio::{process::Command, sync::Mutex, time::{sleep, timeout}};

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const STALE_AFTER: u64 = 12 * 60 * 60;
const PERIODIC_CHECK: Duration = Duration::from_secs(12 * 60 * 60);
const INITIAL_CHECK_DELAY: Duration = Duration::from_secs(90);
const EVENT_STATE_CHANGED: &str = "state-changed";

#[derive(Clone, Copy)]
struct AppSpec {
    id: &'static str,
    name: &'static str,
    check_script: &'static str,
    update_script: &'static str,
}

const APPS: [AppSpec; 2] = [
    AppSpec {
        id: "codex-desktop",
        name: "Codex Desktop",
        check_script: "apps/codex-desktop/check.sh",
        update_script: "apps/codex-desktop/update.sh",
    },
    AppSpec {
        id: "spotifast",
        name: "Spotifast",
        check_script: "apps/spotifast/check.sh",
        update_script: "apps/spotifast/update.sh",
    },
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    id: String,
    name: String,
    installed: Option<String>,
    latest: Option<String>,
    update_available: bool,
    state: String,
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
struct StoredState {
    apps: Vec<AppStatus>,
    autostart_enabled: bool,
    autostart_initialized: bool,
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
struct UiSnapshot {
    apps: Vec<AppStatus>,
    busy: bool,
}

struct AppState {
    data: StdMutex<StoredState>,
    state_file: PathBuf,
    operation: Mutex<()>,
    busy: AtomicBool,
    allow_exit: AtomicBool,
    process_groups: StdMutex<HashSet<i32>>,
}

impl AppState {
    fn new(state_file: PathBuf, mut data: StoredState) -> Self {
        for spec in APPS {
            if !data.apps.iter().any(|app| app.id == spec.id) {
                data.apps.push(AppStatus::new(&spec));
            }
        }
        data.apps.retain(|app| APPS.iter().any(|spec| app.id == spec.id));
        for app in &mut data.apps {
            if let Some(checked_at) = app.checked_at {
                if now_seconds().saturating_sub(checked_at) > STALE_AFTER {
                    app.state = "error".to_string();
                    app.error = Some("Comprobación caducada".to_string());
                }
            }
        }
        Self {
            data: StdMutex::new(data),
            state_file,
            operation: Mutex::new(()),
            busy: AtomicBool::new(false),
            allow_exit: AtomicBool::new(false),
            process_groups: StdMutex::new(HashSet::new()),
        }
    }

    fn snapshot(&self) -> UiSnapshot {
        UiSnapshot {
            apps: self.lock_data().apps.clone(),
            busy: self.busy.load(Ordering::Relaxed),
        }
    }

    fn lock_data(&self) -> std::sync::MutexGuard<'_, StoredState> {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn persist(&self) {
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

    fn stop_children(&self, force: bool) {
        let pids: Vec<i32> = self.process_groups.lock().unwrap_or_else(|e| e.into_inner()).iter().copied().collect();
        let signal = if force { Signal::SIGKILL } else { Signal::SIGTERM };
        for pid in pids {
            let _ = kill(Pid::from_raw(-pid), signal);
        }
    }
}

#[derive(Deserialize)]
struct CheckResult {
    installed: String,
    latest: String,
    update_available: bool,
}

fn main() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            open_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .invoke_handler(tauri::generate_handler![get_state, check_all, update_app])
        .setup(|app| {
            let state_file = state_file_path();
            let mut data = load_state(&state_file);
            let autostart = app.autolaunch();
            let mut autostart_enabled = autostart.is_enabled().unwrap_or(false);
            if !data.autostart_initialized && !autostart_enabled {
                if autostart.enable().is_ok() {
                    autostart_enabled = true;
                    data.autostart_initialized = true;
                }
            } else {
                data.autostart_initialized = true;
            }
            data.autostart_enabled = autostart_enabled;

            app.manage(AppState::new(state_file, data));
            let saved = app.state::<AppState>().lock_data().clone();
            build_tray(app, saved.autostart_enabled)?;
            app.state::<AppState>().persist();
            update_tray(app.handle());

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                sleep(INITIAL_CHECK_DELAY).await;
                loop {
                    let _ = check_all_inner(app_handle.clone()).await;
                    sleep(PERIODIC_CHECK).await;
                }
            });

            if std::env::args().any(|arg| arg == "--open") {
                open_window(app.handle());
            }
            Ok(())
        });

    let app = builder.build(tauri::generate_context!()).expect("No se pudo iniciar Actualizaciones");
    app.run(|app_handle, event| {
        match event {
            tauri::RunEvent::ExitRequested { api, .. } => {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    if !state.allow_exit.load(Ordering::Relaxed) {
                        api.prevent_exit();
                    }
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    state.stop_children(true);
                }
            }
            _ => {}
        }
    });
}

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> UiSnapshot {
    state.snapshot()
}

#[tauri::command]
async fn check_all(app: AppHandle) -> Result<(), String> {
    check_all_inner(app).await
}

#[tauri::command]
async fn update_app(app: AppHandle, id: String) -> Result<(), String> {
    let spec = app_spec(&id).ok_or_else(|| "Aplicación desconocida".to_string())?;
    let state = app.state::<AppState>();
    let _operation = state.operation.lock().await;
    let previous_version = {
        let mut data = state.lock_data();
        let status = data.apps.iter_mut().find(|item| item.id == spec.id).expect("registro de aplicación");
        if !status.update_available {
            return Err("No hay una actualización confirmada para esta aplicación".to_string());
        }
        status.state = "updating".to_string();
        status.error = None;
        status.notice = None;
        status.installed.clone()
    };
    state.busy.store(true, Ordering::Relaxed);
    publish_state(&app);

    let result = run_script(&app, spec.update_script, UPDATE_TIMEOUT).await;
    match result {
        Err(error) => set_error(&app, spec.id, error),
        Ok(_) => {
            {
                let mut data = state.lock_data();
                let status = data.apps.iter_mut().find(|item| item.id == spec.id).expect("registro de aplicación");
                status.state = "checking".to_string();
            }
            publish_state(&app);
            match read_check_result(&app, spec).await {
                Ok(result) => {
                    let mut data = state.lock_data();
                    let status = data.apps.iter_mut().find(|item| item.id == spec.id).expect("registro de aplicación");
                    let version_advanced = previous_version.as_deref() != Some(result.installed.as_str());
                    status.installed = Some(result.installed);
                    status.latest = Some(result.latest);
                    status.update_available = result.update_available;
                    status.checked_at = Some(now_seconds());
                    if !version_advanced || status.update_available {
                        status.state = "error".to_string();
                        status.error = Some("La instalación terminó, pero no se confirmó una versión nueva".to_string());
                    } else {
                        status.state = "current".to_string();
                        status.error = None;
                        if spec.id == "codex-desktop" && codex_is_running() {
                            status.notice = Some("Reinicia Codex Desktop para completar la actualización".to_string());
                        }
                    }
                }
                Err(error) => set_error(&app, spec.id, format!("No se pudo confirmar la versión instalada: {error}")),
            }
        }
    }
    state.busy.store(false, Ordering::Relaxed);
    publish_state(&app);
    Ok(())
}

async fn check_all_inner(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _operation = state.operation.lock().await;
    state.busy.store(true, Ordering::Relaxed);
    {
        let mut data = state.lock_data();
        for status in &mut data.apps {
            status.state = "checking".to_string();
            status.error = None;
        }
    }
    publish_state(&app);

    for spec in APPS {
        match read_check_result(&app, &spec).await {
            Ok(result) => {
                let mut data = state.lock_data();
                if let Some(status) = data.apps.iter_mut().find(|item| item.id == spec.id) {
                    status.installed = Some(result.installed);
                    status.latest = Some(result.latest);
                    status.update_available = result.update_available;
            status.checked_at = Some(now_seconds());
            status.state = if status.update_available { "available" } else { "current" }.to_string();
            status.error = None;
                }
            }
            Err(error) => {
                let mut data = state.lock_data();
                if let Some(status) = data.apps.iter_mut().find(|item| item.id == spec.id) {
                    status.state = "error".to_string();
                    status.error = Some(error);
                }
            }
        }
        publish_state(&app);
    }
    state.busy.store(false, Ordering::Relaxed);
    publish_state(&app);
    Ok(())
}

async fn read_check_result(app: &AppHandle, spec: &AppSpec) -> Result<CheckResult, String> {
    let output = run_script(app, spec.check_script, CHECK_TIMEOUT).await?;
    serde_json::from_slice::<CheckResult>(&output.stdout)
        .map_err(|_| "El script devolvió una respuesta JSON no válida".to_string())
}

async fn run_script(
    app: &AppHandle,
    script: &str,
    time_limit: Duration,
) -> Result<std::process::Output, String> {
    let path = script_path(app, script)?;
    if !path.is_file() {
        return Err("No se encontró el script de esta aplicación".to_string());
    }
    let mut command = Command::new("/usr/bin/bash");
    command.arg(path)
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
        app.state::<AppState>().remember_process_group(pid);
    }

    let result = timeout(time_limit, child.wait_with_output()).await;
    match result {
        Ok(Ok(output)) => {
            if let Some(pid) = pid { app.state::<AppState>().forget_process_group(pid); }
            if output.status.success() {
                Ok(output)
            } else {
                let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(if details.is_empty() { "El proceso terminó con error".to_string() } else { concise(&details, 180) })
            }
        }
        Ok(Err(error)) => {
            if let Some(pid) = pid { app.state::<AppState>().forget_process_group(pid); }
            Err(format!("Falló la ejecución del proceso: {error}"))
        }
        Err(_) => {
            if let Some(pid) = pid {
                signal_process_group(pid, Signal::SIGTERM);
                sleep(Duration::from_millis(400)).await;
                signal_process_group(pid, Signal::SIGKILL);
                app.state::<AppState>().forget_process_group(pid);
            }
            Err("La operación superó el tiempo límite".to_string())
        }
    }
}

fn script_path(app: &AppHandle, relative: &str) -> Result<PathBuf, String> {
    if let Ok(path) = app.path().resolve(relative, BaseDirectory::Resource) {
        if path.is_file() { return Ok(path); }
    }
    let development_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join(relative);
    if development_path.is_file() {
        return Ok(development_path);
    }
    Err("No se encontró el script empaquetado".to_string())
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

fn set_error(app: &AppHandle, id: &str, message: String) {
    let state = app.state::<AppState>();
    let mut data = state.lock_data();
    if let Some(status) = data.apps.iter_mut().find(|item| item.id == id) {
        status.state = "error".to_string();
        status.error = Some(concise(&message, 180));
    }
}

fn publish_state(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.persist();
    let _ = app.emit(EVENT_STATE_CHANGED, state.snapshot());
    update_tray(app);
}

fn update_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("updates") else { return };
    let state = app.state::<AppState>();
    let pending = state.lock_data().apps.iter().any(|status| status.update_available);
    let icon = if pending { pending_icon() } else { normal_icon() };
    let _ = tray.set_icon(Some(icon));
}

fn normal_icon() -> Image<'static> {
    Image::from_bytes(include_bytes!("../icons/tray-normal.png")).expect("icono de bandeja")
}

fn pending_icon() -> Image<'static> {
    Image::from_bytes(include_bytes!("../icons/tray-pending.png")).expect("icono de bandeja")
}

fn build_tray(app: &tauri::App, autostart_enabled: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Abrir", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "Comprobar", true, None::<&str>)?;
    let autostart_text = if autostart_enabled { "Desactivar inicio automático" } else { "Activar inicio automático" };
    let autostart = MenuItem::with_id(app, "autostart", autostart_text, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &check, &autostart, &separator, &quit])?;
    let autostart_item = autostart.clone();
    TrayIconBuilder::with_id("updates")
        .icon(normal_icon())
        .tooltip("Actualizaciones")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => open_window(app),
            "check" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move { let _ = check_all_inner(handle).await; });
            }
            "autostart" => toggle_autostart(app, &autostart_item),
            "quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.allow_exit.store(true, Ordering::Relaxed);
                }
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn toggle_autostart(app: &AppHandle, item: &MenuItem<tauri::Wry>) {
    let manager = app.autolaunch();
    let was_enabled = manager.is_enabled().unwrap_or(false);
    let result = if was_enabled { manager.disable() } else { manager.enable() };
    if let Err(error) = result {
        eprintln!("No se pudo cambiar el inicio automático: {error}");
        return;
    }
    let enabled = !was_enabled;
    let label = if enabled { "Desactivar inicio automático" } else { "Activar inicio automático" };
    let _ = item.set_text(label);
    let state = app.state::<AppState>();
    {
        let mut data = state.lock_data();
        data.autostart_enabled = enabled;
        data.autostart_initialized = true;
    }
    publish_state(app);
}

fn open_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    match tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Actualizaciones")
        .inner_size(440.0, 250.0)
        .min_inner_size(400.0, 220.0)
        .max_inner_size(620.0, 650.0)
        .resizable(true)
        .center()
        .build()
    {
        Ok(window) => {
            let closing_window = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = closing_window.destroy();
                }
            });
        }
        Err(error) => eprintln!("No se pudo abrir la ventana: {error}"),
    }
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

fn signal_process_group(pid: i32, signal: Signal) {
    let _ = kill(Pid::from_raw(-pid), signal);
}
