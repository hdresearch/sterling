use std::collections::HashMap;

use serde::Deserialize;

use crate::error::ConfigError;

/// Top-level configuration, loaded from TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    /// Stripe billing integration (optional). When configured, usage is metered
    /// through Stripe and credit balances are checked via Stripe's API.
    #[serde(default)]
    pub stripe: Option<StripeConfig>,
    /// Provider definitions keyed by name (e.g. "openai", "anthropic", "local-vllm").
    pub providers: HashMap<String, ProviderConfig>,
    /// Model definitions keyed by the public model name clients use.
    pub models: HashMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Optional master admin key for /admin endpoints. If unset, admin endpoints are disabled.
    pub admin_api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Main DB URL (billing tables: llm_api_keys, llm_teams, llm_credit_transactions)
    pub url: String,
    /// Log DB URL (high-volume: spend_logs, request_logs). Defaults to `url` if not set.
    #[serde(default)]
    pub log_url: Option<String>,
}

/// Stripe billing integration config.
/// When present, the proxy reports LLM usage as Stripe meter events and
/// uses Stripe's credit balance for auth gating instead of the local ledger.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct StripeConfig {
    /// Stripe secret key (sk_live_xxx or sk_test_xxx).
    /// Required for meter events and balance queries.
    #[serde(default)]
    pub secret_key: Option<String>,
    /// Stripe billing meter event name (e.g. "llm_spend").
    #[serde(default = "default_meter_name")]
    pub meter_event_name: String,
    /// How often to poll Stripe for credit balances, in seconds.
    #[serde(default = "default_balance_poll_secs")]
    pub balance_poll_interval_secs: u64,
    /// How often to flush accumulated meter events to Stripe, in seconds.
    #[serde(default = "default_meter_flush_secs")]
    pub meter_flush_interval_secs: u64,
}

impl DatabaseConfig {
    /// Returns the log DB URL, falling back to the main DB URL.
    pub fn log_url(&self) -> &str {
        self.log_url.as_deref().unwrap_or(&self.url)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    /// Provider type: "openai", "anthropic", or "openai_compatible".
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    /// Environment variable name containing the real API key.
    pub api_key_env: Option<String>,
    /// Base URL override (required for openai_compatible, optional for others).
    pub api_base: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Openai,
    Anthropic,
    /// Any endpoint that speaks the OpenAI chat completions protocol.
    OpenaiCompatible,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    /// Ordered list of provider names to try (fallback chain).
    pub routing: Vec<String>,
    /// The model name to send to the provider (if different from the public name).
    /// e.g. public name "claude-sonnet" → provider model_name "claude-sonnet-4-20250514"
    pub model_name: Option<String>,
}

impl AppConfig {
    pub fn load(path: &str) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadFile {
            path: path.to_string(),
            reason: e.to_string(),
        })?;
        let mut config: Self =
            toml::from_str(&contents).map_err(|e| ConfigError::Parse(e.to_string()))?;
        config.apply_env_overrides();
        config.validate()?;
        Ok(config)
    }

    /// Environment variables override TOML values. This lets ECS inject secrets
    /// without baking them into the config file.
    ///
    /// - `LLM_PROXY_DATABASE_URL`           → database.url
    /// - `LLM_PROXY_LOG_DATABASE_URL`       → database.log_url
    /// - `LLM_PROXY_ADMIN_API_KEY`          → server.admin_api_key
    /// - `LLM_PROXY_HOST`                   → server.host
    /// - `LLM_PROXY_PORT`                   → server.port
    /// - `LLM_PROXY_STRIPE_SECRET_KEY`      → stripe.secret_key
    /// - `LLM_PROXY_STRIPE_METER_EVENT_NAME`→ stripe.meter_event_name
    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("LLM_PROXY_DATABASE_URL") {
            self.database.url = v;
        }
        if let Ok(v) = std::env::var("LLM_PROXY_LOG_DATABASE_URL") {
            self.database.log_url = Some(v);
        }
        if let Ok(v) = std::env::var("LLM_PROXY_ADMIN_API_KEY") {
            self.server.admin_api_key = Some(v);
        }
        if let Ok(v) = std::env::var("LLM_PROXY_HOST") {
            self.server.host = v;
        }
        if let Ok(v) = std::env::var("LLM_PROXY_PORT") {
            if let Ok(port) = v.parse::<u16>() {
                self.server.port = port;
            }
        }
        if let Ok(v) = std::env::var("LLM_PROXY_STRIPE_SECRET_KEY") {
            let stripe = self.stripe.get_or_insert_with(StripeConfig::default);
            stripe.secret_key = Some(v);
        }
        if let Ok(v) = std::env::var("LLM_PROXY_STRIPE_METER_EVENT_NAME") {
            let stripe = self.stripe.get_or_insert_with(StripeConfig::default);
            stripe.meter_event_name = v;
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        for (model_name, model_cfg) in &self.models {
            for provider_name in &model_cfg.routing {
                if !self.providers.contains_key(provider_name) {
                    return Err(ConfigError::Validation(format!(
                        "model '{model_name}' references undefined provider '{provider_name}'"
                    )));
                }
            }
        }

        for (name, provider) in &self.providers {
            if provider.provider_type == ProviderType::OpenaiCompatible
                && provider.api_base.is_none()
            {
                return Err(ConfigError::Validation(format!(
                    "provider '{name}' of type openai_compatible requires api_base"
                )));
            }
        }

        Ok(())
    }
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8090
}

