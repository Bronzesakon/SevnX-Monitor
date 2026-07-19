use std::{future::Future, pin::Pin, sync::Arc};

use chrono::Utc;
use tokio::sync::{Mutex, watch};

use crate::{
    api::{
        client::{SevnxApiClient, VerifiedUsageQuery},
        error::{ApiError, PublicError},
    },
    model::{AppSnapshot, AppState, AuthStatus, DashboardSnapshot, UsageRange, UsageSnapshot},
};

type SourceFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ApiError>> + Send + 'a>>;

/// Allows the refresh gate to be verified without a network connection and
/// keeps scheduling separate from HTTP implementation details.
pub trait RefreshSource: Send + Sync {
    fn fetch_dashboard(&self) -> SourceFuture<'_, DashboardSnapshot>;
    fn fetch_usage(
        &self,
        range: UsageRange,
        query: VerifiedUsageQuery,
    ) -> SourceFuture<'_, UsageSnapshot>;
}

impl RefreshSource for SevnxApiClient {
    fn fetch_dashboard(&self) -> SourceFuture<'_, DashboardSnapshot> {
        Box::pin(self.fetch_dashboard())
    }

    fn fetch_usage(
        &self,
        range: UsageRange,
        query: VerifiedUsageQuery,
    ) -> SourceFuture<'_, UsageSnapshot> {
        Box::pin(self.fetch_usage(range, query))
    }
}

pub struct RefreshCoordinator<S = SevnxApiClient>
where
    S: RefreshSource,
{
    source: Arc<S>,
    state: AppState,
    gate: Arc<RefreshGate>,
}

impl<S> Clone for RefreshCoordinator<S>
where
    S: RefreshSource,
{
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            state: self.state.clone(),
            gate: self.gate.clone(),
        }
    }
}

struct RefreshGate {
    state: Mutex<RefreshGateState>,
    completed: watch::Sender<u64>,
}

#[derive(Default)]
struct RefreshGateState {
    in_flight: bool,
    generation: u64,
    last_result: Option<Result<(), PublicError>>,
}

impl<S> RefreshCoordinator<S>
where
    S: RefreshSource,
{
    pub fn new(source: Arc<S>, state: AppState) -> Self {
        let (completed, _) = watch::channel(0_u64);
        Self {
            source,
            state,
            gate: Arc::new(RefreshGate {
                state: Mutex::new(RefreshGateState::default()),
                completed,
            }),
        }
    }

    /// Starts one complete refresh or joins the one already in progress. A
    /// concurrent trigger intentionally receives the same completed snapshot,
    /// even if it requested a newer range; the next explicit refresh can then
    /// apply that range without creating a request burst.
    pub async fn refresh_all(
        &self,
        range: UsageRange,
        query: VerifiedUsageQuery,
    ) -> Result<AppSnapshot, PublicError> {
        let mut receiver = self.gate.completed.subscribe();
        let (owns_refresh, observed_generation) = {
            let mut gate = self.gate.state.lock().await;
            if gate.in_flight {
                (false, gate.generation)
            } else {
                gate.in_flight = true;
                (true, gate.generation)
            }
        };

        if !owns_refresh {
            loop {
                if receiver.changed().await.is_err() {
                    return Err(PublicError::not_ready());
                }
                let gate = self.gate.state.lock().await;
                if gate.generation != observed_generation {
                    return gate
                        .last_result
                        .clone()
                        .unwrap_or_else(|| Err(PublicError::not_ready()))
                        .map(|()| self.state.snapshot());
                }
            }
        }

        let result = self.run_refresh(range, query).await;
        let gate_result = result.as_ref().map(|_| ()).map_err(Clone::clone);
        let generation = {
            let mut gate = self.gate.state.lock().await;
            gate.in_flight = false;
            gate.generation = gate.generation.wrapping_add(1);
            gate.last_result = Some(gate_result);
            gate.generation
        };
        let _ = self.gate.completed.send(generation);
        result
    }

    async fn run_refresh(
        &self,
        range: UsageRange,
        query: VerifiedUsageQuery,
    ) -> Result<AppSnapshot, PublicError> {
        self.state.mark_refreshing();
        let (dashboard_result, usage_result) = tokio::join!(
            self.source.fetch_dashboard(),
            self.source.fetch_usage(range, query)
        );
        let result = resolve_refresh_results(dashboard_result, usage_result);
        match result {
            Ok((dashboard, usage)) => {
                self.state.set_auth_status(AuthStatus::Authenticated);
                self.state.replace_success(dashboard, usage, Utc::now());
                Ok(self.state.snapshot())
            }
            Err(error) => {
                let public = error.to_public();
                if error.is_auth_invalid() {
                    self.state.mark_auth_expired(public.clone());
                } else {
                    self.state.mark_request_failure(public.clone());
                }
                Err(public)
            }
        }
    }
}

