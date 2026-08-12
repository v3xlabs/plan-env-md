use poem::web::Data;
use poem_openapi::param::Path;
use poem_openapi::payload::Json;
use poem_openapi::{ApiResponse, Object, OpenApi};
use sqlx::SqlitePool;

use crate::api::internal;
use crate::auth::{self, SessionAuth};

pub struct InvitesApi;

#[derive(Object)]
struct InviteBody {
    id: i64,
    code: String,
    created_at: String,
    used_at: Option<String>,
    used_by: Option<String>,
}

#[derive(ApiResponse)]
enum ListInvitesResponse {
    #[oai(status = 200)]
    Ok(Json<Vec<InviteBody>>),
    /// Admin only
    #[oai(status = 403)]
    Forbidden,
}

#[derive(ApiResponse)]
enum CreateInviteResponse {
    #[oai(status = 200)]
    Ok(Json<InviteBody>),
    /// Admin only
    #[oai(status = 403)]
    Forbidden,
}

#[derive(ApiResponse)]
enum DeleteInviteResponse {
    #[oai(status = 204)]
    Deleted,
    /// Admin only
    #[oai(status = 403)]
    Forbidden,
    /// No unused invite with this id
    #[oai(status = 404)]
    NotFound,
}

#[OpenApi]
impl InvitesApi {
    /// List invite codes and who used them.
    #[oai(path = "/invites", method = "get")]
    async fn list(
        &self,
        pool: Data<&SqlitePool>,
        session: SessionAuth,
    ) -> poem::Result<ListInvitesResponse> {
        if !session.0.is_admin {
            return Ok(ListInvitesResponse::Forbidden);
        }
        let rows = sqlx::query!(
            r#"SELECT i.id as "id!: i64", i.code as "code!: String",
                      i.created_at as "created_at!: String", i.used_at,
                      u.username as "used_by?: String"
               FROM invite_codes i LEFT JOIN users u ON u.id = i.used_by
               ORDER BY i.id"#
        )
        .fetch_all(pool.0)
        .await
        .map_err(internal)?;

        Ok(ListInvitesResponse::Ok(Json(
            rows.into_iter()
                .map(|row| InviteBody {
                    id: row.id,
                    code: row.code,
                    created_at: row.created_at,
                    used_at: row.used_at,
                    used_by: row.used_by,
                })
                .collect(),
        )))
    }

    /// Mint an invite code.
    #[oai(path = "/invites", method = "post")]
    async fn create(
        &self,
        pool: Data<&SqlitePool>,
        session: SessionAuth,
    ) -> poem::Result<CreateInviteResponse> {
        if !session.0.is_admin {
            return Ok(CreateInviteResponse::Forbidden);
        }
        let code = auth::random_base62(16);
        let done = sqlx::query!(
            "INSERT INTO invite_codes (code, created_by) VALUES (?, ?)",
            code,
            session.0.id
        )
        .execute(pool.0)
        .await
        .map_err(internal)?;
        let invite_id = done.last_insert_rowid();
        let row = sqlx::query!(
            "SELECT created_at FROM invite_codes WHERE id = ?",
            invite_id
        )
        .fetch_one(pool.0)
        .await
        .map_err(internal)?;

        Ok(CreateInviteResponse::Ok(Json(InviteBody {
            id: invite_id,
            code,
            created_at: row.created_at,
            used_at: None,
            used_by: None,
        })))
    }

    /// Delete an unused invite code.
    #[oai(path = "/invites/:id", method = "delete")]
    async fn delete(
        &self,
        pool: Data<&SqlitePool>,
        session: SessionAuth,
        id: Path<i64>,
    ) -> poem::Result<DeleteInviteResponse> {
        if !session.0.is_admin {
            return Ok(DeleteInviteResponse::Forbidden);
        }
        let done = sqlx::query!(
            "DELETE FROM invite_codes WHERE id = ? AND used_by IS NULL",
            id.0
        )
        .execute(pool.0)
        .await
        .map_err(internal)?;

        Ok(if done.rows_affected() == 0 {
            DeleteInviteResponse::NotFound
        } else {
            DeleteInviteResponse::Deleted
        })
    }
}
