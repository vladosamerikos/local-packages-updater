use actualizaciones_core::{self as core, AppState};
use ksni::TrayMethods;
use std::{collections::HashMap, path::PathBuf, process::Command, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, time::Duration};
use tokio::sync::{mpsc, Notify};
use zbus::{interface, Connection, object_server::SignalEmitter};
use nix::{sys::signal::{kill, Signal}, unistd::Pid};

const BUS: &str = "es.vlad.Actualizaciones.Agent";
const PATH: &str = "/es/vlad/Actualizaciones/Agent";
const UI: &str = "/usr/bin/actualizaciones";
const AUTOSTART: &str = "Actualizaciones.desktop";

#[derive(Clone, Copy)]
enum TrayAction { Open, Check, Autostart, Quit }
struct Tray { actions: mpsc::UnboundedSender<TrayAction>, pending: bool, autostart: bool }
impl ksni::Tray for Tray {
    fn id(&self) -> String { "actualizaciones".into() }
    fn title(&self) -> String { "Actualizaciones".into() }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let bytes: &[u8] = if self.pending { include_bytes!("../../src-tauri/icons/tray-pending.png") }
            else { include_bytes!("../../src-tauri/icons/tray-normal.png") };
        let image = image::load_from_memory(bytes).expect("icono SNI").into_rgba8();
        let (width, height) = image.dimensions();
        let mut data = image.into_vec();
        for pixel in data.chunks_exact_mut(4) { pixel.rotate_right(1); }
        vec![ksni::Icon { width: width as i32, height: height as i32, data }]
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let entry = |label: &str, action| -> ksni::MenuItem<Self> {
            StandardItem { label: label.into(), activate: Box::new(move |tray: &mut Self| { let _ = tray.actions.send(action); }), ..Default::default() }.into()
        };
        vec![entry("Abrir", TrayAction::Open), entry("Comprobar", TrayAction::Check),
             entry(if self.autostart { "Desactivar inicio automático" } else { "Activar inicio automático" }, TrayAction::Autostart),
             ksni::MenuItem::Separator, entry("Salir", TrayAction::Quit)]
    }
}

