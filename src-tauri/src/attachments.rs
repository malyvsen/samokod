// Plan image attachments: pasted or dropped images saved under the plan's
// own `attachments/` directory. Detection is magic bytes only, never the
// client hint; the client filename is ignored entirely.
use std::path::{Path, PathBuf};

use crate::agent::AgentManager;
use crate::types::{AgentError, SessionKey, SessionRole};

pub const ATTACHMENTS_DIR: &str = "attachments";
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILES: usize = 20;

impl AgentManager {
    /// Save one base64 image for a session's plan, materializing a pending
    /// scoping directory first. The pending entry stays until `send_prompt`
    /// clears it. Returns the absolute file path.
    pub fn save_attachment(
        &self,
        session: SessionKey,
        data_base64: String,
    ) -> Result<String, AgentError> {
        let repo_root = self.current_repo().ok_or_else(|| AgentError::NoSession {
            raw: "open a repository first".to_string(),
        })?;
        let materialize =
            session.role == SessionRole::Scoping && self.is_pending_scoping(&session.plan);
        let path = save_plan_attachment(&repo_root, &session.plan, materialize, &data_base64)?;
        Ok(path.to_string_lossy().to_string())
    }
}

/// Save one base64 image for a plan name, locating its directory with the
/// shared plan search. A missing directory materializes only for a pending
/// scoping session, otherwise the plan reports gone. Pure except directory
/// probes and the write.
pub(crate) fn save_plan_attachment(
    repo_root: &Path,
    plan_name: &str,
    materialize_pending: bool,
    data_base64: &str,
) -> Result<PathBuf, AgentError> {
    let mut plan_dir = crate::plans::locate(repo_root, plan_name);
    if plan_dir.is_none() && materialize_pending {
        crate::plans::materialize_scoping(repo_root, plan_name)?;
        plan_dir = crate::plans::locate(repo_root, plan_name);
    }
    let Some(plan_dir) = plan_dir else {
        return Err(AgentError::NoSession {
            raw: "plan is gone".to_string(),
        });
    };
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data_base64)
        .map_err(|error| AgentError::RequestFailed {
            raw: format!("invalid image data: {error}"),
        })?;
    save_bytes(&plan_dir, &bytes).map_err(|raw| AgentError::RequestFailed { raw })
}

/// Detect the image type by magic bytes, never the client hint. Returns the
/// file extension. Pure.
pub fn detect_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Some("png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(&[0x47, 0x49, 0x46]) {
        return Some("gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

/// Save raw image bytes into the plan's `attachments/` directory as
/// `image-<unix_ms>-<n>.<ext>`, bumping `n` until free so rapid pastes
/// never collide. Rejects oversize, unknown, and over-count uploads.
pub fn save_bytes(plan_dir: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "image too large: {} bytes, max {MAX_BYTES}",
            bytes.len()
        ));
    }
    let Some(ext) = detect_extension(bytes) else {
        return Err("unsupported image type".to_string());
    };
    let dir = plan_dir.join(ATTACHMENTS_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let count = std::fs::read_dir(&dir)
        .map_err(|error| format!("cannot list {}: {error}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .count();
    if count >= MAX_FILES {
        return Err(format!("too many attachments, max {MAX_FILES}"));
    }
    let ms = crate::plans::unix_ms(std::time::SystemTime::now()).unwrap_or(0);
    let mut counter = 0;
    loop {
        let path = dir.join(format!("image-{ms}-{counter}.{ext}"));
        if !path.exists() {
            std::fs::write(&path, bytes)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            return Ok(path);
        }
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    fn png_bytes() -> Vec<u8> {
        vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00]
    }

    fn jpeg_bytes() -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00]
    }

    fn gif_bytes() -> Vec<u8> {
        b"GIF89a\x01\x00".to_vec()
    }

    fn webp_bytes() -> Vec<u8> {
        [b"RIFF".as_slice(), &[0x00, 0x00, 0x00, 0x00], b"WEBP"].concat()
    }

    #[test]
    fn detects_every_supported_format() {
        assert_eq!(detect_extension(&png_bytes()), Some("png"));
        assert_eq!(detect_extension(&jpeg_bytes()), Some("jpg"));
        assert_eq!(detect_extension(&gif_bytes()), Some("gif"));
        assert_eq!(detect_extension(&webp_bytes()), Some("webp"));
    }

    #[test]
    fn rejects_text_bytes() {
        assert_eq!(detect_extension(b"hello world"), None);
        assert_eq!(detect_extension(b""), None);
    }

    #[test]
    fn rejects_oversize_upload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let big = vec![0u8; MAX_BYTES + 1];
        assert!(save_bytes(dir.path(), &big).is_err());
    }

    #[test]
    fn rejects_unknown_mime() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(save_bytes(dir.path(), b"not an image").is_err());
    }

    #[test]
    fn gates_max_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = dir.path().join("plan");
        std::fs::create_dir_all(plan.join(ATTACHMENTS_DIR)).expect("mkdir");
        for index in 0..MAX_FILES {
            std::fs::write(
                plan.join(ATTACHMENTS_DIR).join(format!("old-{index}.png")),
                png_bytes(),
            )
            .expect("seed");
        }
        assert!(save_bytes(&plan, &png_bytes()).is_err());
    }

    #[test]
    fn bumps_counter_until_free() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = save_bytes(dir.path(), &png_bytes()).expect("first");
        let second = save_bytes(dir.path(), &png_bytes()).expect("second");
        assert_ne!(first, second);
        assert!(first.is_file());
        assert!(second.is_file());
    }

    #[test]
    fn pending_scoping_materializes() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "test-plan";
        let encoded = base64::engine::general_purpose::STANDARD.encode(png_bytes());
        let path = save_plan_attachment(dir.path(), name, true, &encoded).expect("saved");
        assert!(path.is_file());
        assert!(
            crate::plans::PlanRef {
                name: name.to_string(),
                phase: crate::plans::Phase::Scoping,
            }
            .path(dir.path())
            .is_dir()
        );
    }

    #[test]
    fn missing_plan_reports_gone() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let encoded = base64::engine::general_purpose::STANDARD.encode(png_bytes());
        let missing = save_plan_attachment(dir.path(), "gone", false, &encoded);
        assert!(matches!(missing, Err(AgentError::NoSession { .. })));
    }

    #[test]
    fn invalid_base64_fails_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "test-plan";
        crate::plans::materialize_scoping(dir.path(), name).expect("materialize");
        let failed = save_plan_attachment(dir.path(), name, false, "!!!not-base64!!!");
        assert!(matches!(failed, Err(AgentError::RequestFailed { .. })));
    }
}
