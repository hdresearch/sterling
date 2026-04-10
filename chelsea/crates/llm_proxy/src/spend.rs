//! Cost calculation for LLM requests.
//!
//! Prices are per 1M tokens. Uses exact match first, then prefix matching
//! for model families (e.g. all `claude-opus-4-*` variants share pricing).

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::UsageInfo;

struct ModelPricing {
    input_per_1m: Decimal,
    output_per_1m: Decimal,
}

const ONE_MILLION: Decimal = dec!(1_000_000);

/// Prefix-based pricing table. Order matters — first match wins.
/// More specific prefixes should come before broader ones.
static PRICING: &[(&str, ModelPricing)] = &[
    // OpenAI
    (
        "gpt-4o-mini",
        ModelPricing {
            input_per_1m: dec!(0.15),
            output_per_1m: dec!(0.60),
        },
    ),
    (
        "gpt-4o",
        ModelPricing {
            input_per_1m: dec!(2.50),
            output_per_1m: dec!(10.00),
        },
    ),
    (
        "gpt-4-turbo",
        ModelPricing {
            input_per_1m: dec!(10.00),
            output_per_1m: dec!(30.00),
        },
    ),
    (
        "o1-mini",
        ModelPricing {
            input_per_1m: dec!(3.00),
            output_per_1m: dec!(12.00),
        },
    ),
    (
        "o1",
        ModelPricing {
            input_per_1m: dec!(15.00),
            output_per_1m: dec!(60.00),
        },
    ),
    (
        "o3-mini",
        ModelPricing {
            input_per_1m: dec!(1.10),
            output_per_1m: dec!(4.40),
        },
    ),
    // Anthropic — prefix matches all dated variants + aliases
    (
        "claude-opus-4",
        ModelPricing {
            input_per_1m: dec!(15.00),
            output_per_1m: dec!(75.00),
        },
    ),
    (
        "claude-sonnet-4",
        ModelPricing {
            input_per_1m: dec!(3.00),
            output_per_1m: dec!(15.00),
        },
    ),
    (
        "claude-haiku-4",
        ModelPricing {
            input_per_1m: dec!(0.80),
            output_per_1m: dec!(4.00),
        },
    ),
    (
        "claude-3-5-haiku",
        ModelPricing {
            input_per_1m: dec!(0.80),
            output_per_1m: dec!(4.00),
        },
    ),
    (
        "claude-3-5-sonnet",
        ModelPricing {
            input_per_1m: dec!(3.00),
            output_per_1m: dec!(15.00),
        },
    ),
    (
        "claude-3-opus",
        ModelPricing {
            input_per_1m: dec!(15.00),
            output_per_1m: dec!(75.00),
        },
    ),
];

/// Calculate the cost of a request given the model name and usage info.
/// Returns Decimal::ZERO for unknown models (they still get logged, just no dollar cost).
pub fn calculate_cost(model_name: &str, usage: &UsageInfo) -> Decimal {
    let pricing = match PRICING
        .iter()
        .find(|(prefix, _)| model_name.starts_with(prefix))
    {
        Some((_, p)) => p,
        None => return Decimal::ZERO,
    };

    let input_cost = (Decimal::from(usage.prompt_tokens) / ONE_MILLION) * pricing.input_per_1m;
    let output_cost =
        (Decimal::from(usage.completion_tokens) / ONE_MILLION) * pricing.output_per_1m;

    input_cost + output_cost
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::UsageInfo;

    #[test]
    fn known_model_pricing() {
        let usage = UsageInfo {
            prompt_tokens: 1_000_000,
            completion_tokens: 1_000_000,
            total_tokens: 2_000_000,
        };
        // gpt-4o: $2.50/1M input + $10.00/1M output = $12.50
        let cost = calculate_cost("gpt-4o", &usage);
        assert_eq!(cost, dec!(12.50));
    }

    #[test]
    fn anthropic_pricing() {
        let usage = UsageInfo {
            prompt_tokens: 500_000,
            completion_tokens: 100_000,
            total_tokens: 600_000,
        };
        // claude-sonnet-4: $3.00/1M input, $15.00/1M output
        // 0.5M * 3.00 + 0.1M * 15.00 = 1.50 + 1.50 = 3.00
        let cost = calculate_cost("claude-sonnet-4-20250514", &usage);
        assert_eq!(cost, dec!(3.00));
    }

    #[test]
    fn prefix_matching_new_model_variants() {
        let usage = UsageInfo {
            prompt_tokens: 1_000_000,
            completion_tokens: 1_000_000,
            total_tokens: 2_000_000,
        };
        // All claude-opus-4-* variants should get opus pricing ($15 + $75 = $90)
        assert_eq!(calculate_cost("claude-opus-4-6", &usage), dec!(90));
        assert_eq!(calculate_cost("claude-opus-4-6-thinking", &usage), dec!(90));
        assert_eq!(calculate_cost("claude-opus-4-20250514", &usage), dec!(90));
        assert_eq!(calculate_cost("claude-opus-4-5-20251101", &usage), dec!(90));

        // gpt-4o-mini should match before gpt-4o
        let mini_cost = calculate_cost("gpt-4o-mini", &usage);
        let full_cost = calculate_cost("gpt-4o", &usage);
        assert!(mini_cost < full_cost);
    }

    #[test]
    fn unknown_model_returns_zero() {
        let usage = UsageInfo {
            prompt_tokens: 1000,
            completion_tokens: 1000,
            total_tokens: 2000,
        };
        let cost = calculate_cost("some-unknown-model-v9", &usage);
        assert_eq!(cost, Decimal::ZERO);
    }

    #[test]
    fn zero_tokens_zero_cost() {
        let cost = calculate_cost("gpt-4o", &UsageInfo::default());
        assert_eq!(cost, Decimal::ZERO);
    }

    #[test]
    fn small_request_cost() {
        let usage = UsageInfo {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        // gpt-4o: (100/1M * 2.50) + (50/1M * 10.00) = 0.00025 + 0.0005 = 0.00075
        let cost = calculate_cost("gpt-4o", &usage);
        assert_eq!(cost, dec!(0.00075));
    }
}
