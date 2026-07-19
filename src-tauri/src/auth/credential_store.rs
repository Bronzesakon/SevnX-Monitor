use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use zeroize::Zeroize;

use crate::storage::{StorageError, dpapi};

use super::session::PersistedSession;

/// The only persistent credential repository. Its on-disk representation is
/// DPAPI ciphertext, never JSON or a browser profile database.
#[derive(Clone, Debug)]
pub struct CredentialStore {
    path: PathBuf,
}

impl CredentialStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn load(&self) -> Result<Option<PersistedSession>, StorageError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let cipher = fs::read(&self.path).map_err(|_| StorageError::Read)?;
        let mut plain = dpapi::unprotect(&cipher)?;
        let decoded = serde_json::from_slice(&plain).map_err(|_| StorageError::Decode);
        plain.zeroize();
        decoded.map(Some)
    }

    pub(crate) fn save(&self, session: &PersistedSession) -> Result<(), StorageError> {
        let mut plain = serde_json::to_vec(session).map_err(|_| StorageError::Protect)?;
        let protected = dpapi::protect(&plain);
        plain.zeroize();
        let cipher = protected?;
        atomic_write(&self.path, &cipher)
    }

    pub(crate) fn delete(&self) -> Result<(), StorageError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(StorageError::Write),
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().ok_or(StorageError::Write)?;
    fs::create_dir_all(parent).map_err(|_| StorageError::Write)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("session"),
        std::process::id()
    ));
    let mut file = fs::File::create(&temporary).map_err(|_| StorageError::Write)?;
    file.write_all(bytes).map_err(|_| StorageError::Write)?;
    file.sync_all().map_err(|_| StorageError::Write)?;
    fs::rename(&temporary, path).map_err(|_| StorageError::Write)
}
