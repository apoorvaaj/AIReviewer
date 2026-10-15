//! Configuration (read-only; the app never writes config).
//!
//! Lookup order for the file: $IB_COACH_CONFIG, then
//! %APPDATA%\ib-feedback-coach\config.toml (Windows) /
//! ~/Library/Application Support/ib-feedback-coach/config.toml (macOS).
//! Missing file => defaults. The API key is read from the environment variable
//! named by `api_key_env` (default ANTHROPIC_API_KEY), or from `api_key` in the
//! file as a fallback. Phase 2 replaces the key with a stateless proxy.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Model used for the per-check feedback calls.
    pub primary_model: String,
    /// Small model used for the "could this be pasted?" second pass.
    pub second_pass_model: String,
    /// Effort for the primary model: low | medium | high | xhigh | max.
    pub effort: String,
    pub second_pass_effort: String,
    pub max_tokens: u32,
    pub api_base_url: String,
    pub api_key_env: String,
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    pub max_concurrency: usize,
    pub request_timeout_secs: u64,
    /// Set true ONLY if your Anthropic organisation has a Zero Data Retention
    /// agreement. It changes the wording on the "What leaves your laptop" screen.
    pub zero_data_retention: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            primary_model: "claude-sonnet-5-5".into(),
            second_pass_model: "claude-haiku-5-5".into(),
            effort: "medium".into(),
            second_pass_effort: "low".into(),
            max_tokens: 8000,
            api_base_url: "https://api.anthropic.com".into(),
            api_key_env: "ANTHROPIC_API_KEY".into(),
            api_key: None,
            max_concurrency: 4,
            request_timeout_secs: 180,
            zero_data_retention: false,
        }
    }
}

const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| format!("config.toml: {e}"))?;
        cfg.check()?;
        Ok(cfg)
    }

    fn check(&self) -> Result<(), String> {
        if !self.api_base_url.starts_with("https://") {
            return Err("config.toml: api_base_url must start with https://".into());
        }
        for e in [&self.effort, &self.second_pass_effort] {
            if !EFFORTS.contains(&e.as_str()) {
                return Err(format!("config.toml: effort {e:?} must be one of {EFFORTS:?}"));
            }
        }
        if self.max_concurrency == 0 || self.max_concurrency > 16 {
            return Err("config.toml: max_concurrency must be 1..=16".into());
        }
        Ok(())
    }

    pub fn default_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("IB_COACH_CONFIG") {
            return Some(PathBuf::from(p));
        }
        if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("ib-feedback-coach").join("config.toml"))
        } else {
            std::env::var_os("HOME").map(|h| {
                PathBuf::from(h)
                    .join("Library/Application Support/ib-feedback-coach/config.toml")
            })
        }
    }

    /// Load from `path` if it exists, else defaults.
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        match path {
            Some(p) if p.exists() => {
                let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
                Self::from_toml(&text)
            }
            _ => Ok(Config::default()),
        }
    }

    pub fn api_key(&self) -> Option<String> {
        std::env::var(&self.api_key_env)
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| self.api_key.clone().filter(|k| !k.trim().is_empty()))
    }

    /// Host name only, e.g. "api.anthropic.com" (shown on the payload screen).
    pub fn destination_host(&self) -> String {
        self.api_base_url
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string()
    }
}
