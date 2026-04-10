use std::sync::OnceLock;

use vers_config::VersConfig;

use crate::processor::AlertProcessor;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static DISCORD_WEBHOOK_CLIENT: OnceLock<crate::discord::Client> = OnceLock::new();
static PAGERDUTY_API_CLIENT: OnceLock<Option<crate::pagerduty::Client>> = OnceLock::new();

fn get_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| reqwest::Client::new())
}

/// Returns a reference to a shared Discord webhook client
fn get_discord_webhook_client() -> &'static crate::discord::Client {
    DISCORD_WEBHOOK_CLIENT.get_or_init(|| {
        crate::discord::Client::with_client(
            VersConfig::common().discord_alert_webhook_url.clone(),
            get_http_client().clone(),
        )
    })
}

/// Returns a reference to a shared PagerDuty API client; will be None if pagerduty_alert_routing_key is None
fn get_pagerduty_api_client() -> &'static Option<crate::pagerduty::Client> {
    PAGERDUTY_API_CLIENT.get_or_init(|| {
        let routing_key = VersConfig::common().pagerduty_alert_routing_key.as_ref()?;
        Some(crate::pagerduty::Client::with_client(
            get_http_client().clone(),
            routing_key.clone(),
        ))
    })
}

/// Returns a list of all alert processors currently enabled.
pub fn get_all_alert_processors() -> Vec<&'static dyn AlertProcessor> {
    let mut processors: Vec<&'static dyn AlertProcessor> = Vec::new();
    processors.push(get_discord_webhook_client());

    if let Some(pd_client) = get_pagerduty_api_client() {
        processors.push(pd_client);
    }

    processors
}
