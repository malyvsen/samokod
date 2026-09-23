// Tauri command edge: thin impure glue over the domain modules.
mod acp;
mod agent;
mod error_hint;
mod permissions;
mod prefs;
mod repo;
mod spend;
mod todos;
mod types;
mod updates;

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::agent::AgentManager;
use crate::prefs::{load_prefs, record_model, record_open, save_prefs, stored_model};
use crate::repo::{RepoInfo, validate_repo};
use crate::types::{Prefs, SessionInfo};

#[tauri::command]
fn get_prefs(app: AppHandle) -> Result<Prefs, String> {
    Ok(load_prefs(&prefs_dir(&app)))
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
) -> Result<SessionInfo, String> {
    let info = validate_repo(&PathBuf::from(&path))?;
    let dir = prefs_dir(&app);
    let stored = stored_model(&load_prefs(&dir), &info.root);
    let session = state
        .open_repo(PathBuf::from(&info.root), info.branch.clone(), stored)
        .await
        .map_err(|error| error.to_string())?;
    let mut prefs = load_prefs(&dir);
    record_open(&mut prefs, &info.root, &info.branch);
    let _ = save_prefs(&dir, &prefs);
    Ok(session)
}

#[tauri::command]
async fn new_chat(app: AppHandle, state: State<'_, AgentManager>) -> Result<SessionInfo, String> {
    let dir = prefs_dir(&app);
    let prefs = load_prefs(&dir);
    let stored = prefs
        .last_repo
        .as_ref()
        .and_then(|repo| stored_model(&prefs, repo));
    state
        .new_chat(stored)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn send_prompt(state: State<'_, AgentManager>, text: String) -> Result<(), String> {
    state
        .send_prompt(text)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn retry_last(state: State<'_, AgentManager>) -> Result<bool, String> {
    state.retry_last().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn cancel_turn(state: State<'_, AgentManager>) -> Result<(), String> {
    state.cancel_turn().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn answer_permission(
    state: State<'_, AgentManager>,
    tool_call_id: String,
    option_id: Option<String>,
) -> Result<(), String> {
    state
        .answer_permission(&tool_call_id, option_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn set_config_option(
    app: AppHandle,
    state: State<'_, AgentManager>,
    id: String,
    value: String,
) -> Result<(), String> {
    state
        .set_config_option(id.clone(), value.clone())
        .await
        .map_err(|error| error.to_string())?;
    if id == "model" {
        let dir = prefs_dir(&app);
        let mut prefs = load_prefs(&dir);
        if let Some(repo) = prefs.last_repo.clone() {
            record_model(&mut prefs, &repo, &value);
            let _ = save_prefs(&dir, &prefs);
        }
    }
    Ok(())
}

fn prefs_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(AgentManager::new(app.handle().clone()));
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_prefs,
            validate_repo_path,
            open_repo,
            new_chat,
            send_prompt,
            retry_last,
            cancel_turn,
            answer_permission,
            set_config_option
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
