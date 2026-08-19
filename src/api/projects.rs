use poem::web::Data;
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::{Binary, Json, PlainText};
use poem_openapi::{ApiResponse, Enum, Object, OpenApi};
use sqlx::SqlitePool;

use crate::api::internal;
use crate::auth::Auth;

pub struct ProjectsApi;

/// Favicons are served into a browser tab at 16 to 32 pixels, so anything past
/// this is a mistake rather than a preference.
const MAX_FAVICON_BYTES: usize = 64 * 1024;

#[derive(Enum, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "lowercase")]
pub enum Scheme {
    Light,
    Dark,
}

#[derive(Object)]
struct ProjectBody {
    slug: String,
    /// Other names that resolve to this project on push
    aliases: Vec<String>,
    document_count: i64,
    /// Newest push in the project, so the list can sort by activity
    last_pushed_at: Option<String>,
    has_favicon_light: bool,
    has_favicon_dark: bool,
}

#[derive(ApiResponse)]
enum AliasResponse {
    #[oai(status = 204)]
    Done,
    /// The alias is already a project of its own, or points somewhere else
    #[oai(status = 409)]
    Taken(PlainText<String>),
    /// Not a valid slug shape
    #[oai(status = 422)]
    Invalid(PlainText<String>),
}

#[derive(ApiResponse)]
enum FaviconResponse {
    #[oai(status = 200)]
    Ok(
        Binary<Vec<u8>>,
        #[oai(header = "Content-Type")] String,
        #[oai(header = "Cache-Control")] String,
    ),
    /// No such project, or no favicon in this colour scheme
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum SetFaviconResponse {
    #[oai(status = 204)]
    Done,
    /// Not an accepted image type, or larger than 64KB
    #[oai(status = 422)]
    Invalid(PlainText<String>),
    /// No project of this name on this account
    #[oai(status = 404)]
    NotFound,
}

#[OpenApi]
impl ProjectsApi {
    /// Your projects, newest activity first.
    #[oai(path = "/projects", method = "get")]
    async fn list(
        &self,
        pool: Data<&SqlitePool>,
        auth: Auth,
    ) -> poem::Result<Json<Vec<ProjectBody>>> {
        let rows = sqlx::query!(
            r#"SELECT p.slug as "slug!: String",
                      COUNT(d.id) as "document_count!: i64",
                      MAX(r.last_pushed_at) as "last_pushed_at: String",
                      p.favicon_light IS NOT NULL as "has_favicon_light!: bool",
                      p.favicon_dark IS NOT NULL as "has_favicon_dark!: bool"
               FROM projects p
               LEFT JOIN documents d ON d.owner_id = p.owner_id AND d.project = p.slug
               LEFT JOIN (SELECT document_id, MAX(created_at) AS last_pushed_at
                          FROM revisions GROUP BY document_id) r ON r.document_id = d.id
               WHERE p.owner_id = ?
               GROUP BY p.id
               ORDER BY MAX(r.last_pushed_at) IS NULL, MAX(r.last_pushed_at) DESC, p.slug"#,
            auth.user().id
        )
        .fetch_all(pool.0)
        .await
        .map_err(internal)?;

        let aliases = sqlx::query!(
            r#"SELECT project as "project!: String", alias as "alias!: String"
               FROM project_aliases WHERE owner_id = ? ORDER BY alias"#,
            auth.user().id
        )
        .fetch_all(pool.0)
        .await
        .map_err(internal)?;

        Ok(Json(
            rows.into_iter()
                .map(|row| ProjectBody {
                    aliases: aliases
                        .iter()
                        .filter(|entry| entry.project == row.slug)
                        .map(|entry| entry.alias.clone())
                        .collect(),
                    slug: row.slug,
                    document_count: row.document_count,
                    last_pushed_at: row.last_pushed_at,
                    has_favicon_light: row.has_favicon_light,
                    has_favicon_dark: row.has_favicon_dark,
                })
                .collect(),
        ))
    }

    /// Point another name at this project. A push naming the alias lands in the
    /// project, so `openlv` and `open-lavatory` do not become two piles.
    #[oai(path = "/projects/:project/aliases/:alias", method = "put")]
    async fn add_alias(
        &self,
        pool: Data<&SqlitePool>,
        auth: Auth,
        project: Path<String>,
        alias: Path<String>,
    ) -> poem::Result<AliasResponse> {
        if !crate::api::docs::valid_slug(&alias.0) || !crate::api::docs::valid_slug(&project.0) {
            return Ok(AliasResponse::Invalid(PlainText(
                "project and alias must match [a-z0-9-]{1,64}".to_string(),
            )));
        }
        if alias.0 == project.0 {
            return Ok(AliasResponse::Invalid(PlainText(
                "a project is already reachable by its own name".to_string(),
            )));
        }
        // a name is either a project or an alias, never both: otherwise a push
        // naming it would have two answers
        let is_project = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM projects WHERE owner_id = ? AND slug = ?"#,
            auth.user().id,
            alias.0
        )
        .fetch_one(pool.0)
        .await
        .map_err(internal)?;
        if is_project > 0 {
            return Ok(AliasResponse::Taken(PlainText(format!(
                "{} is a project of its own; merging projects is not supported",
                alias.0
            ))));
        }

        let mut tx = pool.0.begin().await.map_err(internal)?;
        sqlx::query!(
            "INSERT OR IGNORE INTO projects (owner_id, slug) VALUES (?, ?)",
            auth.user().id,
            project.0
        )
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        sqlx::query!(
            "INSERT INTO project_aliases (owner_id, alias, project) VALUES (?, ?, ?)
             ON CONFLICT (owner_id, alias) DO UPDATE SET project = excluded.project",
            auth.user().id,
            alias.0,
            project.0
        )
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;

