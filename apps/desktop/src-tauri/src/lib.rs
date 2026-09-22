// Tauri command edge: thin impure glue over the domain modules.
mod acp;
mod agent;
mod prefs;
mod repo;
mod types;

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::agent::AgentManager;
use crate::prefs::{load_prefs, record_open, save_prefs};
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
    let session = state
        .open_repo(PathBuf::from(&info.root), info.branch.clone())
        .await
        .map_err(|error| error.to_string())?;
    let dir = prefs_dir(&app);
    let mut prefs = load_prefs(&dir);
    record_open(&mut prefs, &info.root, &info.branch);
    let _ = save_prefs(&dir, &prefs);
    Ok(session)
}

fn prefs_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AgentManager::default())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
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
            open_repo
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