#[derive(Clone)]
struct Agent {
    core: Arc<AppState>,
    instance: String,
    requests: Arc<Mutex<HashMap<String, String>>>,
    counter: Arc<AtomicU64>,
    quitting: Arc<AtomicBool>,
    quit: Arc<Notify>,
}
impl Agent {
    fn new(core: Arc<AppState>) -> Self {
        Self { core, instance: format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()),
            requests: Arc::new(Mutex::new(HashMap::new())), counter: Arc::new(AtomicU64::new(0)), quitting: Arc::new(AtomicBool::new(false)), quit: Arc::new(Notify::new()) }
    }
    fn snapshot(&self) -> String {
        let mut value = serde_json::to_value(self.core.snapshot()).unwrap();
        value["protocolVersion"] = 1.into();
        value["instanceId"] = self.instance.clone().into();
        value.to_string()
    }
    fn start(&self, action: &str, id: &str, request: &str, conn: Connection) -> zbus::fdo::Result<String> {
        if self.quitting.load(Ordering::Relaxed) { return Err(zbus::fdo::Error::Failed("El agente está cerrando".into())); }
        if request.len() > 128 || request.is_empty() || !request.is_ascii() { return Err(zbus::fdo::Error::InvalidArgs("requestId no válido".into())); }
        let key = format!("{action}:{id}:{request}");
        let mut requests = self.requests.lock().unwrap();
        if let Some(operation) = requests.get(&key) { return Ok(operation.clone()); }
        let worker = match action {
            "check_app" => core::start_check_app(self.core.clone(), id),
            "check_all" => core::start_check_all(self.core.clone()),
            "update_app" => core::start_update_app(self.core.clone(), id),
            _ => return Err(zbus::fdo::Error::InvalidArgs("Acción no válida".into())),
        }.map_err(zbus::fdo::Error::Failed)?;
        let operation = format!("{}-{}", self.instance, self.counter.fetch_add(1, Ordering::Relaxed));
        if requests.len() > 512 { requests.clear(); }
        requests.insert(key, operation.clone());
        drop(requests);
        let operation_for_signal = operation.clone();
        let core_for_signal = self.core.clone();
        let id_for_signal = id.to_string();
        tokio::spawn(async move {
            let result = worker.await.map(|_| {
                core_for_signal.snapshot().apps.into_iter()
                    .filter(|app| id_for_signal.is_empty() || app.id == id_for_signal)
                    .filter_map(|app| if app.state == "error" { app.error } else { None })
                    .collect::<Vec<_>>().join("; ")
            }).unwrap_or_else(|error| error.to_string());
            if let Ok(emitter) = SignalEmitter::new(&conn, PATH) {
                let _ = Agent::operation_finished(&emitter, &operation_for_signal, &result).await;
            }
        });
        Ok(operation)
    }
}
#[interface(name = "es.vlad.Actualizaciones.Agent1")]
impl Agent {
    fn get_state(&self) -> String { self.snapshot() }
    fn start_check_app(&self, id: &str, request_id: &str, #[zbus(connection)] conn: &Connection) -> zbus::fdo::Result<String> { self.start("check_app", id, request_id, conn.clone()) }
    fn start_check_all(&self, request_id: &str, #[zbus(connection)] conn: &Connection) -> zbus::fdo::Result<String> { self.start("check_all", "", request_id, conn.clone()) }
    fn start_update_app(&self, id: &str, request_id: &str, #[zbus(connection)] conn: &Connection) -> zbus::fdo::Result<String> { self.start("update_app", id, request_id, conn.clone()) }
    fn shutdown(&self) { self.quitting.store(true, Ordering::Relaxed); self.quit.notify_waiters(); }
    #[zbus(signal)]
    async fn state_changed(emitter: &SignalEmitter<'_>, snapshot: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn operation_finished(emitter: &SignalEmitter<'_>, operation_id: &str, error: &str) -> zbus::Result<()>;
}
fn autostart_path() -> PathBuf { PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/autostart").join(AUTOSTART) }
fn autostart_enabled() -> bool { autostart_path().is_file() }
fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let path = autostart_path();
    if enabled {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, "[Desktop Entry]\nType=Application\nName=Actualizaciones\nExec=/usr/bin/actualizaciones-agent --autostart\nTerminal=false\nX-GNOME-Autostart-enabled=true\n")
    } else if path.exists() { std::fs::remove_file(path) } else { Ok(()) }
}
fn open_ui() {
    if let Err(error) = Command::new(UI).arg("--open").spawn() { eprintln!("No se pudo abrir Actualizaciones: {error}"); }
}
fn close_ui() {
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Some(pid) = entry.file_name().to_string_lossy().parse::<i32>().ok() else { continue };
            if std::fs::read_link(entry.path().join("exe")).ok().as_deref() == Some(std::path::Path::new(UI)) {
                let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
            }
        }
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resource_root = std::env::var_os("ACTUALIZACIONES_RESOURCE_ROOT").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/lib/Actualizaciones"));
    let core = core::create(core::state_file_path(), resource_root)?;
    // Preserve the current preference; migration of the old entry happens in the installer.
    core.set_autostart(autostart_enabled());
    let agent = Agent::new(core.clone());
    let conn = Connection::session().await?;
    conn.request_name_with_flags(BUS, zbus::fdo::RequestNameFlags::DoNotQueue.into()).await?;
    conn.object_server().at(PATH, agent.clone()).await?;
    let (actions, mut incoming) = mpsc::unbounded_channel();
    let tray = Tray { actions, pending: core.snapshot().apps.iter().any(|app| app.update_available), autostart: autostart_enabled() };
    let tray_handle = tray.spawn().await?;
    let mut events = core.subscribe();
    let emitter = SignalEmitter::new(&conn, PATH)?;
    let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + Duration::from_secs(90), Duration::from_secs(12 * 60 * 60));
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            _ = timer.tick() => { let _ = agent.start("check_all", "", &format!("timer-{}", agent.counter.fetch_add(1, Ordering::Relaxed)), conn.clone()); }
            event = events.recv() => {
                if event.is_ok() {
                    let snapshot = agent.snapshot();
                    let _ = Agent::state_changed(&emitter, &snapshot).await;
                    let pending = core.snapshot().apps.iter().any(|app| app.update_available);
                    tray_handle.update(move |tray| tray.pending = pending).await;
                }
            }
            Some(action) = incoming.recv() => match action {
                TrayAction::Open => open_ui(),
                TrayAction::Check => { let _ = agent.start("check_all", "", &format!("tray-{}", agent.counter.fetch_add(1, Ordering::Relaxed)), conn.clone()); },
                TrayAction::Autostart => {
                    let enabled = !autostart_enabled();
                    if let Err(error) = set_autostart(enabled) { eprintln!("Autostart: {error}"); }
                    else { core.set_autostart(enabled); tray_handle.update(move |tray| tray.autostart = enabled).await; }
                },
                TrayAction::Quit => { agent.quitting.store(true, Ordering::Relaxed); break; }
            },
            _ = agent.quit.notified() => break,
            _ = tokio::signal::ctrl_c() => break,
            _ = terminate.recv() => break,
        }
    }
    // Manual installations own their process group and may take up to 20 minutes.
    while core.snapshot().apps.iter().any(|app| app.state == "updating") { tokio::time::sleep(Duration::from_millis(250)).await; }
    close_ui();
    core.persist();
    Ok(())
}
