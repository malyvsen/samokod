// Tauri command edge: thin impure glue over the domain modules.
mod acp;
mod agent;
mod awake;
mod branch;
mod error_hint;
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
mod worktrees;

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_log::{Target, TargetKind};

use crate::agent::AgentManager;
use crate::prefs::{load_prefs, record_open, save_prefs};
use crate::repo::{RepoInfo, validate_repo};
use crate::types::{ConfigOptionView, OpenRepoResult, PlansUpdate, Prefs, SessionKey};

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
    state: State<'_, AgentManager>,
    path: String,
) -> Result<OpenRepoResult, String> {
    let info = validate_repo(&PathBuf::from(&path))?;
    let result = state
        .open_repo(PathBuf::from(&info.root))
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
async fn refresh_branch(state: State<'_, AgentManager>) -> Result<String, String> {
    state
        .refresh_branch()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_plan(state: State<'_, AgentManager>) -> Result<PlansUpdate, String> {
    state.create_plan().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn execute_plan(
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .execute_plan(session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn mark_completed(
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .mark_completed(session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn abandon_plan(
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .abandon_plan(session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn cancel_execution(
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .cancel_execution(session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn select_plan(
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<PlansUpdate, String> {
    state
        .select_plan(session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn send_prompt(
    state: State<'_, AgentManager>,
    session: SessionKey,
    text: String,
) -> Result<(), String> {
    state
        .send_prompt(session, text)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn retry_last(state: State<'_, AgentManager>, session: SessionKey) -> Result<bool, String> {
    state
        .retry_last(session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn cancel_turn(state: State<'_, AgentManager>, session: SessionKey) -> Result<(), String> {
    state
        .cancel_turn(session)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn answer_permission(
    state: State<'_, AgentManager>,
    session: SessionKey,
    tool_call_id: String,
    option_id: Option<String>,
) -> Result<(), String> {
    state
        .answer_permission(session, &tool_call_id, option_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn set_config_option(
    state: State<'_, AgentManager>,
    session: SessionKey,
    config_id: String,
    value: String,
) -> Result<Vec<ConfigOptionView>, String> {
    state
        .set_config_option(session, config_id, value)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn scoping_draft(
    state: State<'_, AgentManager>,
    session: SessionKey,
) -> Result<Option<String>, String> {
    state
        .scoping_draft(session)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn warm_session(state: State<'_, AgentManager>, session: SessionKey) -> Result<(), String> {
    state
        .warm_session(session)
        .await
        .map_err(|error| error.to_string())
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
        .setup(|app| {
            app.manage(AgentManager::new(app.handle().clone()));
            app.handle().plugin(
                tauri_plugin_log::Builder::new()
                    .level(log_level())
                    .target(Target::new(TargetKind::Stdout))
                    .build(),
            )?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_prefs,
            validate_repo_path,
            open_repo,
            refresh_branch,
            create_plan,
            execute_plan,
            mark_completed,
            abandon_plan,
            cancel_execution,
            select_plan,
            send_prompt,
            scoping_draft,
            retry_last,
            cancel_turn,
            answer_permission,
            set_config_option,
            warm_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
