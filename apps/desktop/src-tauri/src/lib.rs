// App shell: commands are the impure edge; `agent` owns the lifecycle and
// `acp` owns the wire protocol.
mod acp;
mod agent;
mod types;

use std::path::PathBuf;

use tauri::State;

use crate::agent::AgentManager;

#[tauri::command]
async fn open_session(state: State<'_, AgentManager>, cwd: String) -> Result<String, String> {
    state
        .open_session(PathBuf::from(cwd))
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AgentManager::default())
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
        .invoke_handler(tauri::generate_handler![open_session])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
