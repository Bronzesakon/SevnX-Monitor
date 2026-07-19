use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::dashboard::{display_decimal, display_duration};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageRange {
    Today,
    Yesterday,
    #[default]
    Last24Hours,
    Last7Days,
    Last14Days,
    Last30Days,
    ThisMonth,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub total_requests: Option<Decimal>,
    pub total_tokens: Option<Decimal>,
    pub total_actual_cost: Option<Decimal>,
    pub total_cost: Option<Decimal>,
    pub average_duration_ms: Option<Decimal>,
    pub total_input_tokens: Option<Decimal>,
    pub total_output_tokens: Option<Decimal>,
    pub total_cache_tokens: Option<Decimal>,
    pub total_cache_creation_tokens: Option<Decimal>,
    pub total_cache_read_tokens: Option<Decimal>,
    pub display_total_requests: Option<String>,
    pub display_total_tokens: Option<String>,
    pub display_total_actual_cost: Option<String>,
    pub display_total_cost: Option<String>,
    pub display_average_duration_ms: Option<String>,
    pub display_total_input_tokens: Option<String>,
    pub display_total_output_tokens: Option<String>,
    pub display_total_cache_tokens: Option<String>,
    pub display_total_cache_creation_tokens: Option<String>,
    pub display_total_cache_read_tokens: Option<String>,
}

impl UsageSummary {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_values(
        total_requests: Option<Decimal>,
        total_tokens: Option<Decimal>,
        total_actual_cost: Option<Decimal>,
        total_cost: Option<Decimal>,
        average_duration_ms: Option<Decimal>,
        total_input_tokens: Option<Decimal>,
        total_output_tokens: Option<Decimal>,
        total_cache_tokens: Option<Decimal>,
        total_cache_creation_tokens: Option<Decimal>,
        total_cache_read_tokens: Option<Decimal>,
    ) -> Self {
        Self {
            display_total_requests: display_decimal(&total_requests),
            display_total_tokens: display_decimal(&total_tokens),
            display_total_actual_cost: display_decimal(&total_actual_cost),
            display_total_cost: display_decimal(&total_cost),
            display_average_duration_ms: display_duration(&average_duration_ms),
            display_total_input_tokens: display_decimal(&total_input_tokens),
            display_total_output_tokens: display_decimal(&total_output_tokens),
            display_total_cache_tokens: display_decimal(&total_cache_tokens),
            display_total_cache_creation_tokens: display_decimal(&total_cache_creation_tokens),
            display_total_cache_read_tokens: display_decimal(&total_cache_read_tokens),
            total_requests,
            total_tokens,
            total_actual_cost,
            total_cost,
            average_duration_ms,
            total_input_tokens,
            total_output_tokens,
            total_cache_tokens,
            total_cache_creation_tokens,
            total_cache_read_tokens,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTrendPoint {
    pub date: Option<String>,
    pub total_tokens: Option<Decimal>,
    pub display_total_tokens: Option<String>,
}

impl TokenTrendPoint {
    pub(crate) fn from_values(date: Option<String>, total_tokens: Option<Decimal>) -> Self {
        Self {
            display_total_tokens: display_decimal(&total_tokens),
            date,
            total_tokens,
        }
    }
}

/// A row in the model, group, or endpoint aggregation tables. The source
/// endpoint supplies each value; no totals are calculated locally.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributionRow {
    pub label: Option<String>,
    pub requests: Option<Decimal>,
    pub tokens: Option<Decimal>,
    pub actual_cost: Option<Decimal>,
    pub standard_cost: Option<Decimal>,
    pub display_requests: Option<String>,
    pub display_tokens: Option<String>,
    pub display_actual_cost: Option<String>,
    pub display_standard_cost: Option<String>,
}

impl DistributionRow {
    pub(crate) fn from_values(
        label: Option<String>,
        requests: Option<Decimal>,
        tokens: Option<Decimal>,
        actual_cost: Option<Decimal>,
        standard_cost: Option<Decimal>,
    ) -> Self {
        Self {
            display_requests: display_decimal(&requests),
            display_tokens: display_decimal(&tokens),
            display_actual_cost: display_decimal(&actual_cost),
            display_standard_cost: display_decimal(&standard_cost),
            label,
            requests,
            tokens,
            actual_cost,
            standard_cost,
        }
    }
}

/// Whitelisted pie data. It intentionally has no raw model response or usage
/// record attached, so tooltip payloads cannot disclose identifiers or tokens.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPieSlice {
    pub label: Option<String>,
    pub tokens: Option<Decimal>,
    pub actual_cost: Option<Decimal>,
    pub display_tokens: Option<String>,
    pub display_actual_cost: Option<String>,
}

impl ModelPieSlice {
    pub(crate) fn from_values(
        label: Option<String>,
        tokens: Option<Decimal>,
        actual_cost: Option<Decimal>,
    ) -> Self {
        Self {
            display_tokens: display_decimal(&tokens),
            display_actual_cost: display_decimal(&actual_cost),
            label,
            tokens,
            actual_cost,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub range: UsageRange,
    pub summary: UsageSummary,
    pub token_trend: Vec<TokenTrendPoint>,
    pub models: Vec<DistributionRow>,
    pub groups: Vec<DistributionRow>,
    pub endpoints: Vec<DistributionRow>,
    pub model_pie: Vec<ModelPieSlice>,
    pub fetched_at: DateTime<Utc>,
}
