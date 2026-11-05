// Window identity and factory: labels, lookup, and creation.
// Every window owns one repo; labels are `main` for the first window and
// `window-N` for the rest.
use std::path::PathBuf;

use tauri::{AppHandle, Manager, WebviewWindowBuilder};

/// Label of the first window: Tauri defaults the single unlabeled entry in
/// `tauri.conf.json` to `main`.
pub(crate) const FIRST_WINDOW_LABEL: &str = "main";

/// Label for a Tauri window. Pure.
pub(crate) fn window_label(window: &tauri::Window) -> String {
    window.label().to_string()
}

/// Fresh `window-N` label above every live `window-N`. Pure and monotonic:
/// closed labels are never reused, so a stale task for a dead window can
/// never emit into a new window that recycled its label.
pub(crate) fn allocate_window_label(existing: &[String]) -> String {
    let mut max: u64 = 0;
    for label in existing {
        if let Some(suffix) = label.strip_prefix("window-")
            && let Ok(index) = suffix.parse::<u64>()
        {
            max = max.max(index);
        }
    }
    format!("window-{}", max + 1)
}

/// Shared window factory: label `window-N`, same title, size, and min size
/// as the `main` entry in `tauri.conf.json`, loading the same frontend.
/// New windows land on the picker with empty backend state.
pub(crate) fn create_window(app: &AppHandle) -> Result<String, String> {
    let existing: Vec<String> = app.webview_windows().keys().cloned().collect();
    let label = allocate_window_label(&existing);
    create_window_with_label(app, &label)
}

/// Build a window with a pre-allocated label.
pub(crate) fn create_window_with_label(app: &AppHandle, label: &str) -> Result<String, String> {
    let mut config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or_else(|| "no window config".to_string())?;
    config.label = label.to_string();
    WebviewWindowBuilder::from_config(app, &config)
        .map_err(|error| {
            log::warn!("failed to build window={label}: {error}");
            format!("failed to build window {label}: {error}")
        })?
        .build()
        .map_err(|error| {
            log::warn!("failed to open window={label}: {error}");
            format!("failed to open window {label}: {error}")
        })?;
    log::info!("open window={label}");
    Ok(label.to_string())
}

/// Allocate the next label, stage `root` for it, then build the window.
/// Staging precedes the build so the new window's mount claim cannot race
/// the frontend load.
pub(crate) fn create_pending_window(app: &AppHandle, root: PathBuf) -> Result<String, String> {
    let existing: Vec<String> = app.webview_windows().keys().cloned().collect();
    let label = allocate_window_label(&existing);
    crate::cli::insert_pending(&label, root);
    let label = create_window_with_label(app, &label)?;
    log::info!("open window={label} with staged repo");
    Ok(label)
}

/// Focus one window by label, unminimizing first. Used for the New Window
/// flow and for switching to the owner of an already-open repo.
pub(crate) fn focus_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let Some(window) = app.get_webview_window(label) else {
        return Err(format!("no such window: {label}"));
    };
    window
        .unminimize()
        .and_then(|()| window.set_focus())
        .map_err(|error| {
            log::warn!("failed to focus window={label}: {error}");
            format!("failed to focus window {label}: {error}")
        })
}

/// Open a fresh picker window for a launch trigger (`single-instance` or
/// `reopened`). Dock-click focus stays best-effort: opening a new picker is
/// the accepted fallback.
pub(crate) fn open_picker_window(app: &AppHandle, reason: &str) {
    log::info!("{reason}: opening picker window");
    match create_window(app) {
        Ok(label) => log::info!("{reason} opened picker window={label}"),
        Err(error) => log::warn!("{reason} failed to open window: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_allocate_monotonically_above_live_labels() {
        // Sequential creates yield distinct labels while both live.
        let first = allocate_window_label(&[]);
        let existing = vec!["main".to_string(), first.clone()];
        let second = allocate_window_label(&existing);
        assert_eq!(first, "window-1");
        assert_eq!(second, "window-2");
        // A closed label leaves a gap, but allocation moves forward so a
        // stale task for the dead window can never hit the new one.
        assert_eq!(
            allocate_window_label(&[
                "main".to_string(),
                "window-1".to_string(),
                "window-3".to_string()
            ]),
            "window-4"
        );
    }
}
