use poem::web::Data;
use poem_openapi::param::Path;
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, Object, OpenApi};
use sqlx::SqlitePool;

use crate::api::internal;
use crate::auth::{self, SessionAuth};

pub struct TokensApi;

#[derive(Object)]
struct TokenBody {
    id: i64,
    name: String,
    token_prefix: String,
    created_at: String,
    last_used_at: Option<String>,
}

#[derive(Object)]
struct CreateTokenRequest {
    name: String,
}

#[derive(Object)]
struct CreatedTokenBody {
    id: i64,
    name: String,
    /// The full token. Shown exactly once; only its hash is stored.
    token: String,
}

#[derive(ApiResponse)]
enum CreateTokenResponse {
    #[oai(status = 200)]
    Ok(Json<CreatedTokenBody>),
    /// Name is empty
    #[oai(status = 422)]
    Invalid(PlainText<String>),
}

#[derive(ApiResponse)]
enum DeleteTokenResponse {
    #[oai(status = 204)]
    Deleted,
    /// No token with this id on this account
    #[oai(status = 404)]
    NotFound,
}

#[OpenApi]
impl TokensApi {
    /// List API tokens. Only the display prefix is available after creation.
    #[oai(path = "/tokens", method = "get")]
    async fn list(
        &self,
        pool: Data<&SqlitePool>,
        session: SessionAuth,
    ) -> poem::Result<Json<Vec<TokenBody>>> {
        let rows = sqlx::query!(
            r#"SELECT id as "id!: i64", name, token_prefix, created_at, last_used_at
               FROM api_tokens WHERE user_id = ? ORDER BY id"#,
            session.0.id
        )
        .fetch_all(pool.0)
        .await
        .map_err(internal)?;

        Ok(Json(
            rows.into_iter()
                .map(|row| TokenBody {
                    id: row.id,
                    name: row.name,
                    token_prefix: row.token_prefix,
                    created_at: row.created_at,
                    last_used_at: row.last_used_at,
                })
                .collect(),
        ))
    }

    /// Create an API token for an agent.
    #[oai(path = "/tokens", method = "post")]
    async fn create(
        &self,
        pool: Data<&SqlitePool>,
        session: SessionAuth,
        body: Json<CreateTokenRequest>,
    ) -> poem::Result<CreateTokenResponse> {
        let name = body.0.name.trim().to_string();
        if name.is_empty() {
            return Ok(CreateTokenResponse::Invalid(PlainText(
                "name must not be empty".to_string(),
            )));
        }

        let token = format!("pem_{}", auth::random_base62(40));
        let token_prefix = token[..12].to_string();
        let token_hash = auth::sha256(&token);
        let done = sqlx::query!(
            "INSERT INTO api_tokens (user_id, name, token_hash, token_prefix) VALUES (?, ?, ?, ?)",
            session.0.id,
            name,
            token_hash,
            token_prefix
        )
        .execute(pool.0)
        .await
        .map_err(internal)?;

        Ok(CreateTokenResponse::Ok(Json(CreatedTokenBody {
            id: done.last_insert_rowid(),
            name,
            token,
        })))
    }

    /// Revoke an API token.
    #[oai(path = "/tokens/:id", method = "delete")]
    async fn delete(
        &self,
        pool: Data<&SqlitePool>,
        session: SessionAuth,
        id: Path<i64>,
    ) -> poem::Result<DeleteTokenResponse> {
        let done = sqlx::query!(
            "DELETE FROM api_tokens WHERE id = ? AND user_id = ?",
            id.0,
            session.0.id
        )
        .execute(pool.0)
        .await
        .map_err(internal)?;

        Ok(if done.rows_affected() == 0 {
            DeleteTokenResponse::NotFound
        } else {
            DeleteTokenResponse::Deleted
        })
    }
}
