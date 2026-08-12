use poem::web::Data;
use poem::{Request, RequestBody};
use poem_openapi::error::ContentTypeError;
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::{Html, Json, ParsePayload, Payload, PlainText};
use poem_openapi::registry::{MetaMediaType, MetaRequest, MetaSchema, MetaSchemaRef, Registry};
use poem_openapi::{
    ApiExtractor, ApiExtractorType, ApiResponse, ExtractParamOptions, Object, OpenApi,
};
use sqlx::SqlitePool;

use crate::api::{internal, is_unique_violation};
use crate::auth::{self, Auth, SessionAuth};
use crate::config::BaseUrl;

pub const MAX_HTML_BYTES: usize = 512 * 1024;
const PUBLIC_ID_LEN: usize = 10;

pub struct DocsApi;

/// Raw HTML request body, limited to 512KB.
struct HtmlBody(Vec<u8>);

impl Payload for HtmlBody {
    const CONTENT_TYPE: &'static str = "text/html";

    fn check_content_type(content_type: &str) -> bool {
        content_type.starts_with(Self::CONTENT_TYPE)
    }

    fn schema_ref() -> MetaSchemaRef {
        MetaSchemaRef::Inline(Box::new(MetaSchema {
            format: Some("binary"),
            ..MetaSchema::new("string")
        }))
    }
}

impl ParsePayload for HtmlBody {
    const IS_REQUIRED: bool = true;

    async fn from_request(_request: &Request, body: &mut RequestBody) -> poem::Result<Self> {
        let bytes = body.take()?.into_bytes_limit(MAX_HTML_BYTES).await?;
        Ok(Self(bytes.to_vec()))
    }
}

// poem-openapi implements ApiExtractor for its payload types through a
// crate-private macro; this is that macro's expansion for HtmlBody
impl<'a> ApiExtractor<'a> for HtmlBody {
    const TYPES: &'static [ApiExtractorType] = &[ApiExtractorType::RequestObject];

    type ParamType = ();
    type ParamRawType = ();

    fn register(registry: &mut Registry) {
        <Self as Payload>::register(registry);
    }

    fn request_meta() -> Option<MetaRequest> {
        Some(MetaRequest {
            description: None,
            content: vec![MetaMediaType {
                content_type: <Self as Payload>::CONTENT_TYPE,
                schema: <Self as Payload>::schema_ref(),
            }],
            required: <Self as ParsePayload>::IS_REQUIRED,
        })
    }

    async fn from_request(
        request: &'a Request,
        body: &mut RequestBody,
        _param_opts: ExtractParamOptions<Self::ParamType>,
    ) -> poem::Result<Self> {
        match request.content_type() {
            Some(content_type) if Self::check_content_type(content_type) => {
                <Self as ParsePayload>::from_request(request, body).await
            }
            Some(content_type) => Err(ContentTypeError::NotSupported {
                content_type: content_type.to_string(),
            }
            .into()),
            None => Err(ContentTypeError::ExpectContentType.into()),
        }
    }
}

#[derive(Object)]
struct PushedBody {
    /// Server-minted public document id, stable across revisions
    id: String,
    slug: String,
    revision: i64,
    size_bytes: i64,
    url: String,
}

#[derive(Object)]
struct RevisionBody {
    revision: i64,
    size_bytes: i64,
    created_at: String,
}

#[derive(Object)]
struct DocumentBody {
    id: String,
    slug: String,
    title: Option<String>,
    published: bool,
    revision_count: i64,
    latest_revision: i64,
    created_at: String,
    updated_at: String,
    url: String,
}

#[derive(Object)]
struct DocumentDetailBody {
    id: String,
    slug: String,
    title: Option<String>,
    published: bool,
    created_at: String,
    updated_at: String,
    url: String,
    revisions: Vec<RevisionBody>,
}

#[derive(Object)]
struct PublishRequest {
    password: String,
}

#[derive(Object)]
struct PublishedBody {
    url: String,
}

