use std::time::Duration;

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use poem::Request;
use poem::web::cookie::{Cookie, SameSite};
use poem_openapi::SecurityScheme;
use poem_openapi::auth::{ApiKey, Bearer};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

pub const SESSION_COOKIE: &str = "session";
const SESSION_DAYS: u64 = 30;
const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
}

/// Bearer PAT, for agents.
#[derive(SecurityScheme)]
#[oai(ty = "bearer", checker = "check_bearer")]
pub struct BearerAuth(pub AuthUser);

/// Session cookie, for the management UI.
#[derive(SecurityScheme)]
#[oai(
    ty = "api_key",
    key_name = "session",
    key_in = "cookie",
    checker = "check_session"
)]
pub struct SessionAuth(pub AuthUser);

/// Either authentication path; endpoints that do not care which one.
#[derive(SecurityScheme)]
pub enum Auth {
    Bearer(BearerAuth),
    Session(SessionAuth),
}

impl Auth {
    pub fn user(&self) -> &AuthUser {
        match self {
            Auth::Bearer(BearerAuth(user)) => user,
            Auth::Session(SessionAuth(user)) => user,
        }
    }
}

async fn check_bearer(req: &Request, bearer: Bearer) -> Option<AuthUser> {
    lookup_bearer(req.data::<SqlitePool>()?, &bearer.token).await
}

async fn check_session(req: &Request, api_key: ApiKey) -> Option<AuthUser> {
    session_user(req.data::<SqlitePool>()?, &api_key.key).await
}

async fn lookup_bearer(pool: &SqlitePool, token: &str) -> Option<AuthUser> {
    let hash = sha256(token);
    let row = sqlx::query!(
        r#"SELECT u.id as "id!: i64", u.username as "username!: String", u.is_admin as "is_admin!: bool"
           FROM api_tokens t JOIN users u ON u.id = t.user_id
           WHERE t.token_hash = ?"#,
        hash
    )
    .fetch_optional(pool)
    .await
    .ok()??;

    let _ = sqlx::query!(
        "UPDATE api_tokens SET last_used_at = datetime('now')
         WHERE token_hash = ?
           AND (last_used_at IS NULL OR last_used_at < datetime('now', '-60 seconds'))",
        hash
    )
    .execute(pool)
    .await;

    Some(AuthUser {
        id: row.id,
        username: row.username,
        is_admin: row.is_admin,
    })
}

pub async fn session_user(pool: &SqlitePool, key: &str) -> Option<AuthUser> {
    let hash = sha256(key);
    let row = sqlx::query!(
        r#"SELECT u.id as "id!: i64", u.username as "username!: String", u.is_admin as "is_admin!: bool"
           FROM sessions s JOIN users u ON u.id = s.user_id
           WHERE s.token_hash = ? AND s.expires_at > datetime('now')"#,
        hash
    )
    .fetch_optional(pool)
    .await
    .ok()??;

    Some(AuthUser {
        id: row.id,
        username: row.username,
        is_admin: row.is_admin,
    })
}

/// Resolve the requester on a plain poem route, outside the OpenAPI app.
pub async fn user_from_request(pool: &SqlitePool, req: &Request) -> Option<AuthUser> {
    let bearer = req
        .headers()
        .get(poem::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if let Some(token) = bearer {
        return lookup_bearer(pool, token).await;
    }
    let key = req.cookie().get(SESSION_COOKIE)?.value_str().to_string();
    session_user(pool, &key).await
}

pub async fn create_session(pool: &SqlitePool, user_id: i64) -> Result<String, sqlx::Error> {
    let token = random_base62(43);
    let hash = sha256(&token);
    sqlx::query!(
        "INSERT INTO sessions (token_hash, user_id, expires_at)
         VALUES (?, ?, datetime('now', '+30 days'))",
        hash,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(token)
}

pub fn session_cookie(token: String, secure: bool) -> Cookie {
    let mut cookie = Cookie::new_with_str(SESSION_COOKIE, token);
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(secure);
    cookie.set_max_age(Duration::from_secs(SESSION_DAYS * 24 * 3600));
    cookie
}

pub fn random_base62(len: usize) -> String {
    // accept only bytes below 248 (= 4 * 62) so the modulo stays uniform;
    // plain modulo over 256 values would bias the first 8 alphabet characters
    let mut out = String::with_capacity(len);
    let mut buf = [0u8; 64];
    while out.len() < len {
        getrandom::fill(&mut buf).expect("os rng failed");
        for byte in buf {
            if byte < 248 && out.len() < len {
                out.push(BASE62[(byte % 62) as usize] as char);
            }
        }
    }
    out
}

pub fn sha256(data: &str) -> Vec<u8> {
    Sha256::digest(data.as_bytes()).to_vec()
}

pub async fn hash_password(password: String) -> String {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .expect("argon2 hashing failed")
            .to_string()
    })
    .await
    .expect("hashing task panicked")
}

pub async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash)
            .map(|parsed| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok()
            })
            .unwrap_or(false)
    })
    .await
    .expect("verification task panicked")
}
