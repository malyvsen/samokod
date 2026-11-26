// Tauri command edge: thin impure glue over the domain modules.
mod acp;
mod agent;
mod attachments;
mod awake;
mod branch;
mod cli;
mod error_hint;
mod git;
mod menu;
mod opencode;
mod permissions;
mod plans;
mod prefs;
mod repo;
mod repo_state;
mod spend;
mod todos;
mod types;
mod updates;
mod windows;
mod working_title;
mod worktrees;

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, State, Window};
use tauri_plugin_log::{Target, TargetKind};

use tauri_specta::{Builder, collect_commands};

use crate::agent::AgentManager;
use crate::menu::{
    NEW_PLAN_ITEM_ID, NEW_WINDOW_ITEM_ID, OPEN_PICKER_ITEM_ID, TOGGLE_SETTINGS_ITEM_ID, build_menu,
    set_new_plan_enabled,
};
use crate::prefs::{load_prefs, record_open, save_prefs};
use crate::repo::{RepoInfo, validate_repo};
use crate::types::{AppEvent, ConfigOptionView, OpenRepoResult, PlansUpdate, Prefs, SessionKey};
use crate::windows::{
    create_window, focus_window as focus_window_by_label, open_picker_window, window_label,
};

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::new()
        // Progress counts and ETA seconds fit safely in a JS number.
        .dangerously_cast_bigints_to_number()
        .commands(collect_commands![
            get_prefs,
            validate_repo_path,
            open_repo,
            refresh_branch,
            create_plan,
            execute_plan,
            cancel_plan,
            select_plan,
            set_plan_mode,
            set_plan_evergreen,
            send_prompt,
            plan_md_text,
            retry_last,
            cancel_turn,
            answer_permission,
            set_global_config_option,
            save_attachment,
            warm_session,
            load_history,
            new_window,
            focus_window,
            list_open_repos,
            take_pending_repo
        ])
        .typ::<AppEvent>()
}

#[cfg(test)]
mod bindings_tests {
    use super::specta_builder;

    fn bindings_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.ts")
    }

    #[test]
    fn export_bindings_file() {
        use specta_typescript::Typescript;
        specta_builder()
            .export(Typescript::default(), bindings_path())
            .expect("export bindings.ts");
    }

    #[test]
    fn export_preserves_wire_casing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let temp = dir.path().join("bindings.ts");
        {
            use specta_typescript::Typescript;
            specta_builder()
                .export(Typescript::default(), &temp)
                .expect("export to temp");
        }
        let text = std::fs::read_to_string(&temp).expect("read temp");
        assert!(text.contains("currentValue"), "export keeps camel value");
        assert!(
            text.contains("has_plan_md"),
            "export keeps snake plan fields"
        );
        assert!(
            text.contains("config_defaults"),
            "export keeps snake defaults"
        );
    }
}

#[tauri::command]
#[specta::specta]
fn get_prefs(app: AppHandle) -> Result<Prefs, String> {
    Ok(load_prefs(&prefs_dir(&app)?))
}

#[tauri::command]
#[specta::specta]
fn validate_repo_path(path: String) -> Result<RepoInfo, String> {
    validate_repo(&PathBuf::from(path))
}

#[tauri::command]
#[specta::specta]
async fn open_repo(
    app: AppHandle,
    window: Window,
    state: State<'_, AgentManager>,
    path: String,
) -> Result<OpenRepoResult, String> {
    let info = validate_repo(&PathBuf::from(&path))?;
    let label = window_label(&window);
    let result = state
        .open_repo(&label, PathBuf::from(&info.root))
        .await
        .map_err(|error| error.to_string())?;
    let dir = prefs_dir(&app)?;
    let mut prefs = load_prefs(&dir);
    record_open(&mut prefs, &info.root);
    if let Err(error) = save_prefs(&dir, &prefs) {
        log::warn!("failed to save prefs after opening {}: {error}", info.root);
    }
    refresh_new_plan_menu(&app);
    Ok(result)
}

