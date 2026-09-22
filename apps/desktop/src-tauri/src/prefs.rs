// Local preferences: recent repos and the last opened repo. Persisted as
// JSON under the OS app-data directory resolved through the Tauri path API.
// All helpers are pure over an explicit directory so tests never touch the
// real profile.
use std::path::Path;

use crate::types::{Prefs, RecentRepo};

const PREFS_FILE: &str = "samokod-prefs.json";
const MAX_RECENT: usize = 10;

/// Load prefs from a directory. Missing or corrupt files yield defaults.
pub fn load_prefs(dir: &Path) -> Prefs {
    let path = dir.join(PREFS_FILE);
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if text.trim().is_empty() {
        return Prefs::default();
    }
    serde_json::from_str(&text).unwrap_or_default()
}

/// Save prefs to a directory. Creates the directory when needed.
pub fn save_prefs(dir: &Path, prefs: &Prefs) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let path = dir.join(PREFS_FILE);
    let text = serde_json::to_string_pretty(prefs).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| error.to_string())
}

/// Record a repo open: moves it to the front, updates branch, caps the list,
/// and stores it as last repo. Pure over the prefs value.
pub fn record_open(prefs: &mut Prefs, path: &str, branch: &str) {
    prefs.recent.retain(|repo| repo.path != path);
    prefs.recent.insert(
        0,
        RecentRepo {
            path: path.to_string(),
            branch: branch.to_string(),
        },
    );
    prefs.recent.truncate(MAX_RECENT);
    prefs.last_repo = Some(path.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_moves_to_front_and_caps() {
        let mut prefs = Prefs::default();
        for index in 0..12 {
            record_open(&mut prefs, &format!("/repo-{index}"), "main");
        }
        assert_eq!(prefs.recent.len(), MAX_RECENT);
        assert_eq!(prefs.recent[0].path, "/repo-11");
        assert_eq!(prefs.last_repo.as_deref(), Some("/repo-11"));
    }

    #[test]
    fn reopen_updates_branch() {
        let mut prefs = Prefs::default();
        record_open(&mut prefs, "/a", "main");
        record_open(&mut prefs, "/b", "main");
        record_open(&mut prefs, "/a", "dev");
        assert_eq!(prefs.recent[0].path, "/a");
        assert_eq!(prefs.recent[0].branch, "dev");
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut prefs = Prefs::default();
        record_open(&mut prefs, "/repo", "main");
        save_prefs(dir.path(), &prefs).expect("save");
        let loaded = load_prefs(dir.path());
        assert_eq!(loaded, prefs);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(PREFS_FILE), "{oops").expect("write");
        assert_eq!(load_prefs(dir.path()), Prefs::default());
    }
}
