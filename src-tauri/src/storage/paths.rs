use std::{env, fs, path::PathBuf};

use super::StorageError;

#[derive(Clone, Debug)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self, StorageError> {
        let local_app_data = env::var_os("LOCALAPPDATA").ok_or(StorageError::Unavailable)?;
        let root = PathBuf::from(local_app_data).join("SevnX Monitor");
        fs::create_dir_all(root.join("auth")).map_err(|_| StorageError::Unavailable)?;
        fs::create_dir_all(root.join("logs")).map_err(|_| StorageError::Unavailable)?;
        fs::create_dir_all(root.join("WebView2")).map_err(|_| StorageError::Unavailable)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn session_file(&self) -> PathBuf {
        self.root.join("auth").join("session.bin")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn webview_profile_dir(&self) -> PathBuf {
        self.root.join("WebView2")
    }
}
