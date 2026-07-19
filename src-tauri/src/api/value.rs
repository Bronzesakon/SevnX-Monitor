use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Accept only JSON number or string representations of decimal values. Null
/// and absent fields become `None`; all other shapes are schema errors.
pub(crate) fn deserialize_optional_decimal<'de, D>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::Number(number)) => Decimal::from_str(&number.to_string())
            .map(Some)
            .map_err(serde::de::Error::custom),
        Some(Value::String(text)) if text.trim() == text && !text.is_empty() => {
            Decimal::from_str(&text)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        _ => Err(serde::de::Error::custom("expected a decimal or null")),
    }
}

/// IDs are only retained for parser validation and are never sent through
/// IPC. SevnX can represent a group ID as either a string or an integer.
pub(crate) fn deserialize_optional_identifier<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(Value::Number(value)) if value.as_i64().is_some() => Ok(Some(value.to_string())),
        _ => Err(serde::de::Error::custom("expected an identifier or null")),
    }
}
