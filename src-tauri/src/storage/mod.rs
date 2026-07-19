mod paths;
mod settings_store;

pub mod dpapi;

pub use paths::AppPaths;
pub use settings_store::{PersistedSettings, SettingsStore, StoredAlertState, StoredBarPosition};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("local application storage is unavailable")]
    Unavailable,
    #[error("local application storage could not be read")]
    Read,
    #[error("local application storage could not be written")]
    Write,
    #[error("local application storage could not be decoded")]
    Decode,
    #[error("local application storage could not be protected")]
    Protect,
    #[error("local application storage could not be unprotected")]
    Unprotect,
}
