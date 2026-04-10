# Alerting
A crate that manages sending alerts to (at present) Discord and PagerDuty.

## Using alerts
All alerts are exported at the top level of the crate. Simply invoke `alerting::alert_name(...)`

## Adding new alerts
1) Add a new alert to `alert.rs`.
2) Add the alert to trait `AlertProcessor` and implement on the respective implementors.

## Configuration
The Discord webhook is always enabled, with required config var `orchestrator_discord_alert_webhook_url`.
The PagerDuty alert processor may be disabled by omitting config var `pagerduty_alert_routing_key`.