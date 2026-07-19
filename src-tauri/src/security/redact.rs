use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

pub const REDACTED: &str = "[已脱敏]";

/// Redact the sensitive key families explicitly banned from logs, diagnostics,
/// and IPC. Matching is deliberately case-insensitive and accepts common
/// snake_case, kebab-case, and camelCase spelling variants.
pub fn redact_text(input: &str) -> String {
    // `SafeLog` prefixes metadata with `detail=`, so an Authorization header
    // is not necessarily at the beginning of a line. Redact the entire
    // Bearer value before applying the generic key/value rules; otherwise the
    // generic matcher would redact only the word "Bearer" and leave its token.
    let authorization_redacted = authorization_value_pattern()
        .replace_all(input, |captures: &regex::Captures<'_>| {
            format!("{}{}", &captures[1], REDACTED)
        })
        .into_owned();
    let header_redacted = header_pattern()
        .replace_all(&authorization_redacted, |captures: &regex::Captures<'_>| {
            format!("{}{}", &captures[1], REDACTED)
        })
        .into_owned();
    let key_value_redacted = key_value_pattern()
        .replace_all(&header_redacted, |captures: &regex::Captures<'_>| {
            let value = &captures[2];
            let replacement = match value.as_bytes().first() {
                Some(b'\"') => format!("\"{REDACTED}\""),
                Some(b'\'') => format!("'{REDACTED}'"),
                _ => REDACTED.to_string(),
            };
            format!("{}{}", &captures[1], replacement)
        })
        .into_owned();
    query_pattern()
        .replace_all(&key_value_redacted, |captures: &regex::Captures<'_>| {
            format!("{}{}", &captures[1], REDACTED)
        })
        .into_owned()
}

/// Recursively redact an already-parsed diagnostic value while keeping its
/// structural shape useful for troubleshooting. Do not call this with an API
/// response that is otherwise going to be retained; raw response bodies must
/// remain transient and unlogged.
pub fn redact_json(value: &Value) -> Value {
    let mut redacted = value.clone();
    redact_json_in_place(&mut redacted);
    redacted
}

pub fn redact_json_in_place(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object.iter_mut() {
                if is_sensitive_key(key) {
                    *nested = Value::String(REDACTED.to_string());
                } else {
                    redact_json_in_place(nested);
                }
            }
        }
        Value::Array(values) => {
            for nested in values {
                redact_json_in_place(nested);
            }
        }
        Value::String(text) => *text = redact_text(text),
        _ => {}
    }
}

pub fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "cookie"
            | "setcookie"
            | "authorization"
            | "password"
            | "secret"
            | "apikey"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "idtoken"
            | "credential"
            | "credentials"
            | "session"
            | "sessionid"
            | "sessiontoken"
    )
}

fn header_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?im)^(\s*(?:set[-_]?cookie|cookie|authorization|password|secret|api[-_]?key|(?:access|refresh|id|session)[-_]?(?:token|id)|session|token|credentials?)\s*:\s*)[^\r\n]*$",
        )
        .expect("valid sensitive-header redaction pattern")
    })
}

fn authorization_value_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)(authorization\s*:\s*)(?:bearer\s+)?[^,;\r\n}\]&]+")
            .expect("valid authorization redaction pattern")
    })
}

fn key_value_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r#"(?i)([\"']?(?:set[-_]?cookie|cookie|authorization|password|secret|api[-_]?key|(?:access|refresh|id|session)[-_]?(?:token|id)|session|token|credentials?)[\"']?\s*[:=]\s*)(\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'|[^,;\s}\]&]+)"#,
        )
        .expect("valid sensitive-key redaction pattern")
    })
}

fn query_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)([?&](?:api[-_]?key|(?:access|refresh|id|session)[-_]?(?:token|id)|session|token|password|secret)=)[^&#\s]*",
        )
        .expect("valid sensitive-query redaction pattern")
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{REDACTED, redact_json, redact_text};

    #[test]
    fn diagnostic_text_never_keeps_test_credentials() {
        let input = concat!(
            "Authorization: Bearer test-token-123\n",
            "Cookie: session=test-cookie-456\n",
            "{\"api_key\":\"test-api-key-789\",\"token\":\"test-json-token\"}\n",
            "https://www.sevnx.one/?access_token=test-access-token"
        );

        let output = redact_text(input);

        for secret in [
            "test-token-123",
            "test-cookie-456",
            "test-api-key-789",
            "test-json-token",
            "test-access-token",
        ] {
            assert!(!output.contains(secret), "leaked {secret}");
        }
        assert!(output.contains(REDACTED));
    }

    #[test]
    fn session_key_variants_are_redacted() {
        let output = redact_text(
            "session_id=test-session-id&sessionToken=test-session-token\nSession: test-session",
        );
        for secret in ["test-session-id", "test-session-token", "test-session"] {
            assert!(!output.contains(secret), "leaked {secret}");
        }
    }

    #[test]
    fn json_redaction_recurses_without_losing_safe_context() {
        let input = json!({
            "endpoint": "/api/v1/usage/stats",
            "nested": { "refreshToken": "test-refresh-token" },
            "items": [{ "password": "test-password" }]
        });

        let output = redact_json(&input);
        assert_eq!(output["endpoint"], "/api/v1/usage/stats");
        assert_eq!(output["nested"]["refreshToken"], REDACTED);
        assert_eq!(output["items"][0]["password"], REDACTED);
    }
}
