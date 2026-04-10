use uuid::Uuid;

use crate::statics::get_all_alert_processors;

/// Alert sent when a billing subscription lookup fails for an org.
pub async fn billing_subscription_lookup_failed(org_id: &Uuid, customer_id: &str, detail: &str) {
    for processor in get_all_alert_processors() {
        processor
            .billing_subscription_lookup_failed(org_id, customer_id, detail)
            .await;
    }
}

/// Alert sent when a Chelsea node exceeds its resource utilizaton warning threshold
pub async fn resource_threshold_exceeded(
    node_id: &Uuid,
    resource_name: &str,
    threshold: f32,
    current: f32,
) {
    for processor in get_all_alert_processors() {
        processor
            .chelsea_resource_threshold_exceeded(node_id, resource_name, threshold, current)
            .await;
    }
}
