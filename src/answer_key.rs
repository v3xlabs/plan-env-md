//! Scoped keys that let the in-document answer widget write answers.
//!
//! The document runs in a sandboxed opaque origin, so its site for cookies is
//! null and the SameSite=Lax session cookie is never sent. A key in an
//! Authorization header is not a cookie, so the request is uncredentialed and
//! the CORS response needs no Allow-Credentials.
//!
//! Agent-authored script shares the DOM with the widget and can read the key.
//! That is why the key carries no read capability and is bound to one document
//! and a short expiry: the worst it permits is a document making itself look
//! answered. A document has exactly one owner, so the document id alone decides
//! who the answers belong to and the key needs no user.

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const PREFIX: &str = "planq_";
pub const TTL_SECONDS: i64 = 4 * 3600;

pub fn mint(secret: &str, document_id: i64) -> String {
    let expiry = unix_now() + TTL_SECONDS;
    let mac = hex_encode(&mac_for(secret, document_id, expiry).finalize().into_bytes());
    format!("{PREFIX}{document_id}.{expiry}.{mac}")
}

/// The document the key may write, or `None` for any malformed, expired or
/// wrongly signed key.
pub fn verify(secret: &str, key: &str) -> Option<i64> {
    let body = key.strip_prefix(PREFIX)?;
    let mut parts = body.splitn(3, '.');
    let document_id: i64 = parts.next()?.parse().ok()?;
    let expiry: i64 = parts.next()?.parse().ok()?;
    let mac_bytes = hex_decode(parts.next()?)?;

    if expiry <= unix_now() {
        return None;
    }
    mac_for(secret, document_id, expiry)
        .verify_slice(&mac_bytes)
        .ok()?;
    Some(document_id)
}

fn mac_for(secret: &str, document_id: i64, expiry: i64) -> Hmac<Sha256> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(format!("answer.{document_id}.{expiry}").as_bytes());
    mac
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_secs() as i64
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}