fn default_meter_name() -> String {
    "llm_spend".to_string()
}

fn default_balance_poll_secs() -> u64 {
    30
}

fn default_meter_flush_secs() -> u64 {
    5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
            [server]
            admin_api_key = "test"

            [database]
            url = "postgres://localhost/test"

            [providers.openai]
            type = "openai"
            api_key_env = "OPENAI_API_KEY"

            [models.gpt-4o]
            routing = ["openai"]
        "#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.server.port, 8090); // default
        assert_eq!(config.server.host, "0.0.0.0"); // default
        assert!(config.providers.contains_key("openai"));
        assert!(config.models.contains_key("gpt-4o"));
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
            [server]
            host = "127.0.0.1"
            port = 9090
            admin_api_key = "secret"

            [database]
            url = "postgres://localhost/test"

            [providers.openai]
            type = "openai"
            api_key_env = "OPENAI_API_KEY"

            [providers.anthropic]
            type = "anthropic"
            api_key_env = "ANTHROPIC_API_KEY"

            [providers.local]
            type = "openai_compatible"
            api_base = "http://localhost:8000/v1"

            [models.gpt-4o]
            routing = ["openai"]

            [models.claude-sonnet]
            routing = ["anthropic"]
            model_name = "claude-sonnet-4-20250514"

            [models.local-llama]
            routing = ["local"]
            model_name = "llama-3"
        "#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        config.validate().unwrap();
        assert_eq!(config.server.port, 9090);
        assert_eq!(config.providers.len(), 3);
        assert_eq!(config.models.len(), 3);
        assert_eq!(
            config.models["claude-sonnet"].model_name.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
    }

    #[test]
    fn validate_catches_missing_provider() {
        let toml = r#"
            [server]
            [database]
            url = "postgres://localhost/test"

            [providers.openai]
            type = "openai"

            [models.gpt-4o]
            routing = ["nonexistent"]
        "#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("nonexistent"));
    }

    #[test]
    fn validate_catches_missing_api_base_for_compatible() {
        let toml = r#"
            [server]
            [database]
            url = "postgres://localhost/test"

            [providers.local]
            type = "openai_compatible"

            [models.test]
            routing = ["local"]
        "#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("api_base"));
    }

    #[test]
    fn parse_stripe_config() {
        let toml = r#"
            [server]
            [database]
            url = "postgres://localhost/test"

            [stripe]
            secret_key = "sk_test_xxx"
            meter_event_name = "my_meter"
            balance_poll_interval_secs = 60
            meter_flush_interval_secs = 10

            [providers.openai]
            type = "openai"

            [models.gpt-4o]
            routing = ["openai"]
        "#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        let stripe = config.stripe.unwrap();
        assert_eq!(stripe.secret_key.unwrap(), "sk_test_xxx");
        assert_eq!(stripe.meter_event_name, "my_meter");
        assert_eq!(stripe.balance_poll_interval_secs, 60);
        assert_eq!(stripe.meter_flush_interval_secs, 10);
    }

    #[test]
    fn stripe_config_defaults_when_omitted() {
        let toml = r#"
            [server]
            [database]
            url = "postgres://localhost/test"

            [providers.openai]
            type = "openai"

            [models.gpt-4o]
            routing = ["openai"]
        "#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        assert!(config.stripe.is_none());
    }

    #[test]
    fn stripe_config_uses_defaults_for_optional_fields() {
        let toml = r#"
            [server]
            [database]
            url = "postgres://localhost/test"

            [stripe]
            secret_key = "sk_test_xxx"

            [providers.openai]
            type = "openai"

            [models.gpt-4o]
            routing = ["openai"]
        "#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        let stripe = config.stripe.unwrap();
        assert_eq!(stripe.meter_event_name, "llm_spend");
        assert_eq!(stripe.balance_poll_interval_secs, 30);
        assert_eq!(stripe.meter_flush_interval_secs, 5);
    }
}
