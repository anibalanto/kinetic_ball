use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

/// Shared secret for HMAC validation.
/// Override at build time with `KB_HMAC_SECRET` env var.
const HMAC_SECRET: &[u8] = match option_env!("KB_HMAC_SECRET") {
    Some(s) => s.as_bytes(),
    None => b"kinetic-ball-v1-shared-hmac-secret-2024",
};

/// Compute HMAC-SHA256 hex digest for `"{version}:{timestamp}"`.
fn compute_hmac(version: &str, timestamp: u64, secret: &[u8]) -> String {
    let message = format!("{}:{}", version, timestamp);
    let mut mac =
        HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Validate an HMAC token against the expected value.
fn validate_hmac(version: &str, timestamp: u64, token: &str, secret: &[u8]) -> bool {
    let expected = compute_hmac(version, timestamp, secret);
    // Constant-time comparison via hmac crate
    let mut mac =
        HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    let message = format!("{}:{}", version, timestamp);
    mac.update(message.as_bytes());
    mac.verify_slice(&hex::decode(token).unwrap_or_default())
        .is_ok()
}

/// Parse a semver string `"major.minor.patch"` into a tuple.
fn parse_version(s: &str) -> Option<(u16, u16, u16)> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Returns true if `client >= min`.
fn is_version_compatible(client: &str, min: &str) -> bool {
    match (parse_version(client), parse_version(min)) {
        (Some(c), Some(m)) => c >= m,
        _ => false,
    }
}

/// Axum middleware that validates HMAC + version on every request.
///
/// Expected headers:
///   - `X-Client-Version`: semver string
///   - `X-Client-Time`: unix minutes (seconds / 60)
///   - `X-Client-Token`: hex-encoded HMAC-SHA256
pub async fn version_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();

    // 1. Extract required headers
    let version = match headers.get("X-Client-Version").and_then(|v| v.to_str().ok()) {
        Some(v) => v.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing X-Client-Version header",
            )
                .into_response();
        }
    };

    let timestamp_str =
        match headers.get("X-Client-Time").and_then(|v| v.to_str().ok()) {
            Some(t) => t.to_string(),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    "Missing X-Client-Time header",
                )
                    .into_response();
            }
        };

    let token = match headers.get("X-Client-Token").and_then(|v| v.to_str().ok()) {
        Some(t) => t.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing X-Client-Token header",
            )
                .into_response();
        }
    };

    // 2. Parse timestamp and check within ±5 minutes
    let timestamp: u64 = match timestamp_str.parse() {
        Ok(t) => t,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid X-Client-Time")
                .into_response();
        }
    };

    let now_minutes = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 60;

    let diff = if now_minutes > timestamp {
        now_minutes - timestamp
    } else {
        timestamp - now_minutes
    };

    if diff > 5 {
        return (
            StatusCode::UNAUTHORIZED,
            "Request timestamp out of range (±5 min)",
        )
            .into_response();
    }

    // 3. Validate HMAC
    if !validate_hmac(&version, timestamp, &token, HMAC_SECRET) {
        return (StatusCode::UNAUTHORIZED, "Invalid client token")
            .into_response();
    }

    // 4. Check version compatibility
    if !is_version_compatible(&version, &state.min_version) {
        return (
            StatusCode::UPGRADE_REQUIRED,
            format!(
                "Client version {} is below minimum required {}",
                version, state.min_version
            ),
        )
            .into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_and_validate_hmac() {
        let version = "0.7.1";
        let timestamp = 17388864u64;
        let secret = b"test-secret";

        let token = compute_hmac(version, timestamp, secret);
        assert!(validate_hmac(version, timestamp, &token, secret));
        assert!(!validate_hmac(version, timestamp, "bad-token", secret));
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("0.7.1"), Some((0, 7, 1)));
        assert_eq!(parse_version("1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version("1.2"), None);
    }

    #[test]
    fn test_version_compatibility() {
        assert!(is_version_compatible("0.7.1", "0.7.1"));
        assert!(is_version_compatible("0.8.0", "0.7.1"));
        assert!(is_version_compatible("1.0.0", "0.7.1"));
        assert!(!is_version_compatible("0.6.0", "0.7.1"));
        assert!(!is_version_compatible("0.7.0", "0.7.1"));
    }
}
