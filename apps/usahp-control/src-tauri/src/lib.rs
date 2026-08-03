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
use usahp_daemon::{
    management::{CaptureAvailability, CaptureStatus},
    service::{
        ServicePhase, ServiceSnapshot, ServiceSupervisor, is_capture_permission_required,
        request_capture_permission,
    },
};

const DEFAULT_CONFIG: &str = include_str!("../resources/default-config.toml");

#[derive(Clone)]
struct ControlState {
    service: Arc<Mutex<Option<ServiceSupervisor>>>,
    fallback: Arc<Mutex<ControlSnapshot>>,
    lifecycle: Arc<Mutex<()>>,
    path_file: PathBuf,
}

#[derive(Clone, Serialize)]
struct ControlSnapshot {
    configured: bool,
    phase: ServicePhase,
    config_path: Option<String>,
    address: Option<String>,
    capture: CaptureStatus,
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
            capture: CaptureStatus {
                active: false,
                availability: CaptureAvailability::Unavailable,
                message: error.clone(),
            },
            switches: Vec::new(),
            connections: Vec::new(),
            active_session: None,
            error,
        }
    }

    fn loading(path: &Path) -> Self {
        Self {
            configured: true,
            phase: ServicePhase::Starting,
            config_path: Some(path.display().to_string()),
            address: None,
            capture: CaptureStatus {
                active: false,
                availability: CaptureAvailability::Available,
                message: None,
            },
            switches: Vec::new(),
            connections: Vec::new(),
            active_session: None,
            error: None,
        }
    }

    fn load_error(path: &Path, error: &anyhow::Error) -> Self {
        let permission_required = is_capture_permission_required(error);
        let message = format!("{error:#}");
        Self {
            configured: true,
            phase: ServicePhase::Error,
            config_path: Some(path.display().to_string()),
            address: None,
            capture: CaptureStatus {
                active: false,
                availability: if permission_required {
                    CaptureAvailability::PermissionRequired
                } else {
                    CaptureAvailability::Unavailable
                },
                message: Some(message.clone()),
            },
            switches: Vec::new(),
            connections: Vec::new(),
            active_session: None,
            error: Some(message),
        }
    }

    fn with_phase(mut self, phase: ServicePhase) -> Self {
        self.phase = phase;
        self.error = None;
        self
    }
}

