//! Quick smoke test: send a real meter event to Stripe test mode.
//!
//! Usage:
//!   STRIPE_SECRET_KEY=sk_test_... cargo run -p billing --example test_meter

use billing::stripe::client::StripeClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key =
        std::env::var("STRIPE_SECRET_KEY").expect("set STRIPE_SECRET_KEY to your Stripe test key");

    let client = StripeClient::new(&key)?;

    // Use the test customer from the Stripe account
    let customer_id = "cus_UAUqvLpc4aNh9V";
    let millicents = 250; // $0.25 of LLM usage
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    println!("Sending meter event: {millicents} millicents for {customer_id}");
    client
        .send_meter_event("llm_spend", customer_id, millicents, timestamp as i64)
        .await?;
    println!("✓ Meter event sent successfully");

    // Also test credit balance query
    println!("\nQuerying credit balance for {customer_id}...");
    let balance = client.get_credit_balance(customer_id).await?;
    println!(
        "✓ Credit balance: {balance} cents (${:.2})",
        balance as f64 / 100.0
    );

    Ok(())
}