fn resolve_refresh_results(
    dashboard: Result<DashboardSnapshot, ApiError>,
    usage: Result<UsageSnapshot, ApiError>,
) -> Result<(DashboardSnapshot, UsageSnapshot), ApiError> {
    match (dashboard, usage) {
        (Err(dashboard), Err(usage)) => {
            if dashboard.is_auth_invalid() {
                Err(dashboard)
            } else if usage.is_auth_invalid() {
                Err(usage)
            } else {
                Err(dashboard)
            }
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(dashboard), Ok(usage)) => Ok((dashboard, usage)),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use chrono::Utc;

    use super::{RefreshCoordinator, RefreshSource, SourceFuture};
    use crate::{
        api::{client::VerifiedUsageQuery, error::ApiError},
        model::{
            ApiKeySummary, AppState, DashboardSnapshot, PerformanceSummary, RequestSummary,
            SpendSummary, TokenSummary, UsageRange, UsageSnapshot, UsageSummary,
        },
    };

    struct FakeSource {
        dashboard_calls: AtomicUsize,
        usage_calls: AtomicUsize,
    }

    impl FakeSource {
        fn new() -> Self {
            Self {
                dashboard_calls: AtomicUsize::new(0),
                usage_calls: AtomicUsize::new(0),
            }
        }
    }

    impl RefreshSource for FakeSource {
        fn fetch_dashboard(&self) -> SourceFuture<'_, DashboardSnapshot> {
            self.dashboard_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(25)).await;
                Ok(DashboardSnapshot {
                    balance: None,
                    api_keys: ApiKeySummary::default(),
                    today_requests: RequestSummary::default(),
                    today_spend: SpendSummary::default(),
                    today_tokens: TokenSummary::default(),
                    cumulative_tokens: TokenSummary::default(),
                    performance: PerformanceSummary::default(),
                    average_response: None,
                    fetched_at: Utc::now(),
                })
            })
        }

        fn fetch_usage(
            &self,
            range: UsageRange,
            _query: VerifiedUsageQuery,
        ) -> SourceFuture<'_, UsageSnapshot> {
            self.usage_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(25)).await;
                Ok(UsageSnapshot {
                    range,
                    summary: UsageSummary::default(),
                    token_trend: Vec::new(),
                    models: Vec::new(),
                    groups: Vec::new(),
                    endpoints: Vec::new(),
                    model_pie: Vec::new(),
                    fetched_at: Utc::now(),
                })
            })
        }
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_full_refresh() {
        let source = Arc::new(FakeSource::new());
        let state = AppState::default();
        let coordinator = RefreshCoordinator::new(source.clone(), state);
        let query = VerifiedUsageQuery::new("2026-07-16", "2026-07-17", "hour")
            .expect("valid verified query");

        let first = {
            let coordinator = coordinator.clone();
            let query = query.clone();
            tokio::spawn(async move {
                coordinator
                    .refresh_all(UsageRange::Last24Hours, query)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(2)).await;
        let second = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .refresh_all(UsageRange::Last24Hours, query)
                    .await
            })
        };

        assert!(first.await.expect("first task").is_ok());
        assert!(second.await.expect("second task").is_ok());
        assert_eq!(source.dashboard_calls.load(Ordering::SeqCst), 1);
        assert_eq!(source.usage_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ordinary_network_failure_keeps_existing_snapshot() {
        struct FailingSource;
        impl RefreshSource for FailingSource {
            fn fetch_dashboard(&self) -> SourceFuture<'_, DashboardSnapshot> {
                Box::pin(async { Err(ApiError::Network) })
            }
            fn fetch_usage(
                &self,
                _range: UsageRange,
                _query: VerifiedUsageQuery,
            ) -> SourceFuture<'_, UsageSnapshot> {
                Box::pin(async { Err(ApiError::Network) })
            }
        }

        let state = AppState::default();
        let coordinator = RefreshCoordinator::new(Arc::new(FailingSource), state.clone());
        let query = VerifiedUsageQuery::new("2026-07-16", "2026-07-17", "hour")
            .expect("valid verified query");
        let error = coordinator
            .refresh_all(UsageRange::Last24Hours, query)
            .await
            .expect_err("network failure");
        assert_eq!(error.code, crate::api::error::PublicErrorCode::Network);
        assert_eq!(state.snapshot().auth, crate::model::AuthStatus::Validating);
    }
}
