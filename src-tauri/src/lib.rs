use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const PET_WIDTH: i32 = 280;
const PET_HEIGHT: i32 = 260;

#[cfg(target_os = "windows")]
#[repr(C)]
struct LastInputInfo {
    size: u32,
    time: u32,
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
extern "system" {
    fn GetLastInputInfo(info: *mut LastInputInfo) -> i32;
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    always_on_top: bool,
    auto_start: bool,
    wandering: bool,
    sound: bool,
    scale: f64,
    pet_name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            always_on_top: true,
            auto_start: false,
            wandering: true,
            sound: true,
            scale: 1.0,
            pet_name: "也祝".into(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPatch {
    always_on_top: Option<bool>,
    auto_start: Option<bool>,
    wandering: Option<bool>,
    sound: Option<bool>,
    scale: Option<f64>,
    pet_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SavedPosition {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedData {
    #[serde(default)]
    settings: Settings,
    position: Option<SavedPosition>,
}

struct AppState {
    file_path: PathBuf,
    data: Mutex<PersistedData>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WalkTarget {
    x: i32,
    y: i32,
    edge: &'static str,
    joining: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowContext {
    bounds: Rect,
    work_area: Rect,
    target: WalkTarget,
}

#[derive(Clone, Serialize)]
struct PetReaction {
    state: &'static str,
    message: String,
}

fn sanitize_scale(value: f64) -> f64 {
    if !value.is_finite() {
        return 1.0;
    }
    ((value * 10.0).round() / 10.0).clamp(0.2, 1.4)
}

fn read_data(file_path: &PathBuf) -> PersistedData {
    let mut data: PersistedData = fs::read_to_string(file_path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default();
    if data.settings.pet_name == "团子" {
        data.settings.pet_name = "也祝".into();
    }
    data
}

fn write_data(state: &AppState) -> Result<(), String> {
    let data = state.data.lock().map_err(|_| "配置锁已损坏".to_string())?;
    if let Some(parent) = state.file_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = serde_json::to_string_pretty(&*data).map_err(|error| error.to_string())?;
    fs::write(&state.file_path, content).map_err(|error| error.to_string())
}

fn clamp_position(position: SavedPosition, area: &Rect) -> SavedPosition {
    SavedPosition {
        x: position.x.clamp(area.x, area.x + area.width as i32 - PET_WIDTH),
        y: position.y.clamp(area.y, area.y + area.height as i32 - PET_HEIGHT),
    }
}

fn rect_for_window(window: &WebviewWindow) -> Result<(Rect, Rect), String> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "找不到当前显示器".to_string())?;
    let work = monitor.work_area();
    Ok((
        Rect { x: position.x, y: position.y, width: size.width, height: size.height },
        Rect { x: work.position.x, y: work.position.y, width: work.size.width, height: work.size.height },
    ))
}

fn edge_target(bounds: &Rect, area: &Rect) -> WalkTarget {
    let left = area.x;
    let top = area.y;
    let right = area.x + area.width as i32 - bounds.width as i32;
    let bottom = area.y + area.height as i32 - bounds.height as i32;
    let x = bounds.x.clamp(left, right);
    let y = bounds.y.clamp(top, bottom);
    let distances = [(y - top).abs(), (x - right).abs(), (y - bottom).abs(), (x - left).abs()];
    let (nearest, distance) = distances.iter().enumerate().min_by_key(|(_, value)| *value).unwrap();
    if *distance > 4 {
        return match nearest {
            0 => WalkTarget { x, y: top, edge: "top", joining: true },
            1 => WalkTarget { x: right, y, edge: "right", joining: true },
            2 => WalkTarget { x, y: bottom, edge: "bottom", joining: true },
            _ => WalkTarget { x: left, y, edge: "left", joining: true },
        };
    }
    if (y - top).abs() <= 4 && x < right - 4 {
        WalkTarget { x: right, y: top, edge: "top", joining: false }
    } else if (x - right).abs() <= 4 && y < bottom - 4 {
        WalkTarget { x: right, y: bottom, edge: "right", joining: false }
    } else if (y - bottom).abs() <= 4 && x > left + 4 {
        WalkTarget { x: left, y: bottom, edge: "bottom", joining: false }
    } else if (x - left).abs() <= 4 && y > top + 4 {
        WalkTarget { x: left, y: top, edge: "left", joining: false }
    } else {
        WalkTarget { x: right, y: top, edge: "top", joining: false }
    }
}

fn open_settings_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|error| error.to_string())?;
        return window.set_focus().map_err(|error| error.to_string());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html?mode=settings".into()))
        .title("桌宠设置")
        .inner_size(380.0, 520.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .center()
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    state.data.lock().map(|data| data.settings.clone()).map_err(|_| "配置锁已损坏".into())
}

#[tauri::command]
fn update_settings(app: AppHandle, state: State<'_, AppState>, patch: SettingsPatch) -> Result<Settings, String> {
    let settings = {
        let mut data = state.data.lock().map_err(|_| "配置锁已损坏".to_string())?;
        let current = &mut data.settings;
        if let Some(value) = patch.always_on_top { current.always_on_top = value; }
        if let Some(value) = patch.auto_start { current.auto_start = value; }
        if let Some(value) = patch.wandering { current.wandering = value; }
        if let Some(value) = patch.sound { current.sound = value; }
        if let Some(value) = patch.scale { current.scale = sanitize_scale(value); }
        if let Some(value) = patch.pet_name {
            let trimmed = value.trim();
            if !trimmed.is_empty() { current.pet_name = trimmed.chars().take(12).collect(); }
        }
        current.clone()
    };
    write_data(&state)?;
    if let Some(window) = app.get_webview_window("main") {
        window.set_always_on_top(settings.always_on_top).map_err(|error| error.to_string())?;
    }
    let autostart = app.autolaunch();
    if settings.auto_start { autostart.enable() } else { autostart.disable() }.map_err(|error| error.to_string())?;
    app.emit_to("main", "settings:changed", settings.clone()).map_err(|error| error.to_string())?;
    app.emit_to("main", "pet:react", PetReaction { state: "happy", message: "设置已经保存好啦～".into() }).map_err(|error| error.to_string())?;
    Ok(settings)
}

#[tauri::command]
fn get_window_context(window: WebviewWindow) -> Result<WindowContext, String> {
    let (bounds, work_area) = rect_for_window(&window)?;
    let target = edge_target(&bounds, &work_area);
    Ok(WindowContext { bounds, work_area, target })
}

#[tauri::command]
fn move_by(window: WebviewWindow, x: f64, y: f64) -> Result<(), String> {
    if !x.is_finite() || !y.is_finite() { return Ok(()); }
    let (bounds, area) = rect_for_window(&window)?;
    let next = clamp_position(SavedPosition { x: bounds.x + x.round() as i32, y: bounds.y + y.round() as i32 }, &area);
    window.set_position(PhysicalPosition::new(next.x, next.y)).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_position(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    state.data.lock().map_err(|_| "配置锁已损坏".to_string())?.position = Some(SavedPosition { x: position.x, y: position.y });
    write_data(&state)
}

#[tauri::command]
fn get_user_idle_ms() -> u32 {
    #[cfg(target_os = "windows")]
    {
        let mut info = LastInputInfo { size: std::mem::size_of::<LastInputInfo>() as u32, time: 0 };
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            return u32::MAX;
        }
        return unsafe { GetTickCount() }.wrapping_sub(info.time);
    }
    #[cfg(not(target_os = "windows"))]
    u32::MAX
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> { open_settings_window(&app) }

#[tauri::command]
fn hide_window(window: WebviewWindow) -> Result<(), String> { window.hide().map_err(|error| error.to_string()) }

#[tauri::command]
fn close_settings(window: WebviewWindow) -> Result<(), String> { window.close().map_err(|error| error.to_string()) }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![
            get_settings, update_settings, get_window_context, move_by, save_position,
            get_user_idle_ms, open_settings, hide_window, close_settings
        ])
        .setup(|app| {
            let file_path = app.path().app_config_dir()?.join("settings.json");
            let data = read_data(&file_path);
            let initial_settings = data.settings.clone();
            let saved_position = data.position;
            app.manage(AppState { file_path, data: Mutex::new(data) });

            let show_item = MenuItem::with_id(app, "show", "显示桌宠", true, None::<&str>)?;
            let greet_item = MenuItem::with_id(app, "greet", "和它打招呼", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &greet_item, &separator, &settings_item, &quit_item])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().expect("应用图标缺失").clone())
                .tooltip("Open Desktop Pet")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "show" => { if let Some(window) = app.get_webview_window("main") { let _ = window.show(); } }
                    "greet" => {
                        if let Some(window) = app.get_webview_window("main") { let _ = window.show(); }
                        let _ = app.emit_to("main", "pet:react", PetReaction { state: "happy", message: "我在这里！".into() });
                    }
                    "settings" => { let _ = open_settings_window(app); }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("main") { let _ = window.show(); }
                    }
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                window.set_always_on_top(initial_settings.always_on_top)?;
                if let Some(position) = saved_position {
                    if let Ok((_, area)) = rect_for_window(&window) {
                        let safe = clamp_position(position, &area);
                        window.set_position(PhysicalPosition::new(safe.x, safe.y))?;
                    }
                }
                let pet_window = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = pet_window.hide();
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Open Desktop Pet");
}
