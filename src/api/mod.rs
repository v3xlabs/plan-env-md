pub mod auth;
pub mod docs;
pub mod invites;
pub mod tokens;

use poem_openapi::payload::PlainText;
use poem_openapi::{OpenApi, OpenApiService};

pub struct Api;

#[OpenApi]
impl Api {
    /// Service health
    #[oai(path = "/health", method = "get")]
    async fn health(&self) -> PlainText<&'static str> {
        PlainText("ok")
    }
}

type Apis = (
    Api,
    auth::AuthApi,
    tokens::TokensApi,
    invites::InvitesApi,
    docs::DocsApi,
);

pub fn service() -> OpenApiService<Apis, ()> {
    OpenApiService::new(
        (
            Api,
            auth::AuthApi,
            tokens::TokensApi,
            invites::InvitesApi,
            docs::DocsApi,
        ),
        "plan-env-md",
        env!("CARGO_PKG_VERSION"),
    )
    .url_prefix("/api")
}

pub fn internal<E>(error: E) -> poem::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    poem::error::InternalServerError(error)
}

pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.is_unique_violation())
}
