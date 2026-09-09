use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const WINDOW_WIDTH: f64 = 280.0;
const WINDOW_HEIGHT: f64 = 260.0;
const PET_BASE_SIZE: f64 = 165.0;
const PET_RIGHT: f64 = 25.0;
const PET_BOTTOM: f64 = 12.0;

#[cfg(target_os = "windows")]
#[repr(C)]
struct LastInputInfo {
    size: u32,
    time: u32,
}

#[cfg(target_os = "windows")]
static KEYBOARD_HOOK_READY: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static LAST_KEYBOARD_INPUT: AtomicU32 = AtomicU32::new(0);

#[cfg(target_os = "windows")]
#[link(name = "user32")]
extern "system" {
    fn GetLastInputInfo(info: *mut LastInputInfo) -> i32;
    fn SetWindowsHookExW(
        id_hook: i32,
        callback: Option<unsafe extern "system" fn(i32, usize, isize) -> isize>,
        module: isize,
        thread_id: u32,
    ) -> isize;
    fn CallNextHookEx(hook: isize, code: i32, message: usize, data: isize) -> isize;
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
    fn GetModuleHandleW(module_name: *const u16) -> isize;
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn keyboard_activity_hook(code: i32, message: usize, data: isize) -> isize {
    const WM_KEYDOWN: usize = 0x0100;
    const WM_SYSKEYDOWN: usize = 0x0104;
    if code >= 0 && (message == WM_KEYDOWN || message == WM_SYSKEYDOWN) {
        LAST_KEYBOARD_INPUT.store(GetTickCount(), Ordering::Relaxed);
    }
    CallNextHookEx(0, code, message, data)
}

#[cfg(target_os = "windows")]
fn install_keyboard_activity_hook() {
    const WH_KEYBOARD_LL: i32 = 13;
    LAST_KEYBOARD_INPUT.store(unsafe { GetTickCount() }.wrapping_sub(60_000), Ordering::Relaxed);
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_activity_hook), module, 0) };
    KEYBOARD_HOOK_READY.store(hook != 0, Ordering::Relaxed);
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

