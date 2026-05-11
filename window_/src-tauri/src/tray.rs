use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Manager};

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop Recording", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(app, &[&show_item, &stop_item, &separator, &quit_item])?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .tooltip("Audio Recorder")
        .menu(&menu)
        .icon(app.default_window_icon().cloned().unwrap())
        .icon_as_template(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "stop" => {
                if let Some(state) = app.try_state::<crate::commands::RecorderState>() {
                    let _ = crate::commands::stop_recording(state);
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