#[tauri::command]
#[specta::specta]
async fn refresh_branch(window: Window, state: State<'_, AgentManager>) -> Result<String, String> {
    state
        .refresh_branch(&window_label(&window))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn create_plan(
    window: Window,
    state: State<'_, AgentManager>,
    draft: Option<String>,
) -> Result<PlansUpdate, String> {
    state
        .create_plan(&window_label(&window), draft)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn execute_plan(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .execute_plan(&window_label(&window), session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn cancel_plan(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .cancel_plan(&window_label(&window), session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn select_plan(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
    draft: Option<String>,
) -> Result<PlansUpdate, String> {
    state
        .select_plan(&window_label(&window), session, draft)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn set_plan_mode(
    window: Window,
    state: State<'_, AgentManager>,
    plan: String,
    manual: bool,
) -> Result<PlansUpdate, String> {
    state
        .set_plan_mode(&window_label(&window), plan, manual)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn set_plan_evergreen(
    window: Window,
    state: State<'_, AgentManager>,
    plan: String,
    evergreen: bool,
) -> Result<PlansUpdate, String> {
    state
        .set_plan_evergreen(&window_label(&window), plan, evergreen)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn send_prompt(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
    text: String,
) -> Result<(), String> {
    state
        .send_prompt(&window_label(&window), session, text)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn retry_last(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<bool, String> {
    state
        .retry_last(&window_label(&window), session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn cancel_turn(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<(), String> {
    state
        .cancel_turn(&window_label(&window), session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn answer_permission(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
    tool_call_id: String,
    option_id: Option<String>,
) -> Result<(), String> {
    state
        .answer_permission(&window_label(&window), session, &tool_call_id, option_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn set_global_config_option(
    window: Window,
    state: State<'_, AgentManager>,
    config_id: String,
    value: String,
) -> Result<Vec<ConfigOptionView>, String> {
    state
        .set_global_config_option(&window_label(&window), config_id, value)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn plan_md_text(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<Option<String>, String> {
    state
        .plan_md_text(&window_label(&window), session)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
fn save_attachment(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
    data_base64: String,
) -> Result<String, String> {
    state
        .save_attachment(&window_label(&window), session, data_base64)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn warm_session(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<(), String> {
    state
        .warm_session(&window_label(&window), session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn load_history(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<(), String> {
    state
        .load_history(&window_label(&window), session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn new_window(app: AppHandle) -> Result<String, String> {
    create_window(&app)
}

#[tauri::command]
#[specta::specta]
fn focus_window(app: AppHandle, label: String) -> Result<(), String> {
    focus_window_by_label(&app, &label)
}

#[tauri::command]
#[specta::specta]
fn list_open_repos(state: State<'_, AgentManager>) -> Vec<crate::types::OpenRepoEntry> {
    state.list_open_repos()
}

#[tauri::command]
#[specta::specta]
fn take_pending_repo(window: Window) -> Option<String> {
    let label = window_label(&window);
    crate::cli::take_pending(&label).map(|path| path.to_string_lossy().to_string())
}

fn log_level(value: Option<&str>) -> log::LevelFilter {
    match value.unwrap_or_default().to_ascii_lowercase().as_str() {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::log_level;

    #[test]
    fn log_level_maps_known_names() {
        assert_eq!(log_level(Some("trace")), log::LevelFilter::Trace);
        assert_eq!(log_level(Some("DEBUG")), log::LevelFilter::Debug);
        assert_eq!(log_level(Some("warn")), log::LevelFilter::Warn);
        assert_eq!(log_level(Some("error")), log::LevelFilter::Error);
        assert_eq!(log_level(None), log::LevelFilter::Info);
        assert_eq!(log_level(Some("")), log::LevelFilter::Info);
        assert_eq!(log_level(Some("verbose")), log::LevelFilter::Info);
    }
}

fn prefs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|error| {
        let message = format!("failed to resolve app data dir: {error}");
        log::error!("{message}");
        message
    })
}

/// Second-process launch: open the forwarded path in this process, or a
/// fresh picker when no path was given. Invalid paths stay silent: the
/// invoking process already failed fast with the diagnostic.
fn handle_second_launch(app: &AppHandle, args: &[String], cwd: String) {
    let Some(raw) = cli::first_positional(args.iter()) else {
        open_picker_window(app, "single-instance second launch");
        return;
    };
    let root = match cli::resolve_and_validate(&raw, std::path::Path::new(&cwd)) {
        Ok(root) => root,
        Err(error) => {
            log::warn!("second-instance ignoring invalid path: {error}");
            return;
        }
    };
    let Some(manager) = app.try_state::<AgentManager>() else {
        log::warn!("second-instance path with no manager");
        return;
    };
    if let Err(error) = manager.open_path_in_window(root) {
        log::warn!("second-instance open failed: {error}");
    }
}

/// Forward a menu event to the focused window, falling back to a
/// broadcast when no window resolves as focused so single-window use
/// always works.
fn emit_to_focused_or_broadcast(app: &AppHandle, event: &str) {
    match focused_window_label(app) {
        Some(label) => {
            if let Err(error) = app.emit_to(label, event, ()) {
                log::warn!("{event} menu failed: {error}");
            }
        }
        None => {
            if let Err(error) = app.emit(event, ()) {
                log::warn!("{event} menu failed: {error}");
            }
        }
    }
}

fn focused_window_label(app: &AppHandle) -> Option<String> {
    app.webview_windows()
        .into_values()
        .find(|window| window.is_focused().unwrap_or(false))
        .map(|window| window.label().to_string())
}

fn emit_toggle_settings(app: &AppHandle) {
    emit_to_focused_or_broadcast(app, "samokod://toggle-settings");
}

fn emit_new_plan(app: &AppHandle) {
    emit_to_focused_or_broadcast(app, "samokod://new-plan");
}

/// Recompute the `New Plan` enabled flag from the focused window's repo
/// state. Runs after `open_repo`, on focus change, and on destroy; the
/// frontend picker no-op stays as the race safety net.
fn refresh_new_plan_menu(app: &AppHandle) {
    let enabled = match app.try_state::<AgentManager>() {
        Some(manager) => {
            let focused = focused_window_label(app);
            manager.new_plan_enabled(focused.as_deref())
        }
        None => false,
    };
    set_new_plan_enabled(app, enabled);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    cli::maybe_detach();
    if let Some(root) = cli::launch_root() {
        cli::insert_pending(windows::FIRST_WINDOW_LABEL, root);
    }
    let builder = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            handle_second_launch(app, &args, cwd);
        }))
        .setup(|app| {
            app.manage(AgentManager::new(app.handle().clone()));
            let attach_value = std::env::var(cli::ATTACH_TERMINAL_ENV).ok();
            let mut log_builder = tauri_plugin_log::Builder::new()
                .level(log_level(attach_value.as_deref()))
                .clear_targets()
                .target(Target::new(TargetKind::LogDir { file_name: None }));
            if cli::attach_terminal_opt_in(attach_value.as_deref()) {
                log_builder = log_builder.target(Target::new(TargetKind::Stdout));
            }
            app.handle().plugin(log_builder.build())?;
            let menu = build_menu(app.handle())?;
            app.set_menu(menu)?;
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            NEW_PLAN_ITEM_ID => emit_new_plan(app),
            NEW_WINDOW_ITEM_ID | OPEN_PICKER_ITEM_ID => {
                if let Err(error) = create_window(app) {
                    log::warn!("{} menu failed: {error}", event.id().as_ref());
                }
            }
            TOGGLE_SETTINGS_ITEM_ID => emit_toggle_settings(app),
            _ => {}
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Destroyed => {
                let app = window.app_handle().clone();
                let label = window.label().to_string();
                if let Some(manager) = app.try_state::<AgentManager>() {
                    manager.remove_window(&label);
                }
                refresh_new_plan_menu(&app);
            }
            tauri::WindowEvent::Focused(_) => {
                let app = window.app_handle().clone();
                refresh_new_plan_menu(&app);
            }
            _ => {}
        })
        .invoke_handler(builder.invoke_handler())
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Reopen { .. } = event {
                open_picker_window(app, "reopened");
            }
        });
}