impl From<ServiceSnapshot> for ControlSnapshot {
    fn from(snapshot: ServiceSnapshot) -> Self {
        Self {
            configured: true,
            phase: snapshot.phase,
            config_path: Some(snapshot.config_path),
            address: Some(snapshot.address),
            capture: snapshot.capture,
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

fn initial_config_path(config_dir: &Path, path_file: &Path) -> std::io::Result<PathBuf> {
    if let Some(path) = read_remembered_path(path_file) {
        return Ok(path);
    }
    std::fs::create_dir_all(config_dir)?;
    let default_path = config_dir.join("default.toml");
    if !default_path.exists() {
        std::fs::write(&default_path, DEFAULT_CONFIG)?;
    }
    remember_path(path_file, &default_path)?;
    Ok(default_path)
}

async fn snapshot_from(state: &ControlState) -> ControlSnapshot {
    let service = state.service.lock().await;
    if let Some(service) = service.as_ref() {
        service.snapshot().into()
    } else {
        drop(service);
        state.fallback.lock().await.clone()
    }
}

async fn install_service(state: &ControlState, service: ServiceSupervisor) {
    let snapshot = service.snapshot().into();
    *state.service.lock().await = Some(service);
    *state.fallback.lock().await = snapshot;
}

async fn load_and_start(state: &ControlState, path: &Path) -> Result<(), String> {
    *state.fallback.lock().await = ControlSnapshot::loading(path);
    let mut loaded = match ServiceSupervisor::load(path).await {
        Ok(loaded) => loaded,
        Err(error) => {
            *state.fallback.lock().await = ControlSnapshot::load_error(path, &error);
            return Err(format!("{error:#}"));
        }
    };
    let result = loaded.start().await.map_err(|error| format!("{error:#}"));
    install_service(state, loaded).await;
    result
}

async fn run_with_service<F, Fut>(
    state: &ControlState,
    phase: ServicePhase,
    operation: F,
) -> Result<(), String>
where
    F: FnOnce(ServiceSupervisor) -> Fut,
    Fut: std::future::Future<Output = (ServiceSupervisor, Result<(), String>)>,
{
    let mut service = state.service.lock().await;
    let loaded = service
        .take()
        .ok_or_else(|| "choose a configuration first".to_string())?;
    *state.fallback.lock().await = ControlSnapshot::from(loaded.snapshot()).with_phase(phase);
    drop(service);

    let (loaded, result) = operation(loaded).await;
    install_service(state, loaded).await;
    result
}

async fn start_runtime(state: &ControlState) -> Result<(), String> {
    let _operation = state
        .lifecycle
        .try_lock()
        .map_err(|_| "another service operation is already in progress".to_string())?;
    run_with_service(state, ServicePhase::Starting, |mut service| async move {
        let result = service.start().await.map_err(|error| format!("{error:#}"));
        (service, result)
    })
    .await
}

async fn stop_runtime(state: &ControlState) -> Result<(), String> {
    let _operation = state
        .lifecycle
        .try_lock()
        .map_err(|_| "another service operation is already in progress".to_string())?;
    if state.service.lock().await.is_none() {
        return Ok(());
    }
    run_with_service(state, ServicePhase::Stopping, |mut service| async move {
        let result = service.stop().await.map_err(|error| format!("{error:#}"));
        (service, result)
    })
    .await
}

#[tauri::command]
async fn service_snapshot(state: State<'_, ControlState>) -> Result<ControlSnapshot, String> {
    Ok(snapshot_from(&state).await)
}

#[tauri::command]
async fn start_service(state: State<'_, ControlState>) -> Result<(), String> {
    start_runtime(&state).await
}

#[tauri::command]
async fn stop_service(state: State<'_, ControlState>) -> Result<(), String> {
    stop_runtime(&state).await
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

    if snapshot_from(&state).await.configured {
        app.restart();
    }
    let _operation = state
        .lifecycle
        .try_lock()
        .map_err(|_| "another service operation is already in progress".to_string())?;
    load_and_start(&state, &path).await
}

#[tauri::command]
async fn grant_capture_permission(state: State<'_, ControlState>) -> Result<bool, String> {
    if !request_capture_permission() {
        return Ok(false);
    }
    let path = read_remembered_path(&state.path_file)
        .ok_or_else(|| "choose a configuration first".to_string())?;
    let _operation = state
        .lifecycle
        .try_lock()
        .map_err(|_| "another service operation is already in progress".to_string())?;
    load_and_start(&state, &path).await?;
    Ok(true)
}

#[tauri::command]
async fn quit_usahp<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, ControlState>,
) -> Result<(), String> {
    stop_runtime(&state).await?;
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
            snapshot_from(&state).await.active_session.is_some()
        };
        if has_session {
            show_main(&app);
            let _ = app.emit("confirm-service-action", action);
            return;
        }
        let state = app.state::<ControlState>();
        match action {
            "stop" => {
                let _ = stop_runtime(&state).await;
            }
            "quit" if stop_runtime(&state).await.is_ok() => app.exit(0),
            "quit" => {}
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
                    if snapshot_from(&state).await.phase == ServicePhase::Running {
                        request_or_perform(app.clone(), "stop");
                    } else if start_runtime(&state).await.is_err() {
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
            let initial_path = initial_config_path(&config_dir, &path_file);
            let initial_error = initial_path.as_ref().err().map(ToString::to_string);
            let fallback = Arc::new(Mutex::new(ControlSnapshot::unconfigured(initial_error)));
            let state = ControlState {
                service: Arc::new(Mutex::new(None)),
                fallback: fallback.clone(),
                lifecycle: Arc::new(Mutex::new(())),
                path_file: path_file.clone(),
            };
            app.manage(state.clone());
            build_tray(app)?;

            match initial_path {
                Ok(path) => {
                    tauri::async_runtime::spawn(async move {
                        let _operation = state.lifecycle.lock().await;
                        let _ = load_and_start(&state, &path).await;
                    });
                }
                Err(error) => {
                    tracing::error!(%error, "could not create the first-run configuration")
                }
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
            grant_capture_permission,
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

    #[test]
    fn first_run_creates_and_remembers_default_without_overwriting_it() {
        let directory = temporary_path("first-run-default");
        let path_file = directory.join("config-path.txt");
        let default_path = initial_config_path(&directory, &path_file).unwrap();
        assert_eq!(
            std::fs::read_to_string(&default_path).unwrap(),
            DEFAULT_CONFIG
        );
        assert_eq!(read_remembered_path(&path_file), Some(default_path.clone()));
        usahp_daemon::service::validate_config(&default_path).unwrap();

        std::fs::write(&default_path, "user edited").unwrap();
        assert_eq!(
            initial_config_path(&directory, &path_file).unwrap(),
            default_path
        );
        assert_eq!(
            std::fs::read_to_string(&default_path).unwrap(),
            "user edited"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn existing_remembered_path_wins_over_generated_default() {
        let directory = temporary_path("custom-path");
        let path_file = directory.join("config-path.txt");
        let selected = directory.join("custom.toml");
        remember_path(&path_file, &selected).unwrap();
        assert_eq!(
            initial_config_path(&directory, &path_file).unwrap(),
            selected
        );
        assert!(!directory.join("default.toml").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_remembered_path_is_retained_for_visible_error_reporting() {
        let directory = temporary_path("invalid-remembered-path");
        let path_file = directory.join("config-path.txt");
        let missing = directory.join("missing.toml");
        remember_path(&path_file, &missing).unwrap();
        assert_eq!(
            initial_config_path(&directory, &path_file).unwrap(),
            missing
        );
        assert!(!directory.join("default.toml").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn lifecycle_gate_rejects_conflicts_without_blocking_snapshots() {
        let state = ControlState {
            service: Arc::new(Mutex::new(None)),
            fallback: Arc::new(Mutex::new(ControlSnapshot::unconfigured(None))),
            lifecycle: Arc::new(Mutex::new(())),
            path_file: temporary_path("lifecycle").join("config-path.txt"),
        };
        let _operation = state.lifecycle.lock().await;
        let error = start_runtime(&state).await.unwrap_err();
        assert!(error.contains("already in progress"));
        let snapshot =
            tokio::time::timeout(std::time::Duration::from_millis(50), snapshot_from(&state))
                .await
                .expect("snapshot should not wait for the lifecycle operation");
        assert_eq!(snapshot.phase, ServicePhase::Stopped);
    }
}
