/// Verified, read-only endpoints permitted in the released product.
///
/// There is deliberately no variant for `/usage`: the product must never
/// request or retain individual usage records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Endpoint {
    AuthMe,
    DashboardStats,
    DashboardModels,
    DashboardSnapshotV2,
    UsageStats,
    RefreshToken,
}

impl Endpoint {
    pub const API_ORIGIN: &'static str = "https://www.sevnx.lol/api/v1";

    pub const fn path(self) -> &'static str {
        match self {
            Self::AuthMe => "/auth/me",
            Self::DashboardStats => "/usage/dashboard/stats",
            Self::DashboardModels => "/usage/dashboard/models",
            Self::DashboardSnapshotV2 => "/usage/dashboard/snapshot-v2",
            Self::UsageStats => "/usage/stats",
            Self::RefreshToken => "/auth/refresh",
        }
    }

    pub const fn method(self) -> &'static str {
        match self {
            Self::RefreshToken => "POST",
            _ => "GET",
        }
    }

    pub const fn all() -> [Self; 6] {
        [
            Self::AuthMe,
            Self::DashboardStats,
            Self::DashboardModels,
            Self::DashboardSnapshotV2,
            Self::UsageStats,
            Self::RefreshToken,
        ]
    }
}
