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
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub name: String,
    /// Tauri accelerator, e.g. `CommandOrControl+Alt+KeyT`. Empty means no hotkey.
    pub hotkey: String,
    /// Instruction for the model. `{target}` and `{secondary}` are replaced with languages.
    pub prompt: String,
    pub enabled: bool,
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
            anthropic: ProviderConfig { model: "claude-opus-5-5".into(), base_url: String::new() },
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

pub fn load(path: &Path) -> Settings {
    fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
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
