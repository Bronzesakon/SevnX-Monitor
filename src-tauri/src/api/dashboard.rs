use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::{
    api::{deserialize_optional_decimal, error::ApiError, parse_success_envelope},
    model::{
        ApiKeySummary, DashboardSnapshot, DurationValue, MoneyValue, PerformanceSummary,
        RequestSummary, SpendSummary, TokenSummary,
    },
};

/// Whitelisted `/auth/me` data. Other account fields are purposefully ignored
/// at deserialization time, so they cannot be stored or sent to IPC.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct AuthMeData {
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub balance: Option<Decimal>,
}

/// Full observed, non-sensitive field set for `/usage/dashboard/stats`.
/// Additional upstream fields are ignored, while a wrong type for any known
/// field fails parsing rather than silently becoming a derived value.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct DashboardStats {
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_api_keys: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub active_api_keys: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_requests: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_requests: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_actual_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_actual_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_input_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_output_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_cache_creation_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_cache_read_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_input_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_output_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_cache_creation_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_cache_read_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub rpm: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub tpm: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub average_duration_ms: Option<Decimal>,
    #[serde(default)]
    pub by_platform: Vec<DashboardPlatform>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct DashboardPlatform {
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_actual_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_requests: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub today_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_actual_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_requests: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_tokens: Option<Decimal>,
}

/// Compatibility-only model for the observed dashboard trend response. The
/// product does not request this endpoint, but keeping the strict parser makes
/// field changes diagnosable without ever exposing the raw response.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct DashboardTrendCompatibility {
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub granularity: Option<String>,
    #[serde(default)]
    pub trend: Vec<DashboardTrendPoint>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct DashboardTrendPoint {
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub requests: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub input_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub output_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub cache_creation_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub cache_read_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub actual_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub cost: Option<Decimal>,
}

pub fn parse_auth_me(body: &[u8]) -> Result<AuthMeData, ApiError> {
    parse_success_envelope(body, "/auth/me")
}

pub fn parse_dashboard_stats(body: &[u8]) -> Result<DashboardStats, ApiError> {
    parse_success_envelope(body, "/usage/dashboard/stats")
}

pub fn parse_dashboard_trend_compat(body: &[u8]) -> Result<DashboardTrendCompatibility, ApiError> {
    parse_success_envelope(body, "/usage/dashboard/trend")
}

