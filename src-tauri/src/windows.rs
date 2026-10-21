// Window identity and factory: labels, lookup, and creation.
// Every window owns one repo; labels are `main` for the first window and
// `window-N` for the rest.
use tauri::{AppHandle, Manager, WebviewWindowBuilder};

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
    let mut config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or_else(|| "no window config".to_string())?;
    config.label = label.clone();
    WebviewWindowBuilder::from_config(app, &config)
        .map_err(|error| {
            log::warn!("failed to build window {label}: {error}");
            format!("failed to build window {label}: {error}")
        })?
        .build()
        .map_err(|error| {
            log::warn!("failed to open window {label}: {error}");
            format!("failed to open window {label}: {error}")
        })?;
    log::info!("open window {label}");
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
            log::warn!("failed to focus window {label}: {error}");
            format!("failed to focus window {label}: {error}")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_creates_yield_distinct_labels_while_both_live() {
        let first = allocate_window_label(&[]);
        let existing = vec!["main".to_string(), first.clone()];
        let second = allocate_window_label(&existing);
        assert_ne!(first, second);
        assert_eq!(first, "window-1");
        assert_eq!(second, "window-2");
    }

    #[test]
    fn labels_never_reuse_closed_slots() {
        assert_eq!(allocate_window_label(&[]), "window-1");
        assert_eq!(
            allocate_window_label(&["main".to_string(), "window-1".to_string()]),
            "window-2"
        );
        // A closed window-2 leaves a gap, but allocation moves forward so a
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
