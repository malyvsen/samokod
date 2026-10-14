// Tauri command edge: thin impure glue over the domain modules.
mod acp;
mod agent;
mod attachments;
mod awake;
mod branch;
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

use tauri::{AppHandle, Manager, State, Window};
use tauri_plugin_log::{Target, TargetKind};

use crate::agent::AgentManager;
use crate::menu::build_menu;
use crate::prefs::{load_prefs, record_open, save_prefs};
use crate::repo::{RepoInfo, validate_repo};
use crate::types::{ConfigOptionView, OpenRepoResult, PlansUpdate, Prefs, SessionKey};
use crate::windows::{create_window, focus_window as focus_window_by_label, window_label};

#[tauri::command]
fn get_prefs(app: AppHandle) -> Result<Prefs, String> {
    Ok(load_prefs(&prefs_dir(&app)?))
}

#[tauri::command]
fn validate_repo_path(path: String) -> Result<RepoInfo, String> {
    validate_repo(&PathBuf::from(path))
}

#[tauri::command]
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
    Ok(result)
}

#[tauri::command]
async fn refresh_branch(window: Window, state: State<'_, AgentManager>) -> Result<String, String> {
    state
        .refresh_branch(&window_label(&window))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
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
async fn finish_landing(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .finish_landing(&window_label(&window), session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn begin_landing(
    window: Window,
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .begin_landing(&window_label(&window), session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
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
async fn new_window(app: AppHandle) -> Result<String, String> {
    create_window(&app)
}

#[tauri::command]
fn focus_window(app: AppHandle, label: String) -> Result<(), String> {
    focus_window_by_label(&app, &label)
}

#[tauri::command]
fn list_open_repos(state: State<'_, AgentManager>) -> Vec<crate::types::OpenRepoEntry> {
    state.list_open_repos()
}

fn log_level() -> log::LevelFilter {
    match std::env::var("SAMOKOD_LOG")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    }
}

fn prefs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|error| {
        let message = format!("failed to resolve app data dir: {error}");
        log::error!("{message}");
        message
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Err(error) = create_window(app) {
                log::warn!("second instance failed to open window: {error}");
            }
        }))
        .setup(|app| {
            app.manage(AgentManager::new(app.handle().clone()));
            app.handle().plugin(
                tauri_plugin_log::Builder::new()
                    .level(log_level())
                    .target(Target::new(TargetKind::Stdout))
                    .build(),
            )?;
            let menu = build_menu(app.handle())?;
            app.set_menu(menu)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "new-window"
                && let Err(error) = create_window(app)
            {
                log::warn!("New Window menu failed: {error}");
            }
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let app = window.app_handle().clone();
                let label = window.label().to_string();
                if let Some(manager) = app.try_state::<AgentManager>() {
                    manager.remove_window(&label);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_prefs,
            validate_repo_path,
            open_repo,
            refresh_branch,
            create_plan,
            execute_plan,
            finish_landing,
            begin_landing,
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
            list_open_repos
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