        Ok(AliasResponse::Done)
    }

    /// Stop resolving this name. Documents already in the project stay put.
    #[oai(path = "/projects/:project/aliases/:alias", method = "delete")]
    async fn remove_alias(
        &self,
        pool: Data<&SqlitePool>,
        auth: Auth,
        project: Path<String>,
        alias: Path<String>,
    ) -> poem::Result<AliasResponse> {
        sqlx::query!(
            "DELETE FROM project_aliases WHERE owner_id = ? AND alias = ? AND project = ?",
            auth.user().id,
            alias.0,
            project.0
        )
        .execute(pool.0)
        .await
        .map_err(internal)?;
        Ok(AliasResponse::Done)
    }

    /// A project's favicon. Owner only, like everything else about a document.
    #[oai(path = "/projects/:project/favicon", method = "get")]
    async fn favicon(
        &self,
        pool: Data<&SqlitePool>,
        auth: Auth,
        project: Path<String>,
        scheme: Query<Option<Scheme>>,
    ) -> poem::Result<FaviconResponse> {
        // an alias is a name for this project, so it reaches its icon too
        let project = crate::api::docs::resolve_project(pool.0, auth.user().id, &project.0)
            .await
            .map_err(internal)?;
        let found = load_favicon(
            pool.0,
            auth.user().id,
            &project,
            scheme.0.unwrap_or(Scheme::Light),
        )
        .await?;

        Ok(match found {
            Some((bytes, content_type)) => FaviconResponse::Ok(
                Binary(bytes),
                content_type,
                "private, max-age=300".to_string(),
            ),
            None => FaviconResponse::NotFound,
        })
    }

    /// Upload a project favicon. Every document in the project then serves it,
    /// so a reader's tab says which project they are looking at.
    #[oai(path = "/projects/:project/favicon", method = "put")]
    async fn set_favicon(
        &self,
        pool: Data<&SqlitePool>,
        auth: Auth,
        project: Path<String>,
        scheme: Query<Option<Scheme>>,
        body: Binary<Vec<u8>>,
    ) -> poem::Result<SetFaviconResponse> {
        if body.0.len() > MAX_FAVICON_BYTES {
            return Ok(SetFaviconResponse::Invalid(PlainText(format!(
                "favicon must be at most {} KB",
                MAX_FAVICON_BYTES / 1024
            ))));
        }
        let Some(content_type) = sniff_image(&body.0) else {
            return Ok(SetFaviconResponse::Invalid(PlainText(
                "favicon must be a PNG, SVG, WebP, GIF or ICO".to_string(),
            )));
        };

        // an alias names an existing project; resolving first stops a stray
        // project row appearing under the alias
        let project = crate::api::docs::resolve_project(pool.0, auth.user().id, &project.0)
            .await
            .map_err(internal)?;
        // the project row may not exist yet if no document has named it
        sqlx::query!(
            "INSERT OR IGNORE INTO projects (owner_id, slug) VALUES (?, ?)",
            auth.user().id,
            project
        )
        .execute(pool.0)
        .await
        .map_err(internal)?;

        let done = match scheme.0.unwrap_or(Scheme::Light) {
            Scheme::Light => {
                sqlx::query!(
                    "UPDATE projects SET favicon_light = ?, favicon_light_type = ?
                 WHERE owner_id = ? AND slug = ?",
                    body.0,
                    content_type,
                    auth.user().id,
                    project
                )
                .execute(pool.0)
                .await
            }
            Scheme::Dark => {
                sqlx::query!(
                    "UPDATE projects SET favicon_dark = ?, favicon_dark_type = ?
                 WHERE owner_id = ? AND slug = ?",
                    body.0,
                    content_type,
                    auth.user().id,
                    project
                )
                .execute(pool.0)
                .await
            }
        }
        .map_err(internal)?;

        Ok(if done.rows_affected() == 0 {
            SetFaviconResponse::NotFound
        } else {
            SetFaviconResponse::Done
        })
    }
}

/// The favicon bytes, falling back to the other scheme when only one was
/// uploaded: one icon in the wrong theme beats no icon at all.
pub async fn load_favicon(
    pool: &SqlitePool,
    owner_id: i64,
    project: &str,
    scheme: Scheme,
) -> poem::Result<Option<(Vec<u8>, String)>> {
    let row = sqlx::query!(
        r#"SELECT favicon_light, favicon_light_type, favicon_dark, favicon_dark_type
           FROM projects WHERE owner_id = ? AND slug = ?"#,
        owner_id,
        project
    )
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(row) = row else {
        return Ok(None);
    };

    let light = row.favicon_light.zip(row.favicon_light_type);
    let dark = row.favicon_dark.zip(row.favicon_dark_type);
    Ok(match scheme {
        Scheme::Light => light.or(dark),
        Scheme::Dark => dark.or(light),
    })
}

/// Content type from the bytes, never from the caller. An extension or a header
/// is a claim; a magic number is evidence.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    const GIF: &[u8] = b"GIF8";
    const ICO: &[u8] = b"\x00\x00\x01\x00";

    if bytes.starts_with(PNG) {
        return Some("image/png");
    }
    if bytes.starts_with(GIF) {
        return Some("image/gif");
    }
    if bytes.starts_with(ICO) {
        return Some("image/x-icon");
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    // SVG is text, so look for a root element in the leading bytes rather than
    // a signature; the served response is sandboxed either way
    let head = std::str::from_utf8(bytes.get(..512.min(bytes.len()))?).ok()?;
    if head.contains("<svg") {
        return Some("image/svg+xml");
    }
    None
}