struct MovementLimits {
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
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

struct PetContextMenu(Menu<tauri::Wry>);

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserActivity {
    idle_ms: u32,
    keyboard_idle_ms: Option<u32>,
}

#[derive(Clone, Serialize)]
struct PetReaction {
    state: &'static str,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsUpdateResult {
    settings: Settings,
    warning: Option<String>,
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

fn movement_limits(window: &WebviewWindow, area: &Rect, scale: f64) -> Result<MovementLimits, String> {
    let dpi_scale = window.scale_factor().map_err(|error| error.to_string())?;
    let visible_size = PET_BASE_SIZE * sanitize_scale(scale) * dpi_scale;
    let center_x = (WINDOW_WIDTH - PET_RIGHT - PET_BASE_SIZE / 2.0) * dpi_scale;
    let visible_left = center_x - visible_size / 2.0;
    let visible_right = center_x + visible_size / 2.0;
    let visible_bottom = (WINDOW_HEIGHT - PET_BOTTOM) * dpi_scale;
    let visible_top = visible_bottom - visible_size;

    Ok(MovementLimits {
        left: (area.x as f64 - visible_left).round() as i32,
        right: (area.x as f64 + area.width as f64 - visible_right).round() as i32,
        top: (area.y as f64 - visible_top).round() as i32,
        bottom: (area.y as f64 + area.height as f64 - visible_bottom).round() as i32,
    })
}

fn clamp_position(position: SavedPosition, limits: &MovementLimits) -> SavedPosition {
    SavedPosition {
        x: position.x.clamp(limits.left, limits.right),
        y: position.y.clamp(limits.top, limits.bottom),
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

fn edge_target(bounds: &Rect, limits: &MovementLimits) -> WalkTarget {
    let left = limits.left;
    let top = limits.top;
    let right = limits.right;
    let bottom = limits.bottom;
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
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "设置窗口尚未初始化".to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    state.data.lock().map(|data| data.settings.clone()).map_err(|_| "配置锁已损坏".into())
}

#[tauri::command]
fn update_settings(app: AppHandle, state: State<'_, AppState>, patch: SettingsPatch) -> Result<SettingsUpdateResult, String> {
    let previous_auto_start = state
        .data
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?
        .settings
        .auto_start;
    let requested_auto_start = patch.auto_start.unwrap_or(previous_auto_start);
    let mut effective_auto_start = requested_auto_start;
    let mut warning = None;

    let autostart = app.autolaunch();
    match autostart.is_enabled() {
        Ok(enabled) if enabled == requested_auto_start => {}
        Ok(_) => {
            let result = if requested_auto_start { autostart.enable() } else { autostart.disable() };
            if let Err(error) = result {
                effective_auto_start = previous_auto_start;
                warning = Some(format!("其他设置已保存，但开机启动设置失败：{error}"));
            }
        }
        Err(error) => {
            effective_auto_start = previous_auto_start;
            warning = Some(format!("其他设置已保存，但无法读取开机启动状态：{error}"));
        }
    }

    let settings = {
        let mut data = state.data.lock().map_err(|_| "配置锁已损坏".to_string())?;
        let current = &mut data.settings;
        if let Some(value) = patch.always_on_top { current.always_on_top = value; }
        current.auto_start = effective_auto_start;
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
        let (bounds, area) = rect_for_window(&window)?;
        let limits = movement_limits(&window, &area, settings.scale)?;
        let safe = clamp_position(SavedPosition { x: bounds.x, y: bounds.y }, &limits);
        window.set_position(PhysicalPosition::new(safe.x, safe.y)).map_err(|error| error.to_string())?;
    }
    app.emit_to("main", "settings:changed", settings.clone()).map_err(|error| error.to_string())?;
    app.emit_to("main", "pet:react", PetReaction { state: "happy", message: "设置已经保存好啦～".into() }).map_err(|error| error.to_string())?;
    Ok(SettingsUpdateResult { settings, warning })
}

#[tauri::command]
fn get_window_context(window: WebviewWindow, state: State<'_, AppState>) -> Result<WindowContext, String> {
    let (bounds, work_area) = rect_for_window(&window)?;
    let scale = state.data.lock().map_err(|_| "配置锁已损坏".to_string())?.settings.scale;
    let limits = movement_limits(&window, &work_area, scale)?;
    let target = edge_target(&bounds, &limits);
    Ok(WindowContext { bounds, work_area, target })
}

#[tauri::command]
fn move_by(window: WebviewWindow, state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    if !x.is_finite() || !y.is_finite() { return Ok(()); }
    let (bounds, area) = rect_for_window(&window)?;
    let scale = state.data.lock().map_err(|_| "配置锁已损坏".to_string())?.settings.scale;
    let limits = movement_limits(&window, &area, scale)?;
    let next = clamp_position(SavedPosition { x: bounds.x + x.round() as i32, y: bounds.y + y.round() as i32 }, &limits);
    window.set_position(PhysicalPosition::new(next.x, next.y)).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_position(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    state.data.lock().map_err(|_| "配置锁已损坏".to_string())?.position = Some(SavedPosition { x: position.x, y: position.y });
    write_data(&state)
}

#[tauri::command]
fn get_user_activity() -> UserActivity {
    #[cfg(target_os = "windows")]
    {
        let mut info = LastInputInfo { size: std::mem::size_of::<LastInputInfo>() as u32, time: 0 };
        let now = unsafe { GetTickCount() };
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            return UserActivity { idle_ms: u32::MAX, keyboard_idle_ms: None };
        }
        let keyboard_idle_ms = KEYBOARD_HOOK_READY
            .load(Ordering::Relaxed)
            .then(|| now.wrapping_sub(LAST_KEYBOARD_INPUT.load(Ordering::Relaxed)));
        return UserActivity { idle_ms: now.wrapping_sub(info.time), keyboard_idle_ms };
    }
    #[cfg(not(target_os = "windows"))]
    UserActivity { idle_ms: u32::MAX, keyboard_idle_ms: None }
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> { open_settings_window(&app) }

#[tauri::command]
fn show_pet_menu(window: WebviewWindow, menu: State<'_, PetContextMenu>) -> Result<(), String> {
    window.popup_menu(&menu.0).map_err(|error| error.to_string())
}

#[tauri::command]
fn start_settings_drag(window: WebviewWindow) -> Result<(), String> {
    if window.label() != "settings" {
        return Err("只有设置窗口可以从标题栏拖动".into());
    }
    window.start_dragging().map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_window(window: WebviewWindow) -> Result<(), String> { window.hide().map_err(|error| error.to_string()) }

#[tauri::command]
fn close_settings(window: WebviewWindow) -> Result<(), String> { window.hide().map_err(|error| error.to_string()) }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "pet_greet" => {
                if let Some(window) = app.get_webview_window("main") { let _ = window.show(); }
                let _ = app.emit_to("main", "pet:react", PetReaction {
                    state: "happy",
                    message: "你好呀，我也正想和你打招呼～".into(),
                });
            }
            "pet_settings" => { let _ = open_settings_window(app); }
            "pet_hide" => {
                if let Some(window) = app.get_webview_window("main") { let _ = window.hide(); }
            }
            "pet_quit" => app.exit(0),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_settings, update_settings, get_window_context, move_by, save_position,
            get_user_activity, open_settings, show_pet_menu, start_settings_drag,
            hide_window, close_settings
        ])
        .setup(|app| {
            #[cfg(target_os = "windows")]
            install_keyboard_activity_hook();

            let file_path = app.path().app_config_dir()?.join("settings.json");
            let data = read_data(&file_path);
            let initial_settings = data.settings.clone();
            let saved_position = data.position;
            app.manage(AppState { file_path, data: Mutex::new(data) });

            let pet_greet_item = MenuItem::with_id(app, "pet_greet", "和也祝打招呼", true, None::<&str>)?;
            let pet_settings_item = MenuItem::with_id(app, "pet_settings", "打开设置…", true, None::<&str>)?;
            let pet_separator = PredefinedMenuItem::separator(app)?;
            let pet_hide_item = MenuItem::with_id(app, "pet_hide", "让也祝退下", true, None::<&str>)?;
            let pet_quit_item = MenuItem::with_id(app, "pet_quit", "退出程序", true, None::<&str>)?;
            let pet_menu = Menu::with_items(app, &[
                &pet_greet_item,
                &pet_settings_item,
                &pet_separator,
                &pet_hide_item,
                &pet_quit_item,
            ])?;
            app.manage(PetContextMenu(pet_menu));

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
                        let limits = movement_limits(&window, &area, initial_settings.scale)?;
                        let safe = clamp_position(position, &limits);
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
            if let Some(window) = app.get_webview_window("settings") {
                let settings_window = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = settings_window.hide();
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Open Desktop Pet");
}
