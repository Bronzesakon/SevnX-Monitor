use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::AppSettings;

use super::StorageError;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredBarPosition {
    pub x: i32,
    pub y: i32,
    pub edge: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAlertState {
    pub last_notified_at: Option<DateTime<Utc>>,
    pub was_low: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSettings {
    #[serde(flatten)]
    pub settings: AppSettings,
    #[serde(default)]
    pub bar_position: Option<StoredBarPosition>,
    #[serde(default)]
    pub alert_state: StoredAlertState,
    #[serde(default)]
    pub onboarding_complete: bool,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            settings: AppSettings::default(),
            bar_position: None,
            alert_state: StoredAlertState::default(),
            onboarding_complete: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<PersistedSettings, StorageError> {
        if !self.path.exists() {
            return Ok(PersistedSettings::default());
        }
        let bytes = fs::read(&self.path).map_err(|_| StorageError::Read)?;
        serde_json::from_slice(&bytes).map_err(|_| StorageError::Decode)
    }

    pub fn save(&self, settings: &PersistedSettings) -> Result<(), StorageError> {
        let bytes = serde_json::to_vec_pretty(settings).map_err(|_| StorageError::Write)?;
        atomic_write(&self.path, &bytes)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().ok_or(StorageError::Write)?;
    fs::create_dir_all(parent).map_err(|_| StorageError::Write)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings"),
        std::process::id()
    ));
    let mut file = fs::File::create(&temp).map_err(|_| StorageError::Write)?;
    file.write_all(bytes).map_err(|_| StorageError::Write)?;
    file.sync_all().map_err(|_| StorageError::Write)?;
    fs::rename(&temp, path).map_err(|_| StorageError::Write)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::model::ThemeMode;

    use super::{PersistedSettings, SettingsStore};

    #[test]
    fn settings_round_trip_without_creating_sensitive_fields() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("sevnx-settings-{stamp}"));
        fs::create_dir_all(&root).expect("temporary directory");
        let store = SettingsStore::new(root.join("settings.json"));
        let mut settings = PersistedSettings::default();
        settings.settings.theme = ThemeMode::Dark;
        store.save(&settings).expect("settings save");

        let restored = store.load().expect("settings load");
        assert_eq!(restored.settings.theme, ThemeMode::Dark);
        let document = fs::read_to_string(root.join("settings.json")).expect("document");
        assert!(!document.contains("cookie"));
        assert!(!document.contains("token"));
        fs::remove_dir_all(root).expect("temporary directory cleanup");
    }
}
