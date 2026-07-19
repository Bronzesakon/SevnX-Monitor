use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::error::ApiError;

/// The verified SevnX API success code. The parser checks this before it ever
/// deserializes `data`, as required by the product contract.
const SUCCESS_CODE: i64 = 0;

pub(crate) fn parse_success_envelope<T>(body: &[u8], endpoint: &'static str) -> Result<T, ApiError>
where
    T: DeserializeOwned,
{
    let root: Value =
        serde_json::from_slice(body).map_err(|_| ApiError::InvalidResponse { endpoint })?;
    let mut object = into_object(root, endpoint)?;

    let code = object
        .remove("code")
        .ok_or(ApiError::InvalidResponse { endpoint })
        .and_then(|value| parse_code(value, endpoint))?;

    if code != SUCCESS_CODE {
        // Do not retain or report `message`: server messages are untrusted and
        // can include information that is not safe for logs or IPC.
        return Err(ApiError::BusinessRejected { code });
    }

    let data = object
        .remove("data")
        .ok_or(ApiError::InvalidResponse { endpoint })?;
    serde_json::from_value(data).map_err(|_| ApiError::InvalidResponse { endpoint })
}

fn into_object(root: Value, endpoint: &'static str) -> Result<Map<String, Value>, ApiError> {
    match root {
        Value::Object(object) => Ok(object),
        _ => Err(ApiError::InvalidResponse { endpoint }),
    }
}

fn parse_code(value: Value, endpoint: &'static str) -> Result<i64, ApiError> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .ok_or(ApiError::InvalidResponse { endpoint }),
        _ => Err(ApiError::InvalidResponse { endpoint }),
    }
}
