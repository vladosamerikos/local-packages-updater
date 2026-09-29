#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use futures_util::StreamExt;
use tauri::{Emitter, Manager, WebviewUrl};
use zbus::{Connection, Proxy};

const BUS: &str = "es.vlad.Actualizaciones.Agent";
const PATH: &str = "/es/vlad/Actualizaciones/Agent";
const INTERFACE: &str = "es.vlad.Actualizaciones.Agent1";

async fn proxy(connection: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(connection, BUS, PATH, INTERFACE).await.map_err(|error| error.to_string())
}
async fn agent_connection() -> Result<Connection, String> {
    Connection::session().await.map_err(|error| error.to_string())
}
#[tauri::command]
async fn get_state() -> Result<serde_json::Value, String> {
    let connection = agent_connection().await?;
    let answer: String = proxy(&connection).await?.call("GetState", &()).await.map_err(|error| error.to_string())?;
    serde_json::from_str(&answer).map_err(|error| error.to_string())
}
async fn start(method: &str, id: Option<&str>, request_id: &str) -> Result<String, String> {
    let connection = agent_connection().await?;
    let proxy = proxy(&connection).await?;
    match id {
        Some(id) => proxy.call(method, &(id, request_id)).await,
        None => proxy.call(method, &(request_id,)).await,
    }.map_err(|error| error.to_string())
}
#[tauri::command]
async fn check_all(request_id: String) -> Result<String, String> { start("StartCheckAll", None, &request_id).await }
#[tauri::command]
async fn check_app(id: String, request_id: String) -> Result<String, String> { start("StartCheckApp", Some(&id), &request_id).await }
#[tauri::command]
async fn update_app(id: String, request_id: String) -> Result<String, String> { start("StartUpdateApp", Some(&id), &request_id).await }

fn open_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show(); let _ = window.set_focus(); return;
    }
    if let Err(error) = tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Actualizaciones").inner_size(540.0, 278.0).resizable(false).maximizable(false).center().build() {
        eprintln!("No se pudo abrir la ventana: {error}");
    }
}
fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| open_window(app)))
        .invoke_handler(tauri::generate_handler![get_state, check_all, check_app, update_app])
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // The bus activates the agent if the UI starts before the login entry.
                loop {
                    match agent_connection().await {
                        Ok(connection) => match proxy(&connection).await {
                            Ok(proxy) => {
                                if let Ok(mut state_events) = proxy.receive_signal("StateChanged").await {
                                    let bus_proxy = Proxy::new(&connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").await;
                                    let mut owner_events = match bus_proxy {
                                        Ok(bus_proxy) => bus_proxy.receive_signal("NameOwnerChanged").await.ok(),
                                        Err(_) => None,
                                    };
                                    if let Ok(answer) = proxy.call::<_, _, String>("GetState", &()).await {
                                        if let Ok(snapshot) = serde_json::from_str::<serde_json::Value>(&answer) { let _ = handle.emit("state-changed", snapshot); }
                                    }
                                    loop {
                                        tokio::select! {
                                            state = state_events.next() => match state {
                                                Some(message) => if let Ok((payload,)) = message.body().deserialize::<(String,)>() {
                                                    if let Ok(snapshot) = serde_json::from_str::<serde_json::Value>(&payload) { let _ = handle.emit("state-changed", snapshot); }
                                                },
                                                None => break,
                                            },
                                            owner = async { owner_events.as_mut().unwrap().next().await }, if owner_events.is_some() => {
                                                if let Some(message) = owner {
                                                    if let Ok((name, _, _)) = message.body().deserialize::<(String, String, String)>() {
                                                        if name == BUS { break; }
                                                    }
                                                } else { break; }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(error) => eprintln!("Agente no disponible: {error}"),
                        },
                        Err(error) => eprintln!("Bus de sesión no disponible: {error}"),
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            });
            open_window(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!()).expect("No se pudo iniciar Actualizaciones");
    app.run(|_, _| {});
}
