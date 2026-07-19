//! Windows DPAPI helpers for the one encrypted credential file.
//!
//! DPAPI defaults to the current Windows user when no machine scope flag is
//! passed. The helpers never format input or output bytes, so credential
//! material cannot escape through an error or log message.

use super::StorageError;

#[cfg(windows)]
pub fn protect(plain: &[u8]) -> Result<Vec<u8>, StorageError> {
    use windows::{
        Win32::Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
        },
        core::w,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(plain.len()).map_err(|_| StorageError::Protect)?,
        pbData: plain.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // No CRYPTPROTECT_LOCAL_MACHINE flag is supplied: encryption is bound to
    // the current Windows user as required by the product specification.
    unsafe {
        CryptProtectData(
            &input,
            w!("SevnX Monitor session"),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| StorageError::Protect)?;
    }
    take_blob(output)
}

#[cfg(windows)]
pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, StorageError> {
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(cipher.len()).map_err(|_| StorageError::Unprotect)?,
        pbData: cipher.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| StorageError::Unprotect)?;
    }
    take_blob(output)
}

#[cfg(windows)]
fn take_blob(
    blob: windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
) -> Result<Vec<u8>, StorageError> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};

    if blob.cbData > 0 && blob.pbData.is_null() {
        return Err(StorageError::Unprotect);
    }
    let output = if blob.cbData == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() }
    };
    // DPAPI allocates the result with LocalAlloc. Always free it immediately
    // after copying, including when the payload is empty.
    unsafe {
        if !blob.pbData.is_null() {
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
    }
    Ok(output)
}

#[cfg(not(windows))]
pub fn protect(_plain: &[u8]) -> Result<Vec<u8>, StorageError> {
    Err(StorageError::Protect)
}

#[cfg(not(windows))]
pub fn unprotect(_cipher: &[u8]) -> Result<Vec<u8>, StorageError> {
    Err(StorageError::Unprotect)
}

#[cfg(all(test, windows))]
mod tests {
    use super::{protect, unprotect};

    #[test]
    fn dpapi_round_trip_is_not_plaintext() {
        let plain = b"test-cookie-must-not-be-stored-as-json";
        let encrypted = protect(plain).expect("DPAPI protects data");
        assert_ne!(encrypted, plain);
        assert!(!encrypted.windows(plain.len()).any(|window| window == plain));
        assert_eq!(unprotect(&encrypted).expect("DPAPI restores data"), plain);
    }
}
