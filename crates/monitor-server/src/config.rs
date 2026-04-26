use std::env;

#[derive(Debug, Clone, PartialEq)]
pub enum AuthMode {
    /// Reads (GET) are open; writes (POST, PATCH, DELETE) require a valid token.
    Relaxed,
    /// All endpoints require a valid token.
    Strict,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_address: String,
    pub database_url: String,
    pub auth_mode: AuthMode,
    pub api_tokens: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:3000".to_string(),
            database_url: "sqlite:monitor.db?mode=rwc".to_string(),
            auth_mode: AuthMode::Relaxed,
            api_tokens: Vec::new(),
        }
    }
}

impl Config {
    /// Load configuration from environment variables, falling back to defaults.
    ///
    /// Environment variables:
    /// - `MONITOR_BIND` → bind_address
    /// - `MONITOR_DB` → database_url
    /// - `MONITOR_AUTH_MODE` → "relaxed" or "strict"
    /// - `MONITOR_API_TOKENS` → comma-separated list of bearer tokens
    pub fn from_env() -> Self {
        let mut cfg = Config::default();

        if let Ok(bind) = env::var("MONITOR_BIND") {
            cfg.bind_address = bind;
        }

        if let Ok(db) = env::var("MONITOR_DB") {
            cfg.database_url = db;
        }

        if let Ok(mode) = env::var("MONITOR_AUTH_MODE") {
            cfg.auth_mode = match mode.to_lowercase().as_str() {
                "relaxed" => AuthMode::Relaxed,
                "strict" => AuthMode::Strict,
                other => {
                    eprintln!(
                        "WARNING: Unknown MONITOR_AUTH_MODE '{}', defaulting to Relaxed",
                        other
                    );
                    AuthMode::Relaxed
                }
            };
        }

        if let Ok(tokens) = env::var("MONITOR_API_TOKENS") {
            cfg.api_tokens = tokens
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
        }

        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.bind_address, "127.0.0.1:3000");
        assert_eq!(cfg.database_url, "sqlite:monitor.db?mode=rwc");
        assert_eq!(cfg.auth_mode, AuthMode::Relaxed);
        assert!(cfg.api_tokens.is_empty());
    }
}
