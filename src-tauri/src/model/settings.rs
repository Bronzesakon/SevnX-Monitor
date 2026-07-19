use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AlertInterval {
    #[serde(rename = "15m")]
    #[default]
    FifteenMinutes,
    #[serde(rename = "30m")]
    ThirtyMinutes,
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "6h")]
    SixHours,
}

impl AlertInterval {
    pub const fn seconds(self) -> i64 {
        match self {
            Self::FifteenMinutes => 15 * 60,
            Self::ThirtyMinutes => 30 * 60,
            Self::OneHour => 60 * 60,
            Self::SixHours => 6 * 60 * 60,
        }
    }
}

/// The complete settings object that may cross IPC. It deliberately contains
/// no session material, API response data, or diagnostic details.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: ThemeMode,
    pub auto_refresh: bool,
    pub low_balance_alert: bool,
    pub low_balance_threshold: Decimal,
    pub alert_interval: AlertInterval,
    pub bar_visible: bool,
    pub autostart: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            auto_refresh: true,
            low_balance_alert: false,
            low_balance_threshold: Decimal::new(5, 0),
            alert_interval: AlertInterval::FifteenMinutes,
            bar_visible: true,
            autostart: false,
        }
    }
}

impl AppSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.low_balance_threshold.is_sign_negative() {
            return Err("余额阈值不能小于零");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AlertInterval, AppSettings};

    #[test]
    fn defaults_match_the_frozen_product_specification() {
        let settings = AppSettings::default();
        assert!(settings.auto_refresh);
        assert!(settings.bar_visible);
        assert!(!settings.low_balance_alert);
        assert_eq!(settings.low_balance_threshold.to_string(), "5");
        assert_eq!(settings.alert_interval, AlertInterval::FifteenMinutes);
    }

    #[test]
    fn settings_reject_a_negative_low_balance_threshold() {
        let mut settings = AppSettings::default();
        settings.low_balance_threshold.set_sign_negative(true);
        assert!(settings.validate().is_err());
    }
}