impl DashboardStats {
    /// Build the IPC-safe eight-card dashboard without accumulating or
    /// calculating any dashboard field locally.
    pub fn into_snapshot(
        self,
        balance: Option<Decimal>,
        fetched_at: DateTime<Utc>,
    ) -> DashboardSnapshot {
        DashboardSnapshot {
            balance: balance.map(|value| MoneyValue::from_decimal(Some(value))),
            api_keys: ApiKeySummary::from_values(self.total_api_keys, self.active_api_keys),
            today_requests: RequestSummary::from_total(self.today_requests),
            today_spend: SpendSummary::from_values(self.today_actual_cost, self.today_cost),
            today_tokens: TokenSummary::from_values(
                self.today_tokens,
                self.today_input_tokens,
                self.today_output_tokens,
            ),
            cumulative_tokens: TokenSummary::from_values(
                self.total_tokens,
                self.total_input_tokens,
                self.total_output_tokens,
            ),
            performance: PerformanceSummary::from_values(self.rpm, self.tpm),
            average_response: self
                .average_duration_ms
                .map(|milliseconds| DurationValue::from_milliseconds(Some(milliseconds))),
            fetched_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rust_decimal::Decimal;

    use super::{parse_auth_me, parse_dashboard_stats, parse_dashboard_trend_compat};
    use crate::api::error::ApiError;

    const AUTH_ME_FIXTURE: &str = r#"
    {
      "code": 0,
      "message": "成功",
      "data": {
        "balance": "18.50",
        "id": "account-id-must-not-leave-parser",
        "email": "person@example.invalid",
        "token": "test-auth-token-must-not-leave-parser"
      }
    }"#;

    const DASHBOARD_STATS_FIXTURE: &str = r#"
    {
      "code": 0,
      "message": "成功",
      "data": {
        "total_api_keys": 4,
        "active_api_keys": "3",
        "today_requests": 10,
        "total_requests": 100,
        "today_actual_cost": "1.25",
        "today_cost": "1.50",
        "total_actual_cost": "10.25",
        "total_cost": "11.50",
        "today_tokens": "12345",
        "today_input_tokens": 10000,
        "today_output_tokens": 2345,
        "today_cache_creation_tokens": 100,
        "today_cache_read_tokens": 200,
        "total_tokens": "56789",
        "total_input_tokens": 50000,
        "total_output_tokens": 6789,
        "total_cache_creation_tokens": 300,
        "total_cache_read_tokens": 400,
        "rpm": "2.5",
        "tpm": 5000,
        "average_duration_ms": "120.5",
        "by_platform": [{
          "platform": "openai",
          "today_actual_cost": "1.25",
          "today_requests": 10,
          "today_tokens": 12345,
          "total_actual_cost": "10.25",
          "total_requests": 100,
          "total_tokens": 56789
        }]
      }
    }"#;

    #[test]
    fn complete_observed_dashboard_fields_parse_without_derivation() {
        let auth = parse_auth_me(AUTH_ME_FIXTURE.as_bytes()).expect("auth fixture parses");
        let stats = parse_dashboard_stats(DASHBOARD_STATS_FIXTURE.as_bytes())
            .expect("dashboard fixture parses");

        assert_eq!(auth.balance, Some(Decimal::new(1850, 2)));
        assert_eq!(stats.today_tokens, Some(Decimal::new(12345, 0)));
        assert_eq!(stats.today_input_tokens, Some(Decimal::new(10000, 0)));
        assert_eq!(stats.today_output_tokens, Some(Decimal::new(2345, 0)));
        assert_eq!(stats.by_platform.len(), 1);

        let snapshot = stats.into_snapshot(auth.balance, Utc::now());
        assert_eq!(snapshot.today_tokens.total, Some(Decimal::new(12345, 0)));
        assert_eq!(
            snapshot.today_tokens.display_total.as_deref(),
            Some("12345")
        );
        assert_eq!(snapshot.today_spend.actual, Some(Decimal::new(125, 2)));
        assert_eq!(snapshot.performance.rpm, Some(Decimal::new(25, 1)));
        assert_eq!(
            snapshot.balance.as_ref().and_then(|balance| balance.value),
            Some(Decimal::new(1850, 2))
        );
        assert_eq!(
            snapshot
                .balance
                .as_ref()
                .and_then(|balance| balance.display.as_deref()),
            Some("18.50")
        );

        let ipc = serde_json::to_string(&snapshot).expect("snapshot serializes");
        assert!(ipc.contains("\"apiKeys\""));
        assert!(!ipc.contains("person@example.invalid"));
        assert!(!ipc.contains("account-id-must-not-leave-parser"));
        assert!(!ipc.contains("test-auth-token-must-not-leave-parser"));
        assert!(!ipc.contains("byPlatform"));
    }

    #[test]
    fn missing_dashboard_fields_become_null_not_calculated_values() {
        let stats = parse_dashboard_stats(br#"{"code":0,"data":{}}"#).expect("minimal data");
        let snapshot = stats.into_snapshot(None, Utc::now());

        assert_eq!(snapshot.today_tokens.total, None);
        assert_eq!(snapshot.average_response, None);
        let ipc = serde_json::to_value(snapshot).expect("snapshot serializes");
        assert!(ipc["balance"].is_null());
        assert!(ipc["todayTokens"]["total"].is_null());
        assert!(ipc["apiKeys"]["total"].is_null());
    }

    #[test]
    fn known_field_with_wrong_type_is_rejected() {
        let result = parse_dashboard_stats(br#"{"code":0,"data":{"today_tokens":true}}"#);
        assert!(matches!(result, Err(ApiError::InvalidResponse { .. })));
    }

    #[test]
    fn business_code_is_checked_before_data_and_message_is_dropped() {
        let result = parse_dashboard_stats(
            br#"{"code":401,"message":"token=test-business-token","data":{"today_tokens":true}}"#,
        );
        let error = result.expect_err("business code must fail first");
        assert!(matches!(error, ApiError::BusinessRejected { code: 401 }));
        let public = error.to_public();
        assert!(!public.message.contains("test-business-token"));
    }

    #[test]
    fn dashboard_trend_compatibility_parser_keeps_observed_types_strict() {
        let trend = parse_dashboard_trend_compat(
            br#"{"code":0,"data":{"start_date":"2026-07-17","end_date":"2026-07-17","granularity":"hour","trend":[{"date":"10:00","requests":1,"total_tokens":2,"input_tokens":1,"output_tokens":1,"cache_creation_tokens":0,"cache_read_tokens":0,"actual_cost":"0.1","cost":"0.2"}]}}"#,
        )
        .expect("compat fixture parses");
        assert_eq!(trend.trend[0].total_tokens, Some(Decimal::new(2, 0)));
    }
}
