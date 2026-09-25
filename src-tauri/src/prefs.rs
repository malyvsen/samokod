// Local state: recent repo paths. Persisted as JSON under the OS app-data
// directory resolved through the Tauri path API. Record helpers stay pure
// over an explicit directory so tests never touch the real profile.
use std::path::Path;

use crate::types::{Prefs, RecentRepo};

const STATE_FILE: &str = "state.json";
const LEGACY_FILE: &str = "samokod-prefs.json";
const MAX_RECENT: usize = 10;

/// Load state from a directory. Prefers `state.json`, falls back to the
/// legacy `samokod-prefs.json` once. Unknown old keys are ignored on parse
/// and shed on save.
pub fn load_prefs(dir: &Path) -> Prefs {
    let path = dir.join(STATE_FILE);
    if let Some(prefs) = read_prefs(&path) {
        return prefs;
    }
    if !path.exists()
        && let Some(prefs) = read_prefs(&dir.join(LEGACY_FILE))
    {
        return prefs;
    }
    Prefs::default()
}

fn read_prefs(path: &Path) -> Option<Prefs> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("failed to read {}: {error}", path.display());
            return Some(Prefs::default());
        }
    };
    if text.trim().is_empty() {
        return Some(Prefs::default());
    }
    match serde_json::from_str(&text) {
        Ok(prefs) => Some(prefs),
        Err(error) => {
            log::warn!("failed to parse {}: {error}", path.display());
            Some(Prefs::default())
        }
    }
}

/// Save state to a directory, then remove the legacy file best-effort.
pub fn save_prefs(dir: &Path, prefs: &Prefs) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let text = serde_json::to_string_pretty(prefs).map_err(|error| error.to_string())?;
    std::fs::write(dir.join(STATE_FILE), text).map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::remove_file(dir.join(LEGACY_FILE))
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("failed to remove legacy prefs file: {error}");
    }
    Ok(())
}

/// Record a repo open: moves it to the front and caps the list. Pure over
/// the prefs value.
pub fn record_open(prefs: &mut Prefs, path: &str) {
    prefs.recent.retain(|repo| repo.path != path);
    prefs.recent.insert(
        0,
        RecentRepo {
            path: path.to_string(),
        },
    );
    prefs.recent.truncate(MAX_RECENT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_moves_to_front_and_caps() {
        let mut prefs = Prefs::default();
        for index in 0..12 {
            record_open(&mut prefs, &format!("/repo-{index}"));
        }
        assert_eq!(prefs.recent.len(), MAX_RECENT);
        assert_eq!(prefs.recent[0].path, "/repo-11");
    }

    #[test]
    fn reopen_moves_to_front() {
        let mut prefs = Prefs::default();
        record_open(&mut prefs, "/a");
        record_open(&mut prefs, "/b");
        record_open(&mut prefs, "/a");
        assert_eq!(prefs.recent[0].path, "/a");
        assert_eq!(prefs.recent.len(), 2);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut prefs = Prefs::default();
        record_open(&mut prefs, "/repo");
        save_prefs(dir.path(), &prefs).expect("save");
        assert_eq!(load_prefs(dir.path()), prefs);
    }

    #[test]
    fn legacy_fallback_and_shed_old_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(LEGACY_FILE),
            r#"{"recent":[{"path":"/repo","branch":"main"}],"models":{"/repo":"m"}}"#,
        )
        .expect("write");
        let loaded = load_prefs(dir.path());
        assert_eq!(loaded.recent.len(), 1);
        assert_eq!(loaded.recent[0].path, "/repo");
        save_prefs(dir.path(), &loaded).expect("save");
        assert!(dir.path().join(STATE_FILE).exists());
        let text = std::fs::read_to_string(dir.path().join(STATE_FILE)).expect("read");
        assert!(!text.contains("branch"));
        assert!(!text.contains("models"));
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(STATE_FILE), "{oops").expect("write");
        assert_eq!(load_prefs(dir.path()), Prefs::default());
    }
}
