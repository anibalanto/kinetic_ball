use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Shared secret for HMAC auth — must match the server.
/// Override at build time with `KB_HMAC_SECRET` env var.
const HMAC_SECRET: &[u8] = match option_env!("KB_HMAC_SECRET") {
    Some(s) => s.as_bytes(),
    None => b"kinetic-ball-v1-shared-hmac-secret-2024",
};

/// Client version baked in at compile time.
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compute HMAC-SHA256 hex digest for `"{version}:{timestamp}"`.
fn compute_hmac(version: &str, timestamp: u64, secret: &[u8]) -> String {
    let message = format!("{}:{}", version, timestamp);
    let mut mac =
        HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Returns the three HMAC auth headers that must be sent with every API request.
///
/// - `X-Client-Version` — semver string
/// - `X-Client-Time` — current unix time in minutes
/// - `X-Client-Token` — HMAC-SHA256 hex digest
pub fn auth_headers() -> Vec<(String, String)> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 60;

    let token = compute_hmac(CLIENT_VERSION, timestamp, HMAC_SECRET);

    vec![
        ("X-Client-Version".to_string(), CLIENT_VERSION.to_string()),
        ("X-Client-Time".to_string(), timestamp.to_string()),
        ("X-Client-Token".to_string(), token),
    ]
}
