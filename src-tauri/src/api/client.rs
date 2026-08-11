use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, Utc};
use reqwest::{
    Client,
    header::{ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, COOKIE, REFERER, SET_COOKIE, USER_AGENT},
};

use crate::{
    api::{
        dashboard::{parse_auth_me, parse_dashboard_stats},
        endpoints::Endpoint,
        error::ApiError,
        usage::{parse_dashboard_models, parse_snapshot_v2, parse_usage_stats},
    },
    auth::{RequestContext, Session},
    model::{DashboardSnapshot, UsageRange, UsageSnapshot},
};

/// Explicit, server-semantics date parameters. Their construction is kept
/// separate from `UsageRange` because the product specification requires the
/// SevnX dashboard's own range semantics to be verified, not guessed locally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedUsageQuery {
    start_date: String,
    end_date: String,
    granularity: String,
}

impl VerifiedUsageQuery {
    pub fn new(
        start_date: impl Into<String>,
        end_date: impl Into<String>,
        granularity: impl Into<String>,
    ) -> Result<Self, ApiError> {
        let start_date = start_date.into();
        let end_date = end_date.into();
        let granularity = granularity.into();
        let start = NaiveDate::parse_from_str(&start_date, "%Y-%m-%d").map_err(|_| {
            ApiError::InvalidResponse {
                endpoint: "usage query",
            }
        })?;
        let end = NaiveDate::parse_from_str(&end_date, "%Y-%m-%d").map_err(|_| {
            ApiError::InvalidResponse {
                endpoint: "usage query",
            }
        })?;
        if start > end
            || granularity.is_empty()
            || granularity.len() > 32
            || !granularity.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '_' || character == '-'
            })
        {
            return Err(ApiError::InvalidResponse {
                endpoint: "usage query",
            });
        }
        Ok(Self {
            start_date,
            end_date,
            granularity,
        })
    }

    fn pairs(&self) -> [(&str, &str); 2] {
        [
            ("start_date", self.start_date.as_str()),
            ("end_date", self.end_date.as_str()),
        ]
    }

    pub fn for_range(range: UsageRange) -> Result<Self, ApiError> {
        // The verified SevnX endpoints accept date-shaped start/end values.
        // The browser's exact date semantics are retained in the query shape;
        // this mapping only selects the frozen product ranges and never derives
        // dashboard totals from client time.
        let today = Local::now().date_naive();
        let (start, end, granularity) = match range {
            UsageRange::Today => (today, today, "hour"),
            UsageRange::Yesterday => {
                let yesterday = today - ChronoDuration::days(1);
                (yesterday, yesterday, "day")
            }
            UsageRange::Last24Hours => (today - ChronoDuration::days(1), today, "hour"),
            UsageRange::Last7Days => (today - ChronoDuration::days(6), today, "day"),
            UsageRange::Last14Days => (today - ChronoDuration::days(13), today, "day"),
            UsageRange::Last30Days => (today - ChronoDuration::days(29), today, "day"),
            UsageRange::ThisMonth => (
                NaiveDate::from_ymd_opt(today.year(), today.month(), 1).ok_or(
                    ApiError::InvalidResponse {
                        endpoint: "usage query",
                    },
                )?,
                today,
                "day",
            ),
        };
        Self::new(
            start.format("%Y-%m-%d").to_string(),
            end.format("%Y-%m-%d").to_string(),
            granularity,
        )
    }
}

#[derive(Clone)]
pub struct SevnxApiClient {
    http: Client,
    session: Session,
    default_api_origin: Arc<Mutex<String>>,
}

