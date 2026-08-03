use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;
use tauri::{
    Emitter, Manager, Runtime, State, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tokio::sync::Mutex;
use usahp_daemon::service::{ServicePhase, ServiceSnapshot, ServiceSupervisor};

struct ControlState {
    service: Arc<Mutex<Option<ServiceSupervisor>>>,
    startup_error: Arc<Mutex<Option<String>>>,
    path_file: PathBuf,
}

#[derive(Serialize)]
struct ControlSnapshot {
    configured: bool,
    phase: ServicePhase,
    config_path: Option<String>,
    address: Option<String>,
    capture_enabled: bool,
    switches: Vec<usahp_core::SwitchSnapshot>,
    connections: Vec<usahp_daemon::management::ConnectionSnapshot>,
    active_session: Option<usahp_daemon::management::ActiveSessionSnapshot>,
    error: Option<String>,
}

impl ControlSnapshot {
    fn unconfigured(error: Option<String>) -> Self {
        Self {
            configured: false,
            phase: if error.is_some() {
                ServicePhase::Error
            } else {
                ServicePhase::Stopped
            },
            config_path: None,
            address: None,
            capture_enabled: false,
            switches: Vec::new(),
            connections: Vec::new(),
            active_session: None,
            error,
        }
    }
}

impl From<ServiceSnapshot> for ControlSnapshot {
    fn from(snapshot: ServiceSnapshot) -> Self {
        Self {
            configured: true,
            phase: snapshot.phase,
            config_path: Some(snapshot.config_path),
            address: Some(snapshot.address),
            capture_enabled: snapshot.capture_enabled,
            switches: snapshot.switches,
            connections: snapshot.connections,
            active_session: snapshot.active_session,
            error: snapshot.error,
        }
    }
}

fn read_remembered_path(path_file: &Path) -> Option<PathBuf> {
    std::fs::read_to_string(path_file)
        .ok()
        .map(|path| PathBuf::from(path.trim()))
        .filter(|path| !path.as_os_str().is_empty())
}

fn remember_path(path_file: &Path, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path_file, path.to_string_lossy().as_bytes())
}

#[tauri::command]
async fn service_snapshot(state: State<'_, ControlState>) -> Result<ControlSnapshot, String> {
    let service = state.service.lock().await;
    if let Some(service) = service.as_ref() {
        Ok(service.snapshot().into())
    } else {
        Ok(ControlSnapshot::unconfigured(
            state.startup_error.lock().await.clone(),
        ))
    }
}

#[tauri::command]
async fn start_service(state: State<'_, ControlState>) -> Result<(), String> {
    let mut service = state.service.lock().await;
    service
        .as_mut()
        .ok_or_else(|| "choose a configuration first".to_string())?
        .start()
        .await
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
async fn stop_service(state: State<'_, ControlState>) -> Result<(), String> {
    let mut service = state.service.lock().await;
    if let Some(service) = service.as_mut() {
        service.stop().await.map_err(|error| format!("{error:#}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn choose_config<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, ControlState>,
    path: String,
) -> Result<(), String> {
    let path = PathBuf::from(path);
    usahp_daemon::service::validate_config(&path).map_err(|error| format!("{error:#}"))?;
    remember_path(&state.path_file, &path).map_err(|error| error.to_string())?;

    let mut service = state.service.lock().await;
    if service.is_some() {
        drop(service);
        app.restart();
    }
    let mut loaded = ServiceSupervisor::load(&path)
        .await
        .map_err(|error| format!("{error:#}"))?;
    let start_result = loaded.start().await.map_err(|error| format!("{error:#}"));
    *service = Some(loaded);
    if start_result.is_ok() {
        *state.startup_error.lock().await = None;
    }
    start_result
}

#[tauri::command]
async fn quit_usahp<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, ControlState>,
) -> Result<(), String> {
    if let Some(service) = state.service.lock().await.as_mut() {
        service.stop().await.map_err(|error| format!("{error:#}"))?;
    }
    app.exit(0);
    Ok(())
}

fn show_main<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn request_or_perform<R: Runtime>(app: tauri::AppHandle<R>, action: &'static str) {
    tauri::async_runtime::spawn(async move {
        let has_session = {
            let state = app.state::<ControlState>();
            let service = state.service.lock().await;
            service
                .as_ref()
                .is_some_and(|service| service.snapshot().active_session.is_some())
        };
        if has_session {
            show_main(&app);
            let _ = app.emit("confirm-service-action", action);
            return;
        }
        let state = app.state::<ControlState>();
        match action {
            "stop" => {
                if let Some(service) = state.service.lock().await.as_mut() {
                    let _ = service.stop().await;
                }
            }
            "quit" => {
                if let Some(service) = state.service.lock().await.as_mut() {
                    let _ = service.stop().await;
                }
                app.exit(0);
            }
            _ => {}
        }
    });
}

fn build_tray<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open USAHP Control", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Start / Stop Service", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit USAHP", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &toggle, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("USAHP Control")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "toggle" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<ControlState>();
                    let phase = state
                        .service
                        .lock()
                        .await
                        .as_ref()
                        .map(|service| service.snapshot().phase);
                    if phase == Some(ServicePhase::Running) {
                        request_or_perform(app.clone(), "stop");
                    } else if let Some(service) = state.service.lock().await.as_mut() {
                        let _ = service.start().await;
                    } else {
                        show_main(&app);
                    }
                });
            }
            "quit" => request_or_perform(app.clone(), "quit"),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "usahp=info,usahp_control=info".into()),
        )
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let path_file = config_dir.join("config-path.txt");
            let saved_path = read_remembered_path(&path_file);
            let service = Arc::new(Mutex::new(None));
            let startup_error = Arc::new(Mutex::new(None));
            app.manage(ControlState {
                service: service.clone(),
                startup_error: startup_error.clone(),
                path_file,
            });
            build_tray(app)?;

            if let Some(path) = saved_path {
                tauri::async_runtime::spawn(async move {
                    match ServiceSupervisor::load(&path).await {
                        Ok(mut loaded) => {
                            let result = loaded.start().await;
                            *service.lock().await = Some(loaded);
                            if let Err(error) = result {
                                *startup_error.lock().await = Some(format!("{error:#}"));
                            }
                        }
                        Err(error) => *startup_error.lock().await = Some(format!("{error:#}")),
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Resized(_) if window.is_minimized().unwrap_or(false) => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            service_snapshot,
            start_service,
            stop_service,
            choose_config,
            quit_usahp
        ])
        .run(tauri::generate_context!())
        .expect("error while running USAHP Control");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("usahp-control-{name}-{}", std::process::id()))
    }

    #[test]
    fn remembered_path_round_trips_and_trims_newlines() {
        let directory = temporary_path("remembered-path");
        let path_file = directory.join("config-path.txt");
        let selected = directory.join("switches.toml");
        remember_path(&path_file, &selected).unwrap();
        assert_eq!(read_remembered_path(&path_file), Some(selected));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_or_blank_remembered_path_is_unconfigured() {
        let directory = temporary_path("missing-path");
        let path_file = directory.join("config-path.txt");
        assert_eq!(read_remembered_path(&path_file), None);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&path_file, "  \n").unwrap();
        assert_eq!(read_remembered_path(&path_file), None);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
