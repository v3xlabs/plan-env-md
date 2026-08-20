use poem::web::cookie::CookieJar;
use poem::web::{Data, RealIp};
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, Object, OpenApi};
use sqlx::SqlitePool;

use crate::api::{internal, is_unique_violation};
use crate::auth::{self, Auth, SESSION_COOKIE, SessionAuth};
use crate::rate_limit::RateLimiter;

pub struct AuthApi;

#[derive(Object)]
struct RegisterRequest {
    username: String,
    password: String,
    invite_code: Option<String>,
}

#[derive(Object)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Object)]
struct UserBody {
    id: i64,
    username: String,
    is_admin: bool,
}

#[derive(ApiResponse)]
enum RegisterResponse {
    #[oai(status = 200)]
    Ok(Json<UserBody>),
    /// Username or password does not meet the requirements
    #[oai(status = 422)]
    Invalid(PlainText<String>),
    /// Invite code missing, unknown, or already used
    #[oai(status = 403)]
    BadInvite,
    /// Username already taken
    #[oai(status = 409)]
    UsernameTaken,
    /// Too many attempts from this address; try again later
    #[oai(status = 429)]
    TooManyRequests,
}

#[derive(ApiResponse)]
enum LoginResponse {
    #[oai(status = 200)]
    Ok(Json<UserBody>),
    /// Unknown username or wrong password
    #[oai(status = 401)]
    Unauthorized,
    /// Too many attempts from this address; try again later
    #[oai(status = 429)]
    TooManyRequests,
}

#[derive(ApiResponse)]
enum LogoutResponse {
    #[oai(status = 204)]
    Done,
}

#[OpenApi]
impl AuthApi {
    /// Create an account. The first account needs no invite code and becomes
    /// admin; every later one consumes an unused invite code.
    #[oai(path = "/auth/register", method = "post")]
    async fn register(
        &self,
        pool: Data<&SqlitePool>,
        limiter: Data<&RateLimiter>,
        real_ip: RealIp,
        cookies: &CookieJar,
        body: Json<RegisterRequest>,
    ) -> poem::Result<RegisterResponse> {
        if !limiter.0.allow(real_ip.0) {
            return Ok(RegisterResponse::TooManyRequests);
        }
        let RegisterRequest {
            username,
            password,
            invite_code,
        } = body.0;

        if !valid_username(&username) {
            return Ok(RegisterResponse::Invalid(PlainText(
                "username must match [a-z0-9-]{3,32}".to_string(),
            )));
        }
        if password.len() < 8 {
            return Ok(RegisterResponse::Invalid(PlainText(
                "password must be at least 8 characters".to_string(),
            )));
        }

        let password_hash = auth::hash_password(password).await;

        let mut tx = pool.0.begin().await.map_err(internal)?;
        let user_count = sqlx::query_scalar!(r#"SELECT COUNT(*) as "count: i64" FROM users"#)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
        let is_admin = user_count == 0;

        // validate the invite before touching the username, so that a caller
        // without a valid invite cannot probe which usernames exist
        let invite_id = if is_admin {
            None
        } else {
            let Some(code) = invite_code else {
                return Ok(RegisterResponse::BadInvite);
            };
            let invite = sqlx::query_scalar!(
                r#"SELECT id as "id!: i64" FROM invite_codes WHERE code = ? AND used_by IS NULL"#,
                code
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal)?;
            match invite {
                Some(id) => Some(id),
                None => return Ok(RegisterResponse::BadInvite),
            }
        };

        let inserted = sqlx::query!(
            "INSERT INTO users (username, password_hash, is_admin) VALUES (?, ?, ?)",
            username,
            password_hash,
            is_admin
        )
        .execute(&mut *tx)
        .await;
        let user_id = match inserted {
            Ok(done) => done.last_insert_rowid(),
            Err(e) if is_unique_violation(&e) => return Ok(RegisterResponse::UsernameTaken),
            Err(e) => return Err(internal(e)),
        };

        if let Some(invite_id) = invite_id {
            let claimed = sqlx::query!(
                "UPDATE invite_codes SET used_by = ?, used_at = datetime('now')
                 WHERE id = ? AND used_by IS NULL",
                user_id,
                invite_id
            )
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
            if claimed.rows_affected() == 0 {
                return Ok(RegisterResponse::BadInvite);
            }
        }
        tx.commit().await.map_err(internal)?;

        let token = auth::create_session(pool.0, user_id)
            .await
            .map_err(internal)?;
        cookies.add(auth::session_cookie(token));

        Ok(RegisterResponse::Ok(Json(UserBody {
            id: user_id,
            username,
            is_admin,
        })))
    }

    /// Log in with username and password.
    #[oai(path = "/auth/login", method = "post")]
    async fn login(
        &self,
        pool: Data<&SqlitePool>,
        limiter: Data<&RateLimiter>,
        real_ip: RealIp,
        cookies: &CookieJar,
        body: Json<LoginRequest>,
    ) -> poem::Result<LoginResponse> {
        if !limiter.0.allow(real_ip.0) {
            return Ok(LoginResponse::TooManyRequests);
        }
        let row = sqlx::query!(
            r#"SELECT id as "id!: i64", username as "username!: String",
                      password_hash as "password_hash!: String", is_admin as "is_admin!: bool"
               FROM users WHERE username = ?"#,
            body.0.username
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;

        let Some(row) = row else {
            return Ok(LoginResponse::Unauthorized);
        };
        if !auth::verify_password(body.0.password, row.password_hash).await {
            return Ok(LoginResponse::Unauthorized);
        }

        let _ = sqlx::query!("DELETE FROM sessions WHERE expires_at <= datetime('now')")
            .execute(pool.0)
            .await;

        let token = auth::create_session(pool.0, row.id)
            .await
            .map_err(internal)?;
        cookies.add(auth::session_cookie(token));

        Ok(LoginResponse::Ok(Json(UserBody {
            id: row.id,
            username: row.username,
            is_admin: row.is_admin,
        })))
    }

    /// End the current session.
    #[oai(path = "/auth/logout", method = "post")]
    async fn logout(
        &self,
        pool: Data<&SqlitePool>,
        cookies: &CookieJar,
        _session: SessionAuth,
    ) -> poem::Result<LogoutResponse> {
        if let Some(cookie) = cookies.get(SESSION_COOKIE) {
            let hash = auth::sha256(cookie.value_str());
            let _ = sqlx::query!("DELETE FROM sessions WHERE token_hash = ?", hash)
                .execute(pool.0)
                .await;
        }
        cookies.remove(SESSION_COOKIE);
        Ok(LogoutResponse::Done)
    }

    /// The authenticated account.
    #[oai(path = "/auth/me", method = "get")]
    async fn me(&self, auth: Auth) -> Json<UserBody> {
        let user = auth.user();
        Json(UserBody {
            id: user.id,
            username: user.username.clone(),
            is_admin: user.is_admin,
        })
    }
}

fn valid_username(username: &str) -> bool {
    (3..=32).contains(&username.len())
        && username
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