impl SevnxApiClient {
    pub fn new(session: Session) -> Result<Self, ApiError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .user_agent("SevnX-Monitor/1.0.0")
            .build()
            .map_err(|_| ApiError::HttpStatus)?;
        Ok(Self {
            http,
            session,
            default_api_origin: Arc::new(Mutex::new(Endpoint::API_ORIGIN.to_owned())),
        })
    }

    pub fn set_default_api_origin(&self, access_url: &str) {
        let origin = format!("{}/api/v1", access_url.trim().trim_end_matches('/'));
        if let Ok(mut current) = self.default_api_origin.lock() {
            *current = origin;
        }
    }

    pub async fn fetch_dashboard(&self) -> Result<DashboardSnapshot, ApiError> {
        let (stats_result, auth_result) = tokio::join!(
            self.request_bytes(Endpoint::DashboardStats, &[]),
            self.request_bytes(Endpoint::AuthMe, &[("timezone", "Asia/Shanghai")])
        );
        let mut bodies = resolve_request_results(vec![stats_result, auth_result])?;
        let stats_body = bodies.remove(0);
        let auth_body = bodies.remove(0);
        let stats = self
            .parse_or_expire(parse_dashboard_stats(&stats_body))
            .await?;
        let auth = self.parse_or_expire(parse_auth_me(&auth_body)).await?;
        self.session.mark_validated().await;
        Ok(stats.into_snapshot(auth.balance, Utc::now()))
    }

    /// Validates an existing session using the same read-only endpoint accepted
    /// by the WebView2 Dashboard flow.
    pub async fn validate_session(&self) -> Result<(), ApiError> {
        let body = self.request_bytes(Endpoint::DashboardStats, &[]).await?;
        let _ = self.parse_or_expire(parse_dashboard_stats(&body)).await?;
        self.session.mark_validated().await;
        Ok(())
    }

    /// Validates transient WebView2 credentials without placing them in the
    /// application session first. The login page's actual Dashboard flow uses
    /// this read-only endpoint, whereas `/auth/me` rejects its Bearer header.
    /// A failed probe must never expire or overwrite the existing Rust session.
    pub(crate) async fn validate_login_candidate(
        &self,
        cookie_header: Option<&str>,
        authorization_header: Option<&str>,
        api_origin: Option<&str>,
        context: Option<&RequestContext>,
    ) -> Result<(), ApiError> {
        let cookie_header = cookie_header.filter(|value| !value.trim().is_empty());
        let authorization_header = authorization_header.filter(|value| !value.trim().is_empty());
        if cookie_header.is_none() && authorization_header.is_none() {
            return Err(ApiError::MissingCredentials);
        }
        let body = self
            .request_bytes_with_credentials(
                Endpoint::DashboardStats,
                &[],
                cookie_header,
                authorization_header,
                api_origin,
                context,
            )
            .await?;
        let _ = parse_dashboard_stats(&body)?;
        Ok(())
    }

    pub async fn fetch_usage(
        &self,
        range: UsageRange,
        query: VerifiedUsageQuery,
    ) -> Result<UsageSnapshot, ApiError> {
        let shared = query.pairs();
        let mut model_query = shared.to_vec();
        model_query.push(("model_source", "requested"));
        let mut snapshot_query = shared.to_vec();
        snapshot_query.extend([
            ("granularity", query.granularity.as_str()),
            ("include_trend", "true"),
            ("include_model_stats", "false"),
            ("include_group_stats", "true"),
        ]);

        let (stats_result, models_result, snapshot_result) = tokio::join!(
            self.request_bytes(Endpoint::UsageStats, &shared),
            self.request_bytes(Endpoint::DashboardModels, &model_query),
            self.request_bytes(Endpoint::DashboardSnapshotV2, &snapshot_query)
        );
        let mut bodies =
            resolve_request_results(vec![stats_result, models_result, snapshot_result])?;
        let stats_body = bodies.remove(0);
        let models_body = bodies.remove(0);
        let snapshot_body = bodies.remove(0);
        let stats = self.parse_or_expire(parse_usage_stats(&stats_body)).await?;
        let models = self
            .parse_or_expire(parse_dashboard_models(&models_body))
            .await?;
        let snapshot = self
            .parse_or_expire(parse_snapshot_v2(&snapshot_body))
            .await?;
        self.session.mark_validated().await;
        Ok(stats.into_snapshot(range, models, snapshot, Utc::now()))
    }

    /// 用 refresh_token 调 `/auth/refresh` 换新 access_token + 轮换 refresh_token。
    /// 成功后更新 Session 内存态；持久化由 AppServices 在成功刷新后统一落盘。
    pub async fn refresh_session(&self) -> Result<(), ApiError> {
        // Do not use `request_credentials` here: it correctly rejects an
        // expired access token for normal API calls, but that expiry is exactly
        // when `/auth/refresh` must still be allowed to run.
        let credentials = self.session.refresh_credentials().await?;
        let default_origin = self
            .default_api_origin
            .lock()
            .map(|origin| origin.clone())
            .unwrap_or_else(|_| Endpoint::API_ORIGIN.to_owned());
        let origin = credentials
            .request_context
            .api_origin
            .as_deref()
            .unwrap_or(default_origin.as_str());
        let url = format!(
            "{}{}",
            origin.trim_end_matches('/'),
            Endpoint::RefreshToken.path()
        );
        let response = self
            .http
            .post(&url)
            .header(REFERER, dashboard_referer(origin))
            .json(&serde_json::json!({ "refresh_token": credentials.refresh_token }))
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let error = ApiError::from_http_status(status.as_u16());
            if error.is_auth_invalid() {
                self.session.mark_expired().await;
            }
            return Err(error);
        }
        let body = response.bytes().await.map_err(classify_reqwest_error)?;
        let value: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
            ApiError::InvalidResponse {
                endpoint: Endpoint::RefreshToken.path(),
            }
        })?;
        let access_token = value["data"]["access_token"].as_str().map(str::to_owned);
        let new_refresh_token = value["data"]["refresh_token"].as_str().map(str::to_owned);
        let expires_at = value["data"]["expires_in"]
            .as_i64()
            .map(|secs| Utc::now() + ChronoDuration::seconds(secs));
        let Some(access_token) = access_token else {
            return Err(ApiError::InvalidResponse {
                endpoint: Endpoint::RefreshToken.path(),
            });
        };
        self.session
            .apply_refreshed_credentials(
                Some(format!("Bearer {access_token}")),
                new_refresh_token,
                expires_at,
            )
            .await;
        Ok(())
    }

    async fn parse_or_expire<T>(&self, result: Result<T, ApiError>) -> Result<T, ApiError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if error.is_auth_invalid() {
                    self.session.mark_expired().await;
                }
                Err(error)
            }
        }
    }

    async fn request_bytes(
        &self,
        endpoint: Endpoint,
        query: &[(&str, &str)],
    ) -> Result<bytes::Bytes, ApiError> {
        let credentials = self.session.request_credentials().await?;
        let default_origin = self
            .default_api_origin
            .lock()
            .map(|origin| origin.clone())
            .unwrap_or_else(|_| Endpoint::API_ORIGIN.to_owned());
        let origin = credentials
            .request_context
            .api_origin
            .as_deref()
            .unwrap_or(default_origin.as_str());
        let url = format!("{}{}", origin.trim_end_matches('/'), endpoint.path());
        let mut request = self
            .http
            .get(url)
            .header(REFERER, dashboard_referer(origin))
            .query(query);
        request = apply_request_context(request, &credentials.request_context);
        if let Some(cookie_header) = credentials.cookie_header {
            request = request.header(COOKIE, cookie_header);
        }
        if let Some(authorization_header) = credentials.authorization_header {
            request = request.header(AUTHORIZATION, authorization_header);
        }

        let response = request.send().await.map_err(classify_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let error = ApiError::from_http_status(status.as_u16());
            if error.is_auth_invalid() {
                self.session.mark_expired().await;
            }
            return Err(error);
        }

        let set_cookie_headers = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let body = response.bytes().await.map_err(classify_reqwest_error)?;
        self.session
            .apply_set_cookie_headers(&set_cookie_headers)
            .await;
        Ok(body)
    }

    /// Candidate login verification intentionally does not call
    /// `apply_set_cookie_headers`: it has not been accepted into the Rust
    /// session yet, so no unverified material may be persisted or mutated.
    async fn request_bytes_with_credentials(
        &self,
        endpoint: Endpoint,
        query: &[(&str, &str)],
        cookie_header: Option<&str>,
        authorization_header: Option<&str>,
        api_origin: Option<&str>,
        context: Option<&RequestContext>,
    ) -> Result<bytes::Bytes, ApiError> {
        let default_origin = self
            .default_api_origin
            .lock()
            .map(|origin| origin.clone())
            .unwrap_or_else(|_| Endpoint::API_ORIGIN.to_owned());
        let origin = api_origin.unwrap_or(default_origin.as_str());
        let url = format!("{}{}", origin.trim_end_matches('/'), endpoint.path());
        let referer = dashboard_referer(origin);
        let mut request = self.http.get(url).header(REFERER, referer).query(query);
        if let Some(context) = context {
            request = apply_request_context(request, context);
        } else {
            request = request.header(ACCEPT, "application/json");
        }
        if let Some(cookie_header) = cookie_header {
            request = request.header(COOKIE, cookie_header);
        }
        if let Some(authorization_header) = authorization_header {
            request = request.header(AUTHORIZATION, authorization_header);
        }
        let response = request.send().await.map_err(classify_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(ApiError::from_http_status(status.as_u16()));
        }
        response.bytes().await.map_err(classify_reqwest_error)
    }
}

