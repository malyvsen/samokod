// App menu: App, File, Edit basics per Tauri menu defaults, plus File >
// New Window. Settings lives in the app menu on macOS and in File
// elsewhere. New Window binds Shift+Cmd+N on macOS; Cmd+N stays unbound
// for future new-plan use.
use tauri::AppHandle;

pub(crate) fn build_menu(app: &AppHandle) -> Result<tauri::menu::Menu<tauri::Wry>, String> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    let pkg = app.package_info();
    #[cfg(target_os = "macos")]
    let accelerator: Option<&str> = Some("Shift+Cmd+N");
    #[cfg(not(target_os = "macos"))]
    let accelerator: Option<&str> = None;
    let new_window = MenuItem::with_id(app, "new-window", "New Window", true, accelerator)
        .map_err(|error| menu_error("New Window item", &error))?;
    let toggle_settings = MenuItem::with_id(
        app,
        "toggle-settings",
        "Settings…",
        true,
        Some("CmdOrCtrl+,"),
    )
    .map_err(|error| menu_error("Settings item", &error))?;
    let close = PredefinedMenuItem::close_window(app, None)
        .map_err(|error| menu_error("Close Window item", &error))?;
    #[cfg(not(target_os = "macos"))]
    let quit =
        PredefinedMenuItem::quit(app, None).map_err(|error| menu_error("Quit item", &error))?;
    #[cfg(not(target_os = "macos"))]
    let settings_sep_before =
        PredefinedMenuItem::separator(app).map_err(|error| menu_error("File separator", &error))?;
    #[cfg(not(target_os = "macos"))]
    let settings_sep_after =
        PredefinedMenuItem::separator(app).map_err(|error| menu_error("File separator", &error))?;
    let mut file_items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&new_window];
    #[cfg(not(target_os = "macos"))]
    {
        file_items.push(&settings_sep_before);
        file_items.push(&toggle_settings);
        file_items.push(&settings_sep_after);
    }
    file_items.push(&close);
    #[cfg(not(target_os = "macos"))]
    file_items.push(&quit);
    let file = Submenu::with_items(app, "File", true, &file_items)
        .map_err(|error| menu_error("File menu", &error))?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)
                .map_err(|error| menu_error("Undo item", &error))?,
            &PredefinedMenuItem::redo(app, None)
                .map_err(|error| menu_error("Redo item", &error))?,
            &PredefinedMenuItem::separator(app)
                .map_err(|error| menu_error("Edit separator", &error))?,
            &PredefinedMenuItem::cut(app, None).map_err(|error| menu_error("Cut item", &error))?,
            &PredefinedMenuItem::copy(app, None)
                .map_err(|error| menu_error("Copy item", &error))?,
            &PredefinedMenuItem::paste(app, None)
                .map_err(|error| menu_error("Paste item", &error))?,
            &PredefinedMenuItem::select_all(app, None)
                .map_err(|error| menu_error("Select All item", &error))?,
        ],
    )
    .map_err(|error| menu_error("Edit menu", &error))?;
    #[cfg(target_os = "macos")]
    {
        let app_menu = Submenu::with_items(
            app,
            pkg.name.clone(),
            true,
            &[
                &PredefinedMenuItem::about(app, None, None)
                    .map_err(|error| menu_error("About item", &error))?,
                &PredefinedMenuItem::separator(app)
                    .map_err(|error| menu_error("App separator", &error))?,
                &toggle_settings,
                &PredefinedMenuItem::separator(app)
                    .map_err(|error| menu_error("App separator", &error))?,
                &PredefinedMenuItem::quit(app, None)
                    .map_err(|error| menu_error("Quit item", &error))?,
            ],
        )
        .map_err(|error| menu_error("App menu", &error))?;
        Menu::with_items(app, &[&app_menu, &file, &edit])
            .map_err(|error| menu_error("menu", &error))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Menu::with_items(app, &[&file, &edit]).map_err(|error| menu_error("menu", &error))
    }
}

fn menu_error(what: &str, error: &dyn std::fmt::Display) -> String {
    log::warn!("failed to build {what}: {error}");
    format!("failed to build menu: {error}")
}