#[derive(ApiResponse)]
enum PublishResponse {
    #[oai(status = 200)]
    Ok(Json<PublishedBody>),
    /// Password must not be empty
    #[oai(status = 422)]
    Invalid(PlainText<String>),
    /// No document with this slug on this account
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum UnpublishResponse {
    #[oai(status = 204)]
    Done,
    /// No document with this slug on this account
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum PushResponse {
    #[oai(status = 200)]
    Ok(Json<PushedBody>),
    /// Slug must match [a-z0-9-]{1,64}
    #[oai(status = 422)]
    InvalidSlug(PlainText<String>),
}

#[derive(ApiResponse)]
enum DocumentResponse {
    #[oai(status = 200)]
    Ok(Json<DocumentDetailBody>),
    /// No document with this slug on this account
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum RawResponse {
    #[oai(status = 200)]
    Ok(
        Html<String>,
        #[oai(header = "Content-Security-Policy")] String,
        #[oai(header = "X-Content-Type-Options")] String,
        #[oai(header = "Referrer-Policy")] String,
    ),
    /// No such document or revision on this account
    #[oai(status = 404)]
    NotFound,
}

// pushed documents are arbitrary HTML on the app origin: the sandbox (without
// allow-same-origin) gives them an opaque origin, so their scripts cannot read
// app cookies or make credentialed API calls
fn raw_ok(html: Vec<u8>) -> RawResponse {
    RawResponse::Ok(
        Html(String::from_utf8_lossy(&html).into_owned()),
        "sandbox allow-scripts allow-popups".to_string(),
        "nosniff".to_string(),
        "no-referrer".to_string(),
    )
}

#[OpenApi]
impl DocsApi {
    /// Push a document. The first push of a slug creates the document and
    /// mints its public id; every later push appends a revision at the same
    /// URL.
    #[oai(path = "/docs/:slug", method = "put")]
    async fn push(
        &self,
        pool: Data<&SqlitePool>,
        base_url: Data<&BaseUrl>,
        auth: Auth,
        slug: Path<String>,
        title: Query<Option<String>>,
        body: HtmlBody,
    ) -> poem::Result<PushResponse> {
        let slug = slug.0;
        if !valid_slug(&slug) {
            return Ok(PushResponse::InvalidSlug(PlainText(
                "slug must match [a-z0-9-]{1,64}".to_string(),
            )));
        }
        let owner_id = auth.user().id;

        let mut tx = pool.0.begin().await.map_err(internal)?;

        let existing = sqlx::query!(
            r#"SELECT id as "id!: i64", public_id as "public_id!: String"
               FROM documents WHERE owner_id = ? AND slug = ?"#,
            owner_id,
            slug
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;

        let (document_id, public_id) = match existing {
            Some(row) => {
                sqlx::query!(
                    "UPDATE documents
                     SET updated_at = datetime('now'), title = COALESCE(?, title)
                     WHERE id = ?",
                    title.0,
                    row.id
                )
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
                (row.id, row.public_id)
            }
            None => {
                let mut minted = None;
                for _ in 0..5 {
                    let candidate = auth::random_base62(PUBLIC_ID_LEN);
                    let inserted = sqlx::query!(
                        "INSERT INTO documents (public_id, owner_id, slug, title)
                         VALUES (?, ?, ?, ?)",
                        candidate,
                        owner_id,
                        slug,
                        title.0
                    )
                    .execute(&mut *tx)
                    .await;
                    match inserted {
                        Ok(done) => {
                            minted = Some((done.last_insert_rowid(), candidate));
                            break;
                        }
                        Err(e) if is_unique_violation(&e) => continue,
                        Err(e) => return Err(internal(e)),
                    }
                }
                minted
                    .ok_or_else(|| internal(std::io::Error::other("public id space exhausted")))?
            }
        };

        let size_bytes = body.0.len() as i64;
        let revision = sqlx::query_scalar!(
            r#"INSERT INTO revisions (document_id, revision, html, size_bytes)
               SELECT ?, COALESCE(MAX(revision), 0) + 1, ?, ? FROM revisions WHERE document_id = ?
               RETURNING revision as "revision!: i64""#,
            document_id,
            body.0,
            size_bytes,
            document_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;

        tx.commit().await.map_err(internal)?;

        let url = format!("{}/{}/{}", base_url.0.0, public_id, slug);
        Ok(PushResponse::Ok(Json(PushedBody {
            id: public_id,
            slug,
            revision,
            size_bytes,
            url,
        })))
    }

    /// List your documents.
    #[oai(path = "/docs", method = "get")]
    async fn list(
        &self,
        pool: Data<&SqlitePool>,
        base_url: Data<&BaseUrl>,
        auth: Auth,
    ) -> poem::Result<Json<Vec<DocumentBody>>> {
        let rows = sqlx::query!(
            r#"SELECT d.public_id as "public_id!: String", d.slug as "slug!: String",
                      d.title, d.published as "published!: bool",
                      d.created_at as "created_at!: String", d.updated_at as "updated_at!: String",
                      COUNT(r.id) as "revision_count!: i64",
                      COALESCE(MAX(r.revision), 0) as "latest_revision!: i64"
               FROM documents d LEFT JOIN revisions r ON r.document_id = d.id
               WHERE d.owner_id = ?
               GROUP BY d.id
               ORDER BY d.updated_at DESC"#,
            auth.user().id
        )
        .fetch_all(pool.0)
        .await
        .map_err(internal)?;

        Ok(Json(
            rows.into_iter()
                .map(|row| DocumentBody {
                    url: format!("{}/{}/{}", base_url.0.0, row.public_id, row.slug),
                    id: row.public_id,
                    slug: row.slug,
                    title: row.title,
                    published: row.published,
                    revision_count: row.revision_count,
                    latest_revision: row.latest_revision,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
                .collect(),
        ))
    }

    /// Document metadata and its revision index.
    #[oai(path = "/docs/:slug", method = "get")]
    async fn detail(
        &self,
        pool: Data<&SqlitePool>,
        base_url: Data<&BaseUrl>,
        auth: Auth,
        slug: Path<String>,
    ) -> poem::Result<DocumentResponse> {
        let row = sqlx::query!(
            r#"SELECT id as "id!: i64", public_id as "public_id!: String", title,
                      published as "published!: bool",
                      created_at as "created_at!: String", updated_at as "updated_at!: String"
               FROM documents WHERE owner_id = ? AND slug = ?"#,
            auth.user().id,
            slug.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;
        let Some(row) = row else {
            return Ok(DocumentResponse::NotFound);
        };

        let revisions = sqlx::query!(
            r#"SELECT revision as "revision!: i64", size_bytes as "size_bytes!: i64",
                      created_at as "created_at!: String"
               FROM revisions WHERE document_id = ? ORDER BY revision"#,
            row.id
        )
        .fetch_all(pool.0)
        .await
        .map_err(internal)?;

        Ok(DocumentResponse::Ok(Json(DocumentDetailBody {
            url: format!("{}/{}/{}", base_url.0.0, row.public_id, slug.0),
            id: row.public_id,
            slug: slug.0,
            title: row.title,
            published: row.published,
            created_at: row.created_at,
            updated_at: row.updated_at,
            revisions: revisions
                .into_iter()
                .map(|r| RevisionBody {
                    revision: r.revision,
                    size_bytes: r.size_bytes,
                    created_at: r.created_at,
                })
                .collect(),
        })))
    }

    /// The latest revision's HTML.
    #[oai(path = "/docs/:slug/raw", method = "get")]
    async fn raw_latest(
        &self,
        pool: Data<&SqlitePool>,
        auth: Auth,
        slug: Path<String>,
    ) -> poem::Result<RawResponse> {
        let html = sqlx::query_scalar!(
            r#"SELECT r.html as "html!: Vec<u8>"
               FROM revisions r JOIN documents d ON d.id = r.document_id
               WHERE d.owner_id = ? AND d.slug = ?
               ORDER BY r.revision DESC LIMIT 1"#,
            auth.user().id,
            slug.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;

        Ok(match html {
            Some(html) => raw_ok(html),
            None => RawResponse::NotFound,
        })
    }

    /// The HTML as of a specific revision.
    #[oai(path = "/docs/:slug/revisions/:revision/raw", method = "get")]
    async fn raw_revision(
        &self,
        pool: Data<&SqlitePool>,
        auth: Auth,
        slug: Path<String>,
        revision: Path<i64>,
    ) -> poem::Result<RawResponse> {
        let html = sqlx::query_scalar!(
            r#"SELECT r.html as "html!: Vec<u8>"
               FROM revisions r JOIN documents d ON d.id = r.document_id
               WHERE d.owner_id = ? AND d.slug = ? AND r.revision = ?"#,
            auth.user().id,
            slug.0,
            revision.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;

        Ok(match html {
            Some(html) => raw_ok(html),
            None => RawResponse::NotFound,
        })
    }

    /// Publish the document behind a password. Calling this again rotates the
    /// password and invalidates every outstanding visitor cookie.
    #[oai(path = "/docs/:slug/publish", method = "post")]
    async fn publish(
        &self,
        pool: Data<&SqlitePool>,
        base_url: Data<&BaseUrl>,
        session: SessionAuth,
        slug: Path<String>,
        body: Json<PublishRequest>,
    ) -> poem::Result<PublishResponse> {
        if body.0.password.is_empty() {
            return Ok(PublishResponse::Invalid(PlainText(
                "password must not be empty".to_string(),
            )));
        }
        let password_hash = auth::hash_password(body.0.password).await;
        let public_id = sqlx::query_scalar!(
            r#"UPDATE documents SET published = 1, password_hash = ?
               WHERE owner_id = ? AND slug = ?
               RETURNING public_id as "public_id!: String""#,
            password_hash,
            session.0.id,
            slug.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;

        Ok(match public_id {
            Some(public_id) => PublishResponse::Ok(Json(PublishedBody {
                url: format!("{}/{}/{}", base_url.0.0, public_id, slug.0),
            })),
            None => PublishResponse::NotFound,
        })
    }

    /// Take the document off the public URL again.
    #[oai(path = "/docs/:slug/unpublish", method = "post")]
    async fn unpublish(
        &self,
        pool: Data<&SqlitePool>,
        session: SessionAuth,
        slug: Path<String>,
    ) -> poem::Result<UnpublishResponse> {
        let done = sqlx::query!(
            "UPDATE documents SET published = 0 WHERE owner_id = ? AND slug = ?",
            session.0.id,
            slug.0
        )
        .execute(pool.0)
        .await
        .map_err(internal)?;

        Ok(if done.rows_affected() == 0 {
            UnpublishResponse::NotFound
        } else {
            UnpublishResponse::Done
        })
    }
}

fn valid_slug(slug: &str) -> bool {
    (1..=64).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
