use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;

/// A money amount as supplied by SevnX. `display` is a source-preserving
/// string; consumers must only replace its currency symbol with `¥`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoneyValue {
    pub value: Option<Decimal>,
    pub display: Option<String>,
}

impl MoneyValue {
    pub fn from_decimal(value: Option<Decimal>) -> Self {
        let display = value.as_ref().map(ToString::to_string);
        Self { value, display }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySummary {
    pub total: Option<Decimal>,
    pub active: Option<Decimal>,
    pub display_total: Option<String>,
    pub display_active: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSummary {
    pub total: Option<Decimal>,
    pub display_total: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendSummary {
    pub actual: Option<Decimal>,
    pub standard: Option<Decimal>,
    pub display_actual: Option<String>,
    pub display_standard: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenSummary {
    pub total: Option<Decimal>,
    pub input: Option<Decimal>,
    pub output: Option<Decimal>,
    pub display_total: Option<String>,
    pub display_input: Option<String>,
    pub display_output: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceSummary {
    pub rpm: Option<Decimal>,
    pub tpm: Option<Decimal>,
    pub display_rpm: Option<String>,
    pub display_tpm: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurationValue {
    pub milliseconds: Option<Decimal>,
    pub display: Option<String>,
}

/// The only Dashboard-shaped object that can cross IPC.
///
/// Every optional field serializes as `null`, rather than being omitted. This
/// lets the Vue layer consistently render missing data as `--` without making
/// up a value.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub balance: Option<MoneyValue>,
    pub api_keys: ApiKeySummary,
    pub today_requests: RequestSummary,
    pub today_spend: SpendSummary,
    pub today_tokens: TokenSummary,
    pub cumulative_tokens: TokenSummary,
    pub performance: PerformanceSummary,
    pub average_response: Option<DurationValue>,
    pub fetched_at: DateTime<Utc>,
}

pub(crate) fn display_decimal(value: &Option<Decimal>) -> Option<String> {
    value.as_ref().map(ToString::to_string)
}

pub(crate) fn display_duration(milliseconds: &Option<Decimal>) -> Option<String> {
    milliseconds.as_ref().map(|value| {
        let second = Decimal::from(1000);
        if *value >= second {
            format!("{}s", (*value / second).round_dp(2))
        } else {
            format!("{}ms", value.round_dp(0))
        }
    })
}

impl ApiKeySummary {
    pub(crate) fn from_values(total: Option<Decimal>, active: Option<Decimal>) -> Self {
        Self {
            display_total: display_decimal(&total),
            display_active: display_decimal(&active),
            total,
            active,
        }
    }
}

impl RequestSummary {
    pub(crate) fn from_total(total: Option<Decimal>) -> Self {
        Self {
            display_total: display_decimal(&total),
            total,
        }
    }
}

impl SpendSummary {
    pub(crate) fn from_values(actual: Option<Decimal>, standard: Option<Decimal>) -> Self {
        Self {
            display_actual: display_decimal(&actual),
            display_standard: display_decimal(&standard),
            actual,
            standard,
        }
    }
}

impl TokenSummary {
    pub(crate) fn from_values(
        total: Option<Decimal>,
        input: Option<Decimal>,
        output: Option<Decimal>,
    ) -> Self {
        Self {
            display_total: display_decimal(&total),
            display_input: display_decimal(&input),
            display_output: display_decimal(&output),
            total,
            input,
            output,
        }
    }
}

impl PerformanceSummary {
    pub(crate) fn from_values(rpm: Option<Decimal>, tpm: Option<Decimal>) -> Self {
        Self {
            display_rpm: display_decimal(&rpm),
            display_tpm: display_decimal(&tpm),
            rpm,
            tpm,
        }
    }
}

impl DurationValue {
    pub(crate) fn from_milliseconds(milliseconds: Option<Decimal>) -> Self {
        Self {
            display: display_duration(&milliseconds),
            milliseconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::display_duration;

    #[test]
    fn duration_display_uses_seconds_for_second_scale_values() {
        assert_eq!(
            display_duration(&Some(Decimal::new(12_506_643_591_501, 9))).as_deref(),
            Some("12.51s")
        );
    }

    #[test]
    fn duration_display_uses_milliseconds_below_one_second() {
        assert_eq!(
            display_duration(&Some(Decimal::new(683, 0))).as_deref(),
            Some("683ms")
        );
    }
}
