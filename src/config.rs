use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 9000;
pub const DEFAULT_UPSTREAM_BASE_URL: &str = "https://api.deepseek.com";
pub const DEFAULT_UPSTREAM_MODEL: &str = "deepseek-v4-pro";
pub const DEFAULT_THINKING: &str = "enabled";
pub const DEFAULT_REASONING_EFFORT: &str = "max";
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_MAX_REQUEST_BODY_BYTES: usize = 20 * 1024 * 1024;
pub const DEFAULT_TUNNEL_PROVIDER: &str = "cloudflared";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub upstream_base_url: String,
    pub upstream_model: String,
    pub thinking: String,
    pub reasoning_effort: String,
    pub request_timeout_secs: u64,
    pub max_request_body_bytes: usize,
    pub reasoning_content_path: PathBuf,
    pub display_reasoning: bool,
    pub collapsible_reasoning: bool,
    pub verbose: bool,
    pub trace_dir: Option<PathBuf>,
    pub tunnel_enabled: bool,
    pub tunnel_provider: String,
    pub cloudflared_bin: Option<PathBuf>,
    pub sync_cursor_openai_base_url: bool,
    pub cursor_state_db_path: PathBuf,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            upstream_base_url: DEFAULT_UPSTREAM_BASE_URL.to_string(),
            upstream_model: DEFAULT_UPSTREAM_MODEL.to_string(),
            thinking: DEFAULT_THINKING.to_string(),
            reasoning_effort: DEFAULT_REASONING_EFFORT.to_string(),
            request_timeout_secs: DEFAULT_REQUEST_TIMEOUT_SECS,
            max_request_body_bytes: DEFAULT_MAX_REQUEST_BODY_BYTES,
            reasoning_content_path: default_reasoning_cache_path(),
            display_reasoning: true,
            collapsible_reasoning: true,
            verbose: false,
            trace_dir: None,
            tunnel_enabled: false,
            tunnel_provider: DEFAULT_TUNNEL_PROVIDER.to_string(),
            cloudflared_bin: None,
            sync_cursor_openai_base_url: true,
            cursor_state_db_path: crate::cursor::state::default_cursor_state_db_path(),
        }
    }
}

impl AppConfig {
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn from_yaml_str(value: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(value)
    }

    pub fn load_or_create(path: &Path) -> Result<Self, std::io::Error> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let default = Self::default();
            std::fs::write(path, serde_yaml::to_string(&default).unwrap())?;
            return Ok(default);
        }
        let content = std::fs::read_to_string(path)?;
        Ok(Self::from_yaml_str(&content).unwrap_or_default())
    }

    pub fn config_path_or_default(path: Option<PathBuf>) -> PathBuf {
        path.unwrap_or_else(default_config_path)
    }
}

pub fn default_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".deepseek-cursor-proxy-rust")
}

pub fn default_config_path() -> PathBuf {
    default_config_dir().join("config.yaml")
}

pub fn default_reasoning_cache_path() -> PathBuf {
    default_config_dir().join("reasoning_content.sqlite3")
}

pub fn is_yaml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("yaml" | "yml")
    )
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, default_config_path, is_yaml_file};
    use std::path::Path;

    #[test]
    fn default_config_has_expected_local_bind_address() {
        let config = AppConfig::default();
        assert_eq!(config.bind_address(), "127.0.0.1:9000");
    }

    #[test]
    fn default_config_path_uses_yaml_extension() {
        assert!(is_yaml_file(&default_config_path()));
        assert!(is_yaml_file(Path::new("config.yml")));
        assert!(!is_yaml_file(Path::new("config.json")));
    }

    #[test]
    fn load_or_create_writes_default_file() {
        let path = std::env::temp_dir()
            .join("deepseek-cursor-proxy-rust-tests")
            .join("config-load-or-create.yaml");
        let _ = std::fs::remove_file(&path);
        let config = AppConfig::load_or_create(&path).unwrap();
        assert_eq!(config.bind_address(), "127.0.0.1:9000");
        assert!(path.exists());
    }
}
