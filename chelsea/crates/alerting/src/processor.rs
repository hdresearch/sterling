use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait AlertProcessor: Send + Sync {
    /// Alert sent when a billing subscription lookup fails for an org.
    async fn billing_subscription_lookup_failed(
        &self,
        org_id: &Uuid,
        customer_id: &str,
        detail: &str,
    );
    /// Alert sent when a Chelsea node exceeds its resource utilizaton warning threshold
    async fn chelsea_resource_threshold_exceeded(
        &self,
        node_id: &Uuid,
        resource_name: &str,
        threshold: f32,
        current: f32,
    );
}
