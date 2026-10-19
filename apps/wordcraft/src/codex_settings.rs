//! Only UI preferences and conversation references. No content, account data or protocol payloads.
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;
use wordcraft_ui_egui::assistant::State;
const LIMIT: u64 = 32 * 1024;
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
pub fn value(state: &State) -> Value {
    json!({"version":1,"open":state.open,"binary":state.binary,"recent":state.recent.iter().filter(|id| valid_id(id)).take(20).collect::<Vec<_>>()})
}
fn safe_root(root: &Path) -> bool {
    if !root.is_absolute() || std::fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_symlink()) {
        return false;
    }
    let Some(actual) = root.ancestors().find_map(|p| std::fs::canonicalize(p).ok()) else { return false };
    let globals = [
        std::env::var_os("CODEX_HOME").map(std::path::PathBuf::from),
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".codex")),
        std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join(".codex")),
    ];
    !globals.into_iter().flatten().filter_map(|p| std::fs::canonicalize(p).ok()).any(|p| actual.starts_with(p))
}
pub fn load(root: &Path, state: &mut State) {
    if !safe_root(root) {
        return;
    }
    let path = root.join("assistant.json");
    let Ok(meta) = std::fs::symlink_metadata(&path) else { return };
    if !meta.is_file() || meta.len() > LIMIT {
        return;
    }
    let Ok(file) = std::fs::File::open(path) else { return };
    let mut bytes = Vec::new();
    if file.take(LIMIT + 1).read_to_end(&mut bytes).is_err() || bytes.len() > LIMIT as usize {
        return;
    }
    let Ok(data) = serde_json::from_slice::<Value>(&bytes) else { return };
    if data.get("version").and_then(Value::as_u64) != Some(1) {
        return;
    }
    state.open = data.get("open").and_then(Value::as_bool).unwrap_or(false);
    if let Some(binary) = data.get("binary").and_then(Value::as_str).filter(|p| p.len() <= 4096 && (p.is_empty() || Path::new(p).is_absolute())) {
        state.binary = binary.into();
    }
    if let Some(recent) = data.get("recent").and_then(Value::as_array) {
        state.recent = recent.iter().filter_map(Value::as_str).filter(|id| valid_id(id)).take(20).map(str::to_owned).collect();
    }
}
pub fn save(root: &Path, data: &Value) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec(data).map_err(|_| "Could not save assistant preferences.")?;
    if bytes.len() > LIMIT as usize {
        return Err("Assistant preferences exceeded the size limit.");
    }
    if !safe_root(root) {
        return Err("Assistant preferences require a separate WordCraft folder.");
    }
    std::fs::create_dir_all(root).map_err(|_| "Could not create the WordCraft assistant folder.")?;
    if !safe_root(root) {
        return Err("Assistant preferences require a separate WordCraft folder.");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).map_err(|_| "Could not protect the WordCraft assistant folder.")?;
    }
    let path = root.join("assistant.json");
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Assistant preferences cannot be a symbolic link.");
    }
    let temporary = root.join(format!("assistant-{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|_| "Could not prepare assistant preferences.")?;
    let result = file.write_all(&bytes).and_then(|()| file.sync_all()).and_then(|()| std::fs::rename(&temporary, &path));
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result.map_err(|_| "Could not save assistant preferences.")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_oversized_and_linked_preferences() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("assistant.json"), vec![b' '; LIMIT as usize + 1]).unwrap();
        let mut state = State::default();
        load(temp.path(), &mut state);
        assert!(state.recent.is_empty());
        #[cfg(unix)]
        {
            let alias = temp.path().join("alias");
            std::os::unix::fs::symlink(temp.path(), &alias).unwrap();
            assert!(save(&alias, &value(&state)).is_err());
            std::fs::remove_file(temp.path().join("assistant.json")).unwrap();
            std::os::unix::fs::symlink(temp.path().join("untouched"), temp.path().join("assistant.json")).unwrap();
            assert!(save(temp.path(), &value(&state)).is_err());
            assert!(!temp.path().join("untouched").exists());
        }
    }
    #[test]
    fn references_roundtrip_without_content_credentials_or_grants() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State {
            open: true,
            binary: temp.path().join("codex").to_string_lossy().into_owned(),
            composer: "SECRET_PROMPT".into(),
            login_url: Some("SECRET_AUTH_URL".into()),
            login_id: Some("SECRET_LOGIN_ID".into()),
            ..Default::default()
        };
        state.remember_thread("valid-thread".into());
        state.push_message("You", "SECRET_DOCUMENT".into());
        state.sharing = "SECRET_SCOPE".into();
        state.signed_in = Some(true);
        save(temp.path(), &value(&state)).unwrap();
        let bytes = std::fs::read(temp.path().join("assistant.json")).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("SECRET"));
        let mut restored = State::default();
        load(temp.path(), &mut restored);
        assert_eq!(restored.recent, vec!["valid-thread"]);
        assert!(restored.open);
        assert_eq!(restored.binary, state.binary);
        assert!(restored.messages.is_empty() && restored.composer.is_empty() && restored.signed_in.is_none() && restored.thread.is_none());
        restored.recent.clear();
        save(temp.path(), &value(&restored)).unwrap();
        let mut cleared = State::default();
        load(temp.path(), &mut cleared);
        assert!(cleared.recent.is_empty());
    }
}
