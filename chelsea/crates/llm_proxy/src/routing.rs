//! Model routing: resolves a model name to a provider + credential + model_name.

use std::collections::HashMap;

use crate::config::AppConfig;
use crate::error::ConfigError;
use crate::providers::{ProviderCredential, ProviderImpl};

/// A resolved route.
pub struct ResolvedRoute {
    pub provider: ProviderImpl,
    pub credential: ProviderCredential,
    pub model_name: String,
    pub provider_name: String,
}

pub struct ModelRouter {
    routes: HashMap<String, Vec<RouteEntry>>,
}

struct RouteEntry {
    provider_name: String,
    provider: ProviderImpl,
    api_key: Option<String>,
    api_base: String,
    model_name: String,
}

impl ModelRouter {
    pub fn from_config(config: &AppConfig) -> Result<Self, ConfigError> {
        let mut routes = HashMap::new();

        for (public_name, model_cfg) in &config.models {
            let mut entries = Vec::new();

            let model_name = model_cfg
                .model_name
                .clone()
                .unwrap_or_else(|| public_name.clone());

            for provider_name in &model_cfg.routing {
                let provider_cfg = config.providers.get(provider_name).ok_or_else(|| {
                    ConfigError::Validation(format!("provider '{provider_name}' not found"))
                })?;

                let provider = ProviderImpl::from_type(&provider_cfg.provider_type);

                let api_key = provider_cfg
                    .api_key_env
                    .as_ref()
                    .and_then(|env_name| std::env::var(env_name).ok());

                let api_base = match provider_cfg.api_base.clone() {
                    Some(base) => base,
                    None => match provider_cfg.provider_type {
                        crate::config::ProviderType::Openai => {
                            "https://api.openai.com/v1".to_string()
                        }
                        crate::config::ProviderType::Anthropic => {
                            "https://api.anthropic.com".to_string()
                        }
                        crate::config::ProviderType::OpenaiCompatible => {
                            return Err(ConfigError::Validation(format!(
                                "provider '{provider_name}' has type openai_compatible but no api_base"
                            )));
                        }
                    },
                };

                entries.push(RouteEntry {
                    provider_name: provider_name.clone(),
                    provider,
                    api_key,
                    api_base,
                    model_name: model_name.clone(),
                });
            }

            routes.insert(public_name.clone(), entries);
        }

        Ok(Self { routes })
    }

    pub fn resolve(&self, model: &str) -> Option<Vec<ResolvedRoute>> {
        self.routes.get(model).map(|entries| {
            entries
                .iter()
                .map(|e| ResolvedRoute {
                    provider: e.provider,
                    credential: ProviderCredential {
                        api_key: e.api_key.clone(),
                        api_base: e.api_base.clone(),
                    },
                    model_name: e.model_name.clone(),
                    provider_name: e.provider_name.clone(),
                })
                .collect()
        })
    }

    pub fn available_models(&self) -> Vec<String> {
        let mut models: Vec<String> = self.routes.keys().cloned().collect();
        models.sort();
        models
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use std::collections::HashMap;

    fn test_config() -> AppConfig {
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                provider_type: ProviderType::Openai,
                api_key_env: Some("OPENAI_API_KEY".to_string()),
                api_base: None,
            },
        );
        providers.insert(
            "anthropic".to_string(),
            ProviderConfig {
                provider_type: ProviderType::Anthropic,
                api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
                api_base: None,
            },
        );
        providers.insert(
            "openai_backup".to_string(),
            ProviderConfig {
                provider_type: ProviderType::OpenaiCompatible,
                api_key_env: None,
                api_base: Some("http://localhost:8000/v1".to_string()),
            },
        );

        let mut models = HashMap::new();
        models.insert(
            "gpt-4o".to_string(),
            ModelConfig {
                routing: vec!["openai".to_string()],
                model_name: None,
            },
        );
        models.insert(
            "claude-sonnet".to_string(),
            ModelConfig {
                routing: vec!["anthropic".to_string()],
                model_name: Some("claude-sonnet-4-20250514".to_string()),
            },
        );
        models.insert(
            "gpt-4o-resilient".to_string(),
            ModelConfig {
                routing: vec!["openai".to_string(), "openai_backup".to_string()],
                model_name: Some("gpt-4o".to_string()),
            },
        );

        AppConfig {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8090,
                admin_api_key: None,
            },
            database: DatabaseConfig {
                url: "postgres://localhost/test".to_string(),
                log_url: None,
            },
            stripe: None,
            providers,
            models,
        }
    }

    #[test]
    fn resolve_simple_model() {
        let router = ModelRouter::from_config(&test_config()).unwrap();
        let routes = router.resolve("gpt-4o").unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].model_name, "gpt-4o");
        assert_eq!(routes[0].provider_name, "openai");
    }

    #[test]
    fn resolve_with_model_name_override() {
        let router = ModelRouter::from_config(&test_config()).unwrap();
        let routes = router.resolve("claude-sonnet").unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].model_name, "claude-sonnet-4-20250514");
        assert_eq!(routes[0].provider_name, "anthropic");
    }

    #[test]
    fn resolve_fallback_chain() {
        let router = ModelRouter::from_config(&test_config()).unwrap();
        let routes = router.resolve("gpt-4o-resilient").unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].provider_name, "openai");
        assert_eq!(routes[1].provider_name, "openai_backup");
        // Both should use the same model_name
        assert_eq!(routes[0].model_name, "gpt-4o");
        assert_eq!(routes[1].model_name, "gpt-4o");
    }

    #[test]
    fn resolve_unknown_model_returns_none() {
        let router = ModelRouter::from_config(&test_config()).unwrap();
        assert!(router.resolve("nonexistent-model").is_none());
    }

    #[test]
    fn available_models_sorted() {
        let router = ModelRouter::from_config(&test_config()).unwrap();
        let models = router.available_models();
        assert_eq!(models, vec!["claude-sonnet", "gpt-4o", "gpt-4o-resilient"]);
    }

    #[test]
    fn credential_base_urls_resolved() {
        let router = ModelRouter::from_config(&test_config()).unwrap();

        let openai_routes = router.resolve("gpt-4o").unwrap();
        assert_eq!(
            openai_routes[0].credential.api_base,
            "https://api.openai.com/v1"
        );

        let anthropic_routes = router.resolve("claude-sonnet").unwrap();
        assert_eq!(
            anthropic_routes[0].credential.api_base,
            "https://api.anthropic.com"
        );

        let fallback_routes = router.resolve("gpt-4o-resilient").unwrap();
        assert_eq!(
            fallback_routes[1].credential.api_base,
            "http://localhost:8000/v1"
        );
    }
}
