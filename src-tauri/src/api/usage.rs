use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::{
    api::{
        deserialize_optional_decimal, deserialize_optional_identifier, error::ApiError,
        parse_success_envelope,
    },
    model::{
        DistributionRow, ModelPieSlice, TokenTrendPoint, UsageRange, UsageSnapshot, UsageSummary,
    },
};

/// Full observed, non-sensitive field set for `/usage/stats`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct UsageStats {
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_requests: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_input_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_output_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_cache_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_cache_creation_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_cache_read_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_actual_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub average_duration_ms: Option<Decimal>,
    #[serde(default)]
    pub endpoints: Vec<EndpointAggregation>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct EndpointAggregation {
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub requests: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub actual_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub cost: Option<Decimal>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct DashboardModels {
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelAggregation>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ModelAggregation {
    #[serde(default)]
    pub model: Option<String>,
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SnapshotV2 {
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub granularity: Option<String>,
    #[serde(default)]
    pub trend: Vec<SnapshotTrendPoint>,
    #[serde(default)]
    pub groups: Vec<GroupAggregation>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SnapshotTrendPoint {
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct GroupAggregation {
    #[serde(default, deserialize_with = "deserialize_optional_identifier")]
    pub group_id: Option<String>,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub requests: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_tokens: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub actual_cost: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub cost: Option<Decimal>,
}

pub fn parse_usage_stats(body: &[u8]) -> Result<UsageStats, ApiError> {
    parse_success_envelope(body, "/usage/stats")
}

pub fn parse_dashboard_models(body: &[u8]) -> Result<DashboardModels, ApiError> {
    parse_success_envelope(body, "/usage/dashboard/models")
}

pub fn parse_snapshot_v2(body: &[u8]) -> Result<SnapshotV2, ApiError> {
    parse_success_envelope(body, "/usage/dashboard/snapshot-v2")
}

impl UsageStats {
    pub fn into_snapshot(
        self,
        range: UsageRange,
        models: DashboardModels,
        snapshot: SnapshotV2,
        fetched_at: DateTime<Utc>,
    ) -> UsageSnapshot {
        let model_pie = models
            .models
            .iter()
            .map(|row| {
                ModelPieSlice::from_values(row.model.clone(), row.total_tokens, row.actual_cost)
            })
            .collect();
        let models = models
            .models
            .into_iter()
            .map(|row| {
                DistributionRow::from_values(
                    row.model,
                    row.requests,
                    row.total_tokens,
                    row.actual_cost,
                    row.cost,
                )
            })
            .collect();
        let groups = snapshot
            .groups
            .into_iter()
            .map(|row| {
                DistributionRow::from_values(
                    row.group_name,
                    row.requests,
                    row.total_tokens,
                    row.actual_cost,
                    row.cost,
                )
            })
            .collect();
        let token_trend = snapshot
            .trend
            .into_iter()
            .map(|row| {
                TokenTrendPoint::from_values(
                    row.date,
                    row.total_tokens,
                    row.input_tokens,
                    row.output_tokens,
                    row.cache_creation_tokens,
                    row.cache_read_tokens,
                    row.actual_cost,
                    row.cost,
                )
            })
            .collect();
        let endpoints = self
            .endpoints
            .iter()
            .map(|row| {
                DistributionRow::from_values(
                    row.endpoint.clone(),
                    row.requests,
                    row.total_tokens,
                    row.actual_cost,
                    row.cost,
                )
            })
            .collect();
        let summary = UsageSummary::from_values(
            self.total_requests,
            self.total_tokens,
            self.total_actual_cost,
            self.total_cost,
            self.average_duration_ms,
            self.total_input_tokens,
            self.total_output_tokens,
            self.total_cache_tokens,
            self.total_cache_creation_tokens,
            self.total_cache_read_tokens,
        );

        UsageSnapshot {
            range,
            summary,
            token_trend,
            models,
            groups,
            endpoints,
            model_pie,
            fetched_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rust_decimal::Decimal;

    use super::{parse_dashboard_models, parse_snapshot_v2, parse_usage_stats};
    use crate::{api::error::ApiError, model::UsageRange};

    const USAGE_STATS_FIXTURE: &str = r#"
    {
      "code": 0,
      "message": "成功",
      "data": {
        "total_requests": 20,
        "total_tokens": "2000",
        "total_input_tokens": 1200,
        "total_output_tokens": 800,
        "total_cache_tokens": 100,
        "total_cache_creation_tokens": 40,
        "total_cache_read_tokens": 60,
        "total_actual_cost": "2.25",
        "total_cost": "2.50",
        "average_duration_ms": "88.5",
        "endpoints": [{
          "endpoint": "/v1/chat/completions",
          "requests": 20,
          "total_tokens": 2000,
          "actual_cost": "2.25",
          "cost": "2.50"
        }]
      }
    }"#;

    const MODELS_FIXTURE: &str = r#"
    {
      "code": 0,
      "data": {
        "start_date": "2026-07-16",
        "end_date": "2026-07-17",
        "models": [{
          "model": "gpt-safe-model",
          "requests": 12,
          "total_tokens": 1500,
          "input_tokens": 900,
          "output_tokens": 600,
          "cache_creation_tokens": 10,
          "cache_read_tokens": 20,
          "actual_cost": "1.75",
          "cost": "2.00"
        }]
      }
    }"#;

    const SNAPSHOT_FIXTURE: &str = r#"
    {
      "code": 0,
      "data": {
        "start_date": "2026-07-16",
        "end_date": "2026-07-17",
        "generated_at": "2026-07-17T12:00:00Z",
        "granularity": "hour",
        "trend": [{
          "date": "2026-07-17T10:00:00Z",
          "requests": 5,
          "total_tokens": 500,
          "input_tokens": 300,
          "output_tokens": 200,
          "cache_creation_tokens": 2,
          "cache_read_tokens": 3,
          "actual_cost": "0.50",
          "cost": "0.60"
        }],
        "groups": [{
          "group_id": "group-id-must-not-leave-ipc",
          "group_name": "默认分组",
          "requests": 20,
          "total_tokens": 2000,
          "actual_cost": "2.25",
          "cost": "2.50"
        }]
      }
    }"#;

    #[test]
    fn complete_observed_usage_fields_build_whitelisted_snapshot() {
        let stats = parse_usage_stats(USAGE_STATS_FIXTURE.as_bytes()).expect("stats parse");
        let models = parse_dashboard_models(MODELS_FIXTURE.as_bytes()).expect("models parse");
        let snapshot = parse_snapshot_v2(SNAPSHOT_FIXTURE.as_bytes()).expect("snapshot parse");

        let usage = stats.into_snapshot(UsageRange::Last24Hours, models, snapshot, Utc::now());
        assert_eq!(usage.summary.total_requests, Some(Decimal::new(20, 0)));
        assert_eq!(usage.summary.total_cache_tokens, Some(Decimal::new(100, 0)));
        assert_eq!(
            usage.summary.average_duration_ms,
            Some(Decimal::new(885, 1))
        );
        assert_eq!(
            usage.token_trend[0].total_tokens,
            Some(Decimal::new(500, 0))
        );
        assert_eq!(
            usage.token_trend[0].input_tokens,
            Some(Decimal::new(300, 0))
        );
        assert_eq!(
            usage.token_trend[0].output_tokens,
            Some(Decimal::new(200, 0))
        );
        assert_eq!(
            usage.token_trend[0].cache_creation_tokens,
            Some(Decimal::new(2, 0))
        );
        assert_eq!(
            usage.token_trend[0].cache_read_tokens,
            Some(Decimal::new(3, 0))
        );
        assert_eq!(usage.token_trend[0].actual_cost, Some(Decimal::new(50, 2)));
        assert_eq!(usage.token_trend[0].cost, Some(Decimal::new(60, 2)));
        assert_eq!(usage.models[0].label.as_deref(), Some("gpt-safe-model"));
        assert_eq!(usage.groups[0].label.as_deref(), Some("默认分组"));
        assert_eq!(
            usage.endpoints[0].label.as_deref(),
            Some("/v1/chat/completions")
        );
        assert_eq!(usage.model_pie[0].actual_cost, Some(Decimal::new(175, 2)));

        let ipc = serde_json::to_string(&usage).expect("snapshot serializes");
        assert!(ipc.contains("\"summary\""));
        assert!(ipc.contains("\"totalActualCost\""));
        assert!(ipc.contains("\"cacheReadTokens\""));
        assert!(ipc.contains("\"actualCost\""));
        assert!(!ipc.contains("group-id-must-not-leave-ipc"));
    }

    #[test]
    fn absent_usage_fields_remain_none_and_collections_are_empty() {
        let stats = parse_usage_stats(br#"{"code":0,"data":{}}"#).expect("minimal stats");
        let models = parse_dashboard_models(br#"{"code":0,"data":{}}"#).expect("minimal models");
        let snapshot = parse_snapshot_v2(br#"{"code":0,"data":{}}"#).expect("minimal snapshot");

        let usage = stats.into_snapshot(UsageRange::Today, models, snapshot, Utc::now());
        assert_eq!(usage.summary.total_tokens, None);
        assert!(usage.token_trend.is_empty());
        assert!(usage.models.is_empty());
        assert!(usage.groups.is_empty());
        assert!(usage.endpoints.is_empty());
        let value = serde_json::to_value(usage).expect("snapshot serializes");
        assert!(value["summary"]["totalTokens"].is_null());
    }

    #[test]
    fn invalid_known_usage_field_is_not_coerced() {
        let result = parse_usage_stats(br#"{"code":0,"data":{"total_actual_cost":[]}}"#);
        assert!(matches!(result, Err(ApiError::InvalidResponse { .. })));
    }
}