fn apply_request_context(
    mut request: reqwest::RequestBuilder,
    context: &RequestContext,
) -> reqwest::RequestBuilder {
    if let Some(value) = &context.user_agent {
        request = request.header(USER_AGENT, value);
    }
    if let Some(value) = &context.accept {
        request = request.header(ACCEPT, value);
    } else {
        request = request.header(ACCEPT, "application/json");
    }
    if let Some(value) = &context.accept_language {
        request = request.header(ACCEPT_LANGUAGE, value);
    }
    for (name, value) in [
        ("sec-fetch-dest", &context.sec_fetch_dest),
        ("sec-fetch-mode", &context.sec_fetch_mode),
        ("sec-fetch-site", &context.sec_fetch_site),
    ] {
        if let Some(value) = value {
            request = request.header(name, value);
        }
    }
    request
}

fn dashboard_referer(api_origin: &str) -> String {
    format!(
        "{}/dashboard",
        api_origin.trim_end_matches("/api/v1").trim_end_matches('/')
    )
}

fn resolve_request_results(
    results: Vec<Result<bytes::Bytes, ApiError>>,
) -> Result<Vec<bytes::Bytes>, ApiError> {
    // If concurrent calls yield both a network error and an authentication
    // error, the explicit authentication result wins. Otherwise a network
    // failure could incorrectly preserve an expired login state.
    if let Some(index) = results
        .iter()
        .position(|result| result.as_ref().is_err_and(ApiError::is_auth_invalid))
    {
        return match results.into_iter().nth(index) {
            Some(Err(error)) => Err(error),
            _ => Err(ApiError::HttpStatus),
        };
    }
    results.into_iter().collect()
}

fn classify_reqwest_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else {
        ApiError::Network
    }
}
