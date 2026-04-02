use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tracing::info;

use crate::errors::AppResult;

/// Build and register the system tray icon with a French-labelled context menu.
///
/// Called from the `setup` hook in lib.rs. If tray support is unavailable on
/// the current platform the error is non-fatal — the caller logs a warning
/// and continues.
pub fn setup_tray(app: &mut tauri::App) -> AppResult<()> {
    let show_item = MenuItem::with_id(app, "tray_show", "Afficher Maintafox", true, None::<&str>)
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;
    let hide_item = MenuItem::with_id(app, "tray_hide", "Masquer", true, None::<&str>)
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;
    let quit_item = MenuItem::with_id(app, "tray_quit", "Quitter", true, None::<&str>)
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;

    let menu = Menu::with_items(app, &[&show_item, &hide_item, &quit_item])
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;

    TrayIconBuilder::new()
        .menu(&menu)
        .icon(app.default_window_icon().cloned().unwrap())
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray_show" => {
                if let Some(w) = app.get_webview_window("main") {
                    w.show().ok();
                    w.set_focus().ok();
                }
            }
            "tray_hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    w.hide().ok();
                }
            }
            "tray_quit" => {
                info!("Quit requested from tray");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click on tray icon: toggle visibility
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    if matches!(w.is_visible(), Ok(true)) {
                        w.hide().ok();
                    } else {
                        w.show().ok();
                        w.set_focus().ok();
                    }
                }
            }
        })
        .build(app)
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;

    Ok(())
}
