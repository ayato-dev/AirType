//! User settings (a JSON file in the app config dir) and API keys (OS keychain).

use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const KEYRING_SERVICE: &str = "dev.ayato.airtype";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Openai,
}

impl Provider {
    fn keyring_user(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Openai => "openai",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    pub model: String,
    /// Empty means the provider's official endpoint.
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Action {
    pub id: String,
    pub name: String,
    /// Tauri accelerator, e.g. `CommandOrControl+Alt+KeyT`. Empty means no hotkey.
    pub hotkey: String,
    /// Instruction for the model. `{target}` and `{secondary}` are replaced with languages.
    pub prompt: String,
    pub enabled: bool,
}

impl Default for Action {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            hotkey: String::new(),
            prompt: String::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub provider: Provider,
    pub anthropic: ProviderConfig,
    pub openai: ProviderConfig,
    /// Claude `output_config.effort`; empty means the model default.
    pub effort: String,
    pub target_lang: String,
    pub secondary_lang: String,
    pub show_hud: bool,
    pub paused: bool,
    pub actions: Vec<Action>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            anthropic: ProviderConfig { model: "claude-haiku-5-5".into(), base_url: String::new() },
            openai: ProviderConfig::default(),
            effort: "low".into(),
            target_lang: "English".into(),
            secondary_lang: "Русский".into(),
            show_hud: true,
            paused: false,
            actions: default_actions(),
        }
    }
}

impl Settings {
    /// A local OpenAI-compatible server (Ollama, LM Studio) works without a key.
    pub fn requires_key(&self) -> bool {
        self.provider == Provider::Anthropic || self.openai.base_url.trim().is_empty()
    }
}

fn default_actions() -> Vec<Action> {
    vec![
        Action {
            id: "translate".into(),
            name: "Перевести".into(),
            hotkey: "CommandOrControl+Alt+KeyT".into(),
            prompt: "Translate the text into {target}. If it is already in {target}, translate it into {secondary} instead. Keep the meaning, tone, slang, emoji and formatting (line breaks, lists, markdown, @mentions, links, code).".into(),
            enabled: true,
        },
        Action {
            id: "fix".into(),
            name: "Исправить".into(),
            hotkey: "CommandOrControl+Alt+KeyR".into(),
            prompt: "Fix spelling, grammar and punctuation. Keep the original language, meaning, tone and formatting. Change as little as possible.".into(),
            enabled: true,
        },
        Action {
            id: "improve".into(),
            name: "Улучшить".into(),
            hotkey: "CommandOrControl+Alt+KeyE".into(),
            prompt: "Rewrite the text so it reads clear, natural and friendly, like a native speaker wrote it. Keep the original language, meaning and formatting, and keep it about the same length.".into(),
            enabled: true,
        },
    ]
}

/// What came of reading the settings file.
pub struct Loaded {
    pub settings: Settings,
    /// Tell the user: the file couldn't be read and what happened to it.
    pub notice: Option<String>,
    /// The broken file is still in place; saving would destroy it.
    pub read_only: bool,
}

/// Reads the settings. A file that can't be read is moved aside rather than
/// silently replaced, so the user's own actions and prompts are never lost.
pub fn load(path: &Path) -> Loaded {
    let error = match fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(settings) => return Loaded { settings, notice: None, read_only: false },
            Err(e) => e.to_string(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Loaded { settings: Settings::default(), notice: None, read_only: false };
        }
        Err(e) => e.to_string(),
    };
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let backup = path.with_extension(format!("json.broken-{stamp}"));
    let (notice, read_only) = match fs::rename(path, &backup) {
        Ok(()) => (
            format!(
                "Файл настроек не прочитался ({error}). Отложил его в {} и включил настройки по умолчанию.",
                backup.display()
            ),
            false,
        ),
        Err(_) => (
            format!(
                "Файл настроек не прочитался ({error}), и отложить его не вышло. Пока не сохраняю, чтобы не затереть: {}",
                path.display()
            ),
            true,
        ),
    };
    Loaded { settings: Settings::default(), notice: Some(notice), read_only }
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    // Write then rename so a crash never leaves a half-written file.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn key_entry(provider: Provider) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, provider.keyring_user()).map_err(|e| e.to_string())
}

pub fn api_key(provider: Provider) -> Option<String> {
    key_entry(provider).ok()?.get_password().ok().filter(|key| !key.trim().is_empty())
}

pub fn set_api_key(provider: Provider, key: &str) -> Result<(), String> {
    let entry = key_entry(provider)?;
    let key = key.trim();
    if key.is_empty() {
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    entry.set_password(key).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("airtype-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_is_defaults_without_notice() {
        let loaded = load(&temp_dir("missing").join("settings.json"));
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.notice.is_none() && !loaded.read_only);
    }

    #[test]
    fn broken_file_is_moved_aside_not_overwritten() {
        let dir = temp_dir("broken");
        let path = dir.join("settings.json");
        fs::write(&path, "{ \"actions\": [ oops").unwrap();
        let loaded = load(&path);
        assert!(loaded.notice.is_some() && !loaded.read_only);
        assert!(!path.exists());
        let backups: Vec<_> = fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), "{ \"actions\": [ oops");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_fields_keep_the_rest() {
        let dir = temp_dir("partial");
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"targetLang":"Deutsch","actions":[{"id":"x","name":"Мой","prompt":"p"}]}"#)
            .unwrap();
        let loaded = load(&path);
        assert!(loaded.notice.is_none());
        assert_eq!(loaded.settings.target_lang, "Deutsch");
        assert_eq!(loaded.settings.actions.len(), 1);
        assert!(loaded.settings.actions[0].enabled);
        assert!(loaded.settings.actions[0].hotkey.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_round_trips() {
        let dir = temp_dir("save");
        let path = dir.join("settings.json");
        let settings = Settings { effort: "medium".into(), ..Settings::default() };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).settings, settings);
        let _ = fs::remove_dir_all(dir);
    }
}
