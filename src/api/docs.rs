use std::collections::HashMap;

use poem::web::{Data, Multipart};
use poem::{FromRequest, Request, RequestBody};
use poem_openapi::error::ContentTypeError;
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::{Html, Json, ParsePayload, Payload, PlainText};
use poem_openapi::registry::{MetaMediaType, MetaRequest, MetaSchema, MetaSchemaRef, Registry};
use poem_openapi::{
    ApiExtractor, ApiExtractorType, ApiResponse, ExtractParamOptions, Object, OpenApi,
};
use sqlx::SqlitePool;

use crate::answer_key;
use crate::api::question::{self, Answer, AnsweredQuestion, Question};
use crate::api::upload::{self, ENTRY_PATH, MAX_ENTRY_BYTES, UploadedFile};
use crate::api::{blob_failed, internal, is_unique_violation};
use crate::auth::{self, Auth, AuthUser, SessionAuth};
use crate::config::{DocsUrl, Secret};

const PUBLIC_ID_LEN: usize = 10;
const META_PART: &str = "meta";

pub struct DocsApi;

/// Metadata the caller may attach to a push. Carried as a JSON multipart part
/// named `meta`; the plain `text/html` body path has no metadata beyond
/// `?title=`, so there is one rule rather than two half rules.
#[derive(serde::Deserialize, Default)]
struct PushMeta {
    title: Option<String>,
    project: Option<String>,
    /// Omitted leaves the tag set alone; `[]` clears it; a list replaces it.
    tags: Option<Vec<String>>,
    questions: Option<Vec<Question>>,
}

/// The push body in either accepted shape.
struct PushBody {
    files: Vec<UploadedFile>,
    meta: PushMeta,
}

impl Payload for PushBody {
    const CONTENT_TYPE: &'static str = "text/html";

    fn check_content_type(content_type: &str) -> bool {
        content_type.starts_with("text/html") || content_type.starts_with("multipart/form-data")
    }

    fn schema_ref() -> MetaSchemaRef {
        MetaSchemaRef::Inline(Box::new(MetaSchema {
            format: Some("binary"),
            ..MetaSchema::new("string")
        }))
    }
}

impl ParsePayload for PushBody {
    const IS_REQUIRED: bool = true;

    async fn from_request(request: &Request, body: &mut RequestBody) -> poem::Result<Self> {
        let multipart = request
            .content_type()
            .is_some_and(|value| value.starts_with("multipart/form-data"));
        if !multipart {
            let bytes = body.take()?.into_bytes_limit(MAX_ENTRY_BYTES).await?;
            return Ok(Self {
                files: vec![UploadedFile {
                    path: ENTRY_PATH.to_string(),
                    content: bytes.to_vec(),
                    content_type: upload::content_type_for(ENTRY_PATH)
                        .expect("index.html is an accepted extension"),
                }],
                meta: PushMeta::default(),
            });
        }

        let mut form = <Multipart as FromRequest>::from_request(request, body).await?;
        let mut files: Vec<UploadedFile> = Vec::new();
        let mut meta = PushMeta::default();
        while let Some(field) = form.next_field().await? {
            // name() borrows the field, and reading the field consumes it
            let Some(name) = field.name().map(str::to_owned) else {
                continue;
            };
            if name == META_PART {
                let text = field.text().await?;
                meta = serde_json::from_str(&text).map_err(|error| {
                    unprocessable(format!("meta part is not valid JSON: {error}"))
                })?;
                continue;
            }

            // the part name is the file's path inside the revision
            upload::validate_path(&name).map_err(unprocessable)?;
            let content_type =
                upload::content_type_for(&name).expect("validate_path checked the extension");
            files.push(UploadedFile {
                path: name,
                content: field.bytes().await?,
                content_type,
            });
        }

        upload::validate_set(&files).map_err(unprocessable)?;
        Ok(Self { files, meta })
    }
}

fn unprocessable(message: String) -> poem::Error {
    poem::Error::from_string(message, poem::http::StatusCode::UNPROCESSABLE_ENTITY)
}

// poem-openapi implements ApiExtractor for its payload types through a
// crate-private macro; this is that macro's expansion for PushBody
impl<'a> ApiExtractor<'a> for PushBody {
    const TYPES: &'static [ApiExtractorType] = &[ApiExtractorType::RequestObject];

    type ParamType = ();
    type ParamRawType = ();

    fn register(registry: &mut Registry) {
        <Self as Payload>::register(registry);
    }

    fn request_meta() -> Option<MetaRequest> {
        Some(MetaRequest {
            description: None,
            content: vec![
                MetaMediaType {
                    content_type: "text/html",
                    schema: <Self as Payload>::schema_ref(),
                },
                MetaMediaType {
                    content_type: "multipart/form-data",
                    schema: <Self as Payload>::schema_ref(),
                },
            ],
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

/// Permission to write one document's answers: a browser session, or a scoped
/// key minted for the in-document widget. Deliberately not a `pem_` API token,
/// which is what agents hold: an answer records what a human decided.
enum AnswerAuth {
    Session(AuthUser),
    Scoped(i64),
}

impl<'a> FromRequest<'a> for AnswerAuth {
    async fn from_request(request: &'a Request, _body: &mut RequestBody) -> poem::Result<Self> {
        let bearer = request
            .headers()
            .get(poem::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));

        if let Some(token) = bearer
            && token.starts_with(answer_key::PREFIX)
        {
            let secret = request
                .data::<Secret>()
                .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::UNAUTHORIZED))?;
            let document_id = answer_key::verify(&secret.0, token)
                .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::UNAUTHORIZED))?;
            return Ok(Self::Scoped(document_id));
        }

        let pool = request
            .data::<SqlitePool>()
            .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::UNAUTHORIZED))?;
        let key = request
            .cookie()
            .get(auth::SESSION_COOKIE)
            .map(|cookie| cookie.value_str().to_string())
            .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::UNAUTHORIZED))?;
        let user = auth::session_user(pool, &key)
            .await
            .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::UNAUTHORIZED))?;
        Ok(Self::Session(user))
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
    /// Every file the revision holds, as stored. A caller that linked an asset
    /// from the entry document compares its href against these paths to see
    /// that the file arrived and arrived where the link points.
    files: Vec<FileBody>,
}

/// One file of a revision, at the path it is served from under the document's
/// directory URL.
#[derive(Object)]
struct FileBody {
    path: String,
    size_bytes: i64,
    content_type: String,
}

#[derive(Object)]
struct RevisionBody {
    revision: i64,
    size_bytes: i64,
    created_at: String,
    files: Vec<FileBody>,
}

#[derive(Object)]
struct DocumentBody {
    id: String,
    slug: String,
    title: Option<String>,
    project: Option<String>,
    tags: Vec<String>,
    published: bool,
    revision_count: i64,
    latest_revision: i64,
    /// Questions the latest revision asks, and how many carry an answer
    questions_total: i64,
    questions_answered: i64,
    /// When the newest revision was pushed; the list sorts by this
    last_pushed_at: String,
    created_at: String,
    updated_at: String,
    url: String,
}

#[derive(Object)]
struct DocumentDetailBody {
    id: String,
    slug: String,
    title: Option<String>,
    project: Option<String>,
    tags: Vec<String>,
    published: bool,
    created_at: String,
    updated_at: String,
    url: String,
    revisions: Vec<RevisionBody>,
    questions: Vec<AnsweredQuestion>,
}

/// Fields a document owner may correct after the fact. Omitted leaves the
/// stored value alone, which is what makes "no backfill" workable for the
/// documents pushed before projects existed.
#[derive(Object)]
struct PatchRequest {
    title: Option<String>,
    project: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(ApiResponse)]
enum PatchResponse {
    #[oai(status = 200)]
    Ok(Json<Box<DocumentDetailBody>>),
    /// Project or a tag does not fit its shape
    #[oai(status = 422)]
    Invalid(PlainText<String>),
    /// No document with this slug on this account
    #[oai(status = 404)]
    NotFound,
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
    Ok(Json<Box<DocumentDetailBody>>),
    /// No document with this slug on this account
    #[oai(status = 404)]
    NotFound,
}

#[derive(Object)]
struct AnswerRequest {
    /// Declared option values, or the reserved `other` for a written answer.
    selected: Vec<String>,
    other_text: Option<String>,
    notes: Option<String>,
}

// The widget calls from the docs origin, so every answer response says that
// origin may read it. There is deliberately no Allow-Credentials: the scoped key
// travels in a header rather than a cookie, so the request is uncredentialed and
// allowing an origin here grants nothing a reader's browser would add to it.
fn cors(docs_url: &DocsUrl) -> String {
    docs_url.0.as_str().to_string()
}

#[derive(ApiResponse)]
enum PreviewImageResponse {
    #[oai(status = 200)]
    Ok(
        poem_openapi::payload::Binary<Vec<u8>>,
        #[oai(header = "Content-Type")] String,
        #[oai(header = "Cache-Control")] String,
    ),
    /// No such revision, or its thumbnail is not rendered yet
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum DeleteDocumentResponse {
    #[oai(status = 204)]
    Done,
    /// No document with this slug on this account
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum RefreshPreviewResponse {
    #[oai(status = 202)]
    Queued,
    /// No document with this slug on this account, or it has no revisions
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum QuestionsResponse {
    #[oai(status = 200)]
    Ok(Json<Vec<AnsweredQuestion>>),
    /// No document with this slug on this account
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum AnswerResponse {
    #[oai(status = 200)]
    Ok(
        Json<Answer>,
        #[oai(header = "Access-Control-Allow-Origin")] String,
    ),
    /// The answer does not fit the question that was asked
    #[oai(status = 422)]
    Invalid(
        PlainText<String>,
        #[oai(header = "Access-Control-Allow-Origin")] String,
    ),
    /// No such document, or the latest revision does not ask this question
    #[oai(status = 404)]
    NotFound(#[oai(header = "Access-Control-Allow-Origin")] String),
}

#[derive(ApiResponse)]
enum ClearAnswerResponse {
    #[oai(status = 204)]
    Done(#[oai(header = "Access-Control-Allow-Origin")] String),
    #[oai(status = 404)]
    NotFound(#[oai(header = "Access-Control-Allow-Origin")] String),
}

#[derive(ApiResponse)]
enum PreflightResponse {
    #[oai(status = 204)]
    Ok(
        #[oai(header = "Access-Control-Allow-Origin")] String,
        #[oai(header = "Access-Control-Allow-Headers")] String,
        #[oai(header = "Access-Control-Allow-Methods")] String,
        #[oai(header = "Access-Control-Max-Age")] String,
    ),
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
    #[allow(clippy::too_many_arguments)]
    async fn push(
        &self,
        pool: Data<&SqlitePool>,
        docs_url: Data<&DocsUrl>,
        blobs: Data<&Option<crate::blobs::Blobs>>,
        auth: Auth,
        slug: Path<String>,
        title: Query<Option<String>>,
        body: PushBody,
    ) -> poem::Result<PushResponse> {
        let slug = slug.0;
        if !valid_slug(&slug) {
            return Ok(PushResponse::InvalidSlug(PlainText(
                "slug must match [a-z0-9-]{1,64}".to_string(),
            )));
        }
        let questions = body.meta.questions.unwrap_or_default();
        if let Err(message) = question::validate_all(&questions) {
            return Ok(PushResponse::InvalidSlug(PlainText(message)));
        }
        if let Some(project) = &body.meta.project
            && !valid_slug(project)
        {
            return Ok(PushResponse::InvalidSlug(PlainText(
                "project must match [a-z0-9-]{1,64}".to_string(),
            )));
        }
        let tags = match body.meta.tags.as_deref().map(normalize_tags).transpose() {
            Ok(tags) => tags,
            Err(message) => return Ok(PushResponse::InvalidSlug(PlainText(message))),
        };
        // ?title= stays supported; the meta part wins when both are given
        let title = body.meta.title.or(title.0);
        let project = match body.meta.project {
            Some(name) => Some(
                resolve_project(pool.0, auth.user().id, &name)
                    .await
                    .map_err(internal)?,
            ),
            None => None,
        };
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
                     SET updated_at = datetime('now'), title = COALESCE(?, title),
                         project = COALESCE(?, project)
                     WHERE id = ?",
                    title,
                    project,
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
                        "INSERT INTO documents (public_id, owner_id, slug, title, project)
                         VALUES (?, ?, ?, ?, ?)",
                        candidate,
                        owner_id,
                        slug,
                        title,
                        project
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

        // total across the revision's files, which for a single file push is
        // the same number it always was
        let size_bytes = body
            .files
            .iter()
            .map(|file| file.content.len())
            .sum::<usize>() as i64;
        let inserted = sqlx::query!(
            r#"INSERT INTO revisions (document_id, revision, size_bytes)
               SELECT ?, COALESCE(MAX(revision), 0) + 1, ? FROM revisions WHERE document_id = ?
               RETURNING id as "id!: i64", revision as "revision!: i64""#,
            document_id,
            size_bytes,
            document_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        let revision = inserted.revision;

        for file in &body.files {
            let file_size = file.content.len() as i64;
            // the object is stored before the row records it, so a transaction
            // that never commits leaves an orphan object rather than a row
            // pointing at bytes that were never written
            let object_key = match blobs.0 {
                Some(blobs) if file_size > crate::blobs::INLINE_LIMIT => {
                    Some(blobs.put(&file.content).await.map_err(blob_failed)?)
                }
                _ => None,
            };
            let content = object_key.is_none().then_some(&file.content);
            sqlx::query!(
                "INSERT INTO revision_files
                     (revision_id, path, content, object_key, content_type, size_bytes)
                 VALUES (?, ?, ?, ?, ?, ?)",
                inserted.id,
                file.path,
                content,
                object_key,
                file.content_type,
                file_size
            )
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        }

        if let Some(project) = &project {
            ensure_project(&mut tx, owner_id, project)
                .await
                .map_err(internal)?;
        }

        // inside the transaction: the worker must never see a half written revision
        crate::preview::enqueue(&mut tx, inserted.id)
            .await
            .map_err(internal)?;

        if let Some(tags) = &tags {
            replace_tags(&mut tx, document_id, tags)
                .await
                .map_err(internal)?;
        }

        for (ord, declared) in questions.iter().enumerate() {
            let ord = ord as i64;
            let data = serde_json::to_string(declared).map_err(internal)?;
            sqlx::query!(
                "INSERT INTO revision_questions (revision_id, ord, key, data) VALUES (?, ?, ?, ?)",
                inserted.id,
                ord,
                declared.key,
                data
            )
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        }

        tx.commit().await.map_err(internal)?;

        let url = format!("{}/{}/{}", docs_url.0.0.as_str(), public_id, slug);
        Ok(PushResponse::Ok(Json(PushedBody {
            id: public_id,
            slug,
            revision,
            size_bytes,
            url,
            files: body
                .files
                .iter()
                .map(|file| FileBody {
                    path: file.path.clone(),
                    size_bytes: file.content.len() as i64,
                    content_type: file.content_type.to_string(),
                })
                .collect(),
        })))
    }

    /// List your documents.
    #[oai(path = "/docs", method = "get")]
    async fn list(
        &self,
        pool: Data<&SqlitePool>,
        docs_url: Data<&DocsUrl>,
        auth: Auth,
        /// Restrict to one project. An alias resolves to the project it names.
        project: Query<Option<String>>,
        /// Newest first; useful for reading only the last few of a project.
        limit: Query<Option<i64>>,
    ) -> poem::Result<Json<Vec<DocumentBody>>> {
        let project = match project.0 {
            Some(name) => Some(
                resolve_project(pool.0, auth.user().id, &name)
                    .await
                    .map_err(internal)?,
            ),
            None => None,
        };
        // question counts are scoped to the latest revision, so a question a
        // past revision asked does not keep a document looking unfinished
        let rows = sqlx::query!(
            r#"SELECT d.id as "id!: i64", d.public_id as "public_id!: String",
                      d.slug as "slug!: String", d.title, d.project,
                      d.published as "published!: bool",
                      d.created_at as "created_at!: String", d.updated_at as "updated_at!: String",
                      COUNT(r.id) as "revision_count!: i64",
                      COALESCE(MAX(r.revision), 0) as "latest_revision!: i64",
                      COALESCE(MAX(r.created_at), d.created_at) as "last_pushed_at!: String",
                      (SELECT COUNT(*) FROM revision_questions q
                        WHERE q.revision_id = (SELECT id FROM revisions
                                                WHERE document_id = d.id
                                                ORDER BY revision DESC LIMIT 1)
                      ) as "questions_total!: i64",
                      (SELECT COUNT(*) FROM revision_questions q
                        JOIN document_answers a
                          ON a.document_id = d.id AND a.key = q.key
                        WHERE q.revision_id = (SELECT id FROM revisions
                                                WHERE document_id = d.id
                                                ORDER BY revision DESC LIMIT 1)
                      ) as "questions_answered!: i64"
               FROM documents d LEFT JOIN revisions r ON r.document_id = d.id
               WHERE d.owner_id = ? AND (? IS NULL OR d.project = ?)
               GROUP BY d.id
               ORDER BY COALESCE(MAX(r.created_at), d.created_at) DESC, d.id DESC
               LIMIT COALESCE(?, -1)"#,
            auth.user().id,
            project,
            project,
            limit.0
        )
        .fetch_all(pool.0)
        .await
        .map_err(internal)?;

        let mut tags = tags_for(pool.0, auth.user().id).await.map_err(internal)?;

        Ok(Json(
            rows.into_iter()
                .map(|row| DocumentBody {
                    url: format!("{}/{}/{}", docs_url.0.0.as_str(), row.public_id, row.slug),
                    tags: tags.remove(&row.id).unwrap_or_default(),
                    id: row.public_id,
                    slug: row.slug,
                    title: row.title,
                    project: row.project,
                    published: row.published,
                    revision_count: row.revision_count,
                    latest_revision: row.latest_revision,
                    questions_total: row.questions_total,
                    questions_answered: row.questions_answered,
                    last_pushed_at: row.last_pushed_at,
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
        docs_url: Data<&DocsUrl>,
        auth: Auth,
        slug: Path<String>,
    ) -> poem::Result<DocumentResponse> {
        match document_detail(pool.0, docs_url.0, auth.user().id, &slug.0).await? {
            Some(body) => Ok(DocumentResponse::Ok(Json(Box::new(body)))),
            None => Ok(DocumentResponse::NotFound),
        }
    }

    /// Delete a document, its revisions and their bodies.
    ///
    /// Signed in only, like publishing. An agent's token can create and revise
    /// but cannot destroy, so a confused agent cannot take a document with it.
    #[oai(path = "/docs/:slug", method = "delete")]
    async fn delete(
        &self,
        pool: Data<&SqlitePool>,
        blobs: Data<&Option<crate::blobs::Blobs>>,
        session: SessionAuth,
        slug: Path<String>,
    ) -> poem::Result<DeleteDocumentResponse> {
        // gathered before the rows go: afterwards there is nothing left to say
        // which objects this document was holding
        let keys = sqlx::query_scalar!(
            r#"SELECT object_key as "object_key!: String" FROM revision_files f
               JOIN revisions r ON r.id = f.revision_id
               JOIN documents d ON d.id = r.document_id
               WHERE d.owner_id = ? AND d.slug = ? AND f.object_key IS NOT NULL
               UNION
               SELECT object_key as "object_key!: String" FROM revision_previews p
               JOIN revisions r ON r.id = p.revision_id
               JOIN documents d ON d.id = r.document_id
               WHERE d.owner_id = ? AND d.slug = ? AND p.object_key IS NOT NULL"#,
            session.0.id,
            slug.0,
            session.0.id,
            slug.0
        )
        .fetch_all(pool.0)
        .await
        .map_err(internal)?;

        // revisions cascade from the document, and files and previews from the
        // revision, so one delete takes the whole tree
        let deleted = sqlx::query!(
            "DELETE FROM documents WHERE owner_id = ? AND slug = ?",
            session.0.id,
            slug.0
        )
        .execute(pool.0)
        .await
        .map_err(internal)?
        .rows_affected();
        if deleted == 0 {
            return Ok(DeleteDocumentResponse::NotFound);
        }

        // the rows are already gone, so a bucket that will not cooperate leaves
        // an orphan object rather than failing the delete the reader asked for
        if let Some(blobs) = blobs.0 {
            for key in keys {
                match still_referenced(pool.0, &key).await {
                    Ok(true) => continue,
                    Ok(false) => {
                        if let Err(error) = blobs.delete(&key).await {
                            tracing::warn!(%key, %error, "orphaned an object");
                        }
                    }
                    Err(error) => tracing::warn!(%key, %error, "cannot tell if an object is free"),
                }
            }
        }

        Ok(DeleteDocumentResponse::Done)
    }

    /// Correct a document's title, project or tags without pushing a revision.
    /// This is how documents pushed before projects existed get sorted.
    #[oai(path = "/docs/:slug", method = "patch")]
    async fn patch(
        &self,
        pool: Data<&SqlitePool>,
        docs_url: Data<&DocsUrl>,
        auth: Auth,
        slug: Path<String>,
        body: Json<PatchRequest>,
    ) -> poem::Result<PatchResponse> {
        if let Some(project) = &body.0.project
            && !valid_slug(project)
        {
            return Ok(PatchResponse::Invalid(PlainText(
                "project must match [a-z0-9-]{1,64}".to_string(),
            )));
        }
        let tags = match body.0.tags.as_deref().map(normalize_tags).transpose() {
            Ok(tags) => tags,
            Err(message) => return Ok(PatchResponse::Invalid(PlainText(message))),
        };
        let project = match &body.0.project {
            Some(name) => Some(
                resolve_project(pool.0, auth.user().id, name)
                    .await
                    .map_err(internal)?,
            ),
            None => None,
        };

        let mut tx = pool.0.begin().await.map_err(internal)?;
        let document_id = sqlx::query_scalar!(
            r#"UPDATE documents
               SET title = COALESCE(?, title), project = COALESCE(?, project),
                   updated_at = datetime('now')
               WHERE owner_id = ? AND slug = ?
               RETURNING id as "id!: i64""#,
            body.0.title,
            project,
            auth.user().id,
            slug.0
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        let Some(document_id) = document_id else {
            return Ok(PatchResponse::NotFound);
        };
        if let Some(project) = &project {
            ensure_project(&mut tx, auth.user().id, project)
                .await
                .map_err(internal)?;
        }
        if let Some(tags) = &tags {
            replace_tags(&mut tx, document_id, tags)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;

        match document_detail(pool.0, docs_url.0, auth.user().id, &slug.0).await? {
            Some(body) => Ok(PatchResponse::Ok(Json(Box::new(body)))),
            None => Ok(PatchResponse::NotFound),
        }
    }

    /// The latest revision's HTML.
    #[oai(path = "/docs/:slug/raw", method = "get")]
    async fn raw_latest(
        &self,
        pool: Data<&SqlitePool>,
        blobs: Data<&Option<crate::blobs::Blobs>>,
        auth: Auth,
        slug: Path<String>,
    ) -> poem::Result<RawResponse> {
        let row = sqlx::query!(
            r#"SELECT f.content as "content: Vec<u8>", f.object_key as "object_key: String"
               FROM revision_files f
               JOIN revisions r ON r.id = f.revision_id
               JOIN documents d ON d.id = r.document_id
               WHERE d.owner_id = ? AND d.slug = ? AND f.path = 'index.html'
               ORDER BY r.revision DESC LIMIT 1"#,
            auth.user().id,
            slug.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;

        Ok(match row {
            Some(row) => raw_ok(
                crate::blobs::resolve(blobs.0.as_ref(), row.content, row.object_key)
                    .await
                    .map_err(blob_failed)?,
            ),
            None => RawResponse::NotFound,
        })
    }

    /// The HTML as of a specific revision.
    #[oai(path = "/docs/:slug/revisions/:revision/raw", method = "get")]
    async fn raw_revision(
        &self,
        pool: Data<&SqlitePool>,
        blobs: Data<&Option<crate::blobs::Blobs>>,
        auth: Auth,
        slug: Path<String>,
        revision: Path<i64>,
    ) -> poem::Result<RawResponse> {
        let row = sqlx::query!(
            r#"SELECT f.content as "content: Vec<u8>", f.object_key as "object_key: String"
               FROM revision_files f
               JOIN revisions r ON r.id = f.revision_id
               JOIN documents d ON d.id = r.document_id
               WHERE d.owner_id = ? AND d.slug = ? AND r.revision = ? AND f.path = 'index.html'"#,
            auth.user().id,
            slug.0,
            revision.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;
        let html = match row {
            Some(row) => Some(
                crate::blobs::resolve(blobs.0.as_ref(), row.content, row.object_key)
                    .await
                    .map_err(blob_failed)?,
            ),
            None => None,
        };

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
        docs_url: Data<&DocsUrl>,
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
                url: format!("{}/{}/{}", docs_url.0.0.as_str(), public_id, slug.0),
            })),
            None => PublishResponse::NotFound,
        })
    }

    /// A revision's thumbnail. Owner only, and never referenced from the public
    /// URL: a thumbnail past the password gate would undo the gate.
    #[oai(path = "/docs/:slug/preview", method = "get")]
    async fn preview(
        &self,
        pool: Data<&SqlitePool>,
        blobs: Data<&Option<crate::blobs::Blobs>>,
        auth: Auth,
        slug: Path<String>,
        scheme: Query<Option<crate::api::projects::Scheme>>,
        revision: Query<Option<i64>>,
    ) -> poem::Result<PreviewImageResponse> {
        let wanted = match scheme.0.unwrap_or(crate::api::projects::Scheme::Light) {
            crate::api::projects::Scheme::Light => "light",
            crate::api::projects::Scheme::Dark => "dark",
        };
        let row = sqlx::query!(
            r#"SELECT p.image as "image: Vec<u8>", p.object_key as "object_key: String",
                      p.content_type as "content_type!: String"
               FROM revision_previews p
               JOIN revisions r ON r.id = p.revision_id
               JOIN documents d ON d.id = r.document_id
               WHERE d.owner_id = ? AND d.slug = ? AND p.scheme = ? AND p.status = 'ready'
                 AND r.revision = COALESCE(?, (SELECT MAX(revision) FROM revisions
                                               WHERE document_id = d.id))"#,
            auth.user().id,
            slug.0,
            wanted,
            revision.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;

        Ok(match row {
            Some(row) => PreviewImageResponse::Ok(
                poem_openapi::payload::Binary(
                    crate::blobs::resolve(blobs.0.as_ref(), row.image, row.object_key)
                        .await
                        .map_err(blob_failed)?,
                ),
                row.content_type,
                "private, max-age=300".to_string(),
            ),
            None => PreviewImageResponse::NotFound,
        })
    }

    /// Queue the latest revision's thumbnail to be rendered again.
    ///
    /// A stored preview is never revisited on its own, so a thumbnail captured
    /// by a broken renderer, or before the page's own assets existed, would
    /// otherwise stay wrong forever.
    #[oai(path = "/docs/:slug/preview/refresh", method = "post")]
    async fn refresh_preview(
        &self,
        pool: Data<&SqlitePool>,
        session: SessionAuth,
        slug: Path<String>,
    ) -> poem::Result<RefreshPreviewResponse> {
        let revision_id = sqlx::query_scalar!(
            r#"SELECT r.id as "id!: i64" FROM revisions r
               JOIN documents d ON d.id = r.document_id
               WHERE d.owner_id = ? AND d.slug = ?
               ORDER BY r.revision DESC LIMIT 1"#,
            session.0.id,
            slug.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;
        let Some(revision_id) = revision_id else {
            return Ok(RefreshPreviewResponse::NotFound);
        };

        let mut tx = pool.0.begin().await.map_err(internal)?;
        crate::preview::enqueue(&mut tx, revision_id)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(RefreshPreviewResponse::Queued)
    }

    /// The latest revision's questions, each with the owner's answer or null.
    #[oai(path = "/docs/:slug/questions", method = "get")]
    async fn questions(
        &self,
        pool: Data<&SqlitePool>,
        auth: Auth,
        slug: Path<String>,
        revision: Query<Option<i64>>,
    ) -> poem::Result<QuestionsResponse> {
        let document_id = sqlx::query_scalar!(
            r#"SELECT id as "id!: i64" FROM documents WHERE owner_id = ? AND slug = ?"#,
            auth.user().id,
            slug.0
        )
        .fetch_optional(pool.0)
        .await
        .map_err(internal)?;
        let Some(document_id) = document_id else {
            return Ok(QuestionsResponse::NotFound);
        };

        let questions = answered_questions(pool.0, document_id, revision.0).await?;
        Ok(QuestionsResponse::Ok(Json(questions)))
    }

    /// Record one answer. Accepts a browser session or a scoped answer key,
    /// never a normal API token.
    #[oai(path = "/docs/:slug/answers/:key", method = "put")]
    async fn answer(
        &self,
        pool: Data<&SqlitePool>,
        docs_url: Data<&DocsUrl>,
        grant: AnswerAuth,
        slug: Path<String>,
        key: Path<String>,
        body: Json<AnswerRequest>,
    ) -> poem::Result<AnswerResponse> {
        let Some(document_id) = resolve_answerable(pool.0, &grant, &slug.0).await? else {
            return Ok(AnswerResponse::NotFound(cors(docs_url.0)));
        };

        let declared = latest_questions(pool.0, document_id).await?;
        let Some(question) = declared.into_iter().find(|q| q.key == key.0) else {
            return Ok(AnswerResponse::NotFound(cors(docs_url.0)));
        };

        let selected = body.0.selected;
        let other_text = body.0.other_text.filter(|text| !text.trim().is_empty());
        let notes = body.0.notes.filter(|text| !text.trim().is_empty());
        if let Err(message) = question::check_answer(&question, &selected, other_text.as_deref()) {
            return Ok(AnswerResponse::Invalid(
                PlainText(message),
                cors(docs_url.0),
            ));
        }
        if [other_text.as_deref(), notes.as_deref()]
            .into_iter()
            .flatten()
            .any(|text| text.chars().count() > question::MAX_TEXT)
        {
            return Ok(AnswerResponse::Invalid(
                PlainText(format!(
                    "text fields are limited to {} characters",
                    question::MAX_TEXT
                )),
                cors(docs_url.0),
            ));
        }

        let encoded = serde_json::to_string(&selected).map_err(internal)?;
        let answered_at = sqlx::query_scalar!(
            r#"INSERT INTO document_answers (document_id, key, selected, other_text, notes)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT (document_id, key) DO UPDATE SET
                 selected = excluded.selected,
                 other_text = excluded.other_text,
                 notes = excluded.notes,
                 answered_at = datetime('now')
               RETURNING answered_at as "answered_at!: String""#,
            document_id,
            key.0,
            encoded,
            other_text,
            notes
        )
        .fetch_one(pool.0)
        .await
        .map_err(internal)?;

        Ok(AnswerResponse::Ok(
            Json(Answer {
                selected,
                other_text,
                notes,
                answered_at,
            }),
            cors(docs_url.0),
        ))
    }

    /// Withdraw an answer, returning the question to unanswered.
    #[oai(path = "/docs/:slug/answers/:key", method = "delete")]
    async fn clear_answer(
        &self,
        pool: Data<&SqlitePool>,
        docs_url: Data<&DocsUrl>,
        grant: AnswerAuth,
        slug: Path<String>,
        key: Path<String>,
    ) -> poem::Result<ClearAnswerResponse> {
        let Some(document_id) = resolve_answerable(pool.0, &grant, &slug.0).await? else {
            return Ok(ClearAnswerResponse::NotFound(cors(docs_url.0)));
        };
        sqlx::query!(
            "DELETE FROM document_answers WHERE document_id = ? AND key = ?",
            document_id,
            key.0
        )
        .execute(pool.0)
        .await
        .map_err(internal)?;
        Ok(ClearAnswerResponse::Done(cors(docs_url.0)))
    }

    /// CORS preflight for the two answer routes. The widget calls from the docs
    /// origin, which is not the origin this API answers on.
    #[oai(path = "/docs/:slug/answers/:key", method = "options")]
    async fn answer_preflight(
        &self,
        docs_url: Data<&DocsUrl>,
        _slug: Path<String>,
        _key: Path<String>,
    ) -> PreflightResponse {
        PreflightResponse::Ok(
            cors(docs_url.0),
            "authorization, content-type".to_string(),
            "PUT, DELETE, OPTIONS".to_string(),
            "600".to_string(),
        )
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

async fn document_detail(
    pool: &SqlitePool,
    docs_url: &DocsUrl,
    owner_id: i64,
    slug: &str,
) -> poem::Result<Option<DocumentDetailBody>> {
    let row = sqlx::query!(
        r#"SELECT id as "id!: i64", public_id as "public_id!: String", title, project,
                  published as "published!: bool",
                  created_at as "created_at!: String", updated_at as "updated_at!: String"
           FROM documents WHERE owner_id = ? AND slug = ?"#,
        owner_id,
        slug
    )
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(row) = row else {
        return Ok(None);
    };

    let revisions = sqlx::query!(
        r#"SELECT revision as "revision!: i64", size_bytes as "size_bytes!: i64",
                  created_at as "created_at!: String"
           FROM revisions WHERE document_id = ? ORDER BY revision"#,
        row.id
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?;

    let mut files: HashMap<i64, Vec<FileBody>> = HashMap::new();
    for file in sqlx::query!(
        r#"SELECT r.revision as "revision!: i64", f.path as "path!: String",
                  f.size_bytes as "size_bytes!: i64", f.content_type as "content_type!: String"
           FROM revision_files f JOIN revisions r ON r.id = f.revision_id
           WHERE r.document_id = ? ORDER BY r.revision, f.path"#,
        row.id
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?
    {
        files.entry(file.revision).or_default().push(FileBody {
            path: file.path,
            size_bytes: file.size_bytes,
            content_type: file.content_type,
        });
    }

    let tags = sqlx::query_scalar!(
        r#"SELECT tag as "tag!: String" FROM document_tags
           WHERE document_id = ? ORDER BY tag"#,
        row.id
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?;

    Ok(Some(DocumentDetailBody {
        url: format!("{}/{}/{}", docs_url.0.as_str(), row.public_id, slug),
        id: row.public_id,
        slug: slug.to_string(),
        title: row.title,
        project: row.project,
        tags,
        published: row.published,
        created_at: row.created_at,
        updated_at: row.updated_at,
        revisions: revisions
            .into_iter()
            .map(|r| RevisionBody {
                files: files.remove(&r.revision).unwrap_or_default(),
                revision: r.revision,
                size_bytes: r.size_bytes,
                created_at: r.created_at,
            })
            .collect(),
        questions: answered_questions(pool, row.id, None).await?,
    }))
}

/// The document a grant may write, if it may write this slug at all.
async fn resolve_answerable(
    pool: &SqlitePool,
    grant: &AnswerAuth,
    slug: &str,
) -> poem::Result<Option<i64>> {
    let id = match grant {
        AnswerAuth::Session(user) => sqlx::query_scalar!(
            r#"SELECT id as "id!: i64" FROM documents WHERE owner_id = ? AND slug = ?"#,
            user.id,
            slug
        )
        .fetch_optional(pool)
        .await
        .map_err(internal)?,
        // a key minted for one document cannot be pointed at another, even by
        // the document that carries it
        AnswerAuth::Scoped(document_id) => sqlx::query_scalar!(
            r#"SELECT id as "id!: i64" FROM documents WHERE id = ? AND slug = ?"#,
            document_id,
            slug
        )
        .fetch_optional(pool)
        .await
        .map_err(internal)?,
    };
    Ok(id)
}

/// Questions as declared by a revision: the given one, or the latest.
pub async fn revision_questions(
    pool: &SqlitePool,
    document_id: i64,
    revision: Option<i64>,
) -> poem::Result<Vec<Question>> {
    let rows = sqlx::query_scalar!(
        r#"SELECT q.data as "data!: String"
           FROM revision_questions q JOIN revisions r ON r.id = q.revision_id
           WHERE r.document_id = ?
             AND r.revision = COALESCE(?, (SELECT MAX(revision) FROM revisions WHERE document_id = ?))
           ORDER BY q.ord"#,
        document_id,
        revision,
        document_id
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?;

    rows.iter()
        .map(|data| serde_json::from_str(data).map_err(internal))
        .collect()
}

async fn latest_questions(pool: &SqlitePool, document_id: i64) -> poem::Result<Vec<Question>> {
    revision_questions(pool, document_id, None).await
}

/// Each declared question joined to the owner's answer, which lives on the
/// document and so survives a new revision re-declaring the same key.
pub async fn answered_questions(
    pool: &SqlitePool,
    document_id: i64,
    revision: Option<i64>,
) -> poem::Result<Vec<AnsweredQuestion>> {
    let questions = revision_questions(pool, document_id, revision).await?;
    if questions.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query!(
        r#"SELECT key as "key!: String", selected as "selected!: String",
                  other_text, notes, answered_at as "answered_at!: String"
           FROM document_answers WHERE document_id = ?"#,
        document_id
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?;

    Ok(questions
        .into_iter()
        .map(|question| {
            let answer = rows
                .iter()
                .find(|row| row.key == question.key)
                .and_then(|row| {
                    Some(Answer {
                        selected: serde_json::from_str(&row.selected).ok()?,
                        other_text: row.other_text.clone(),
                        notes: row.notes.clone(),
                        answered_at: row.answered_at.clone(),
                    })
                });
            AnsweredQuestion { question, answer }
        })
        .collect())
}

pub fn valid_slug(slug: &str) -> bool {
    (1..=64).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether any row still points at an object.
///
/// A key is the hash of its contents, so two documents that ship the same
/// image share one object. Deleting on the strength of one document's rows
/// would pull the bytes out from under the other, which is why this is checked
/// across every owner rather than only the one deleting.
async fn still_referenced(pool: &SqlitePool, key: &str) -> Result<bool, sqlx::Error> {
    let count = sqlx::query_scalar!(
        r#"SELECT (
             EXISTS (SELECT 1 FROM revision_files WHERE object_key = ?)
             OR EXISTS (SELECT 1 FROM revision_previews WHERE object_key = ?)
           ) as "referenced!: bool""#,
        key,
        key
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// A project exists because a document names it. The row carries its settings,
/// so it is created on first mention rather than by a separate call.
async fn ensure_project(
    tx: &mut sqlx::SqliteConnection,
    owner_id: i64,
    project: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT OR IGNORE INTO projects (owner_id, slug) VALUES (?, ?)",
        owner_id,
        project
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// The canonical project a name refers to. An unknown name is its own project,
/// which is what makes "create on first push" work; a known alias resolves to
/// the project it points at, so `openlv` and `open-lavatory` stay one pile.
pub async fn resolve_project(
    pool: &SqlitePool,
    owner_id: i64,
    name: &str,
) -> Result<String, sqlx::Error> {
    let alias = sqlx::query_scalar!(
        r#"SELECT project as "project!: String" FROM project_aliases
           WHERE owner_id = ? AND alias = ?"#,
        owner_id,
        name
    )
    .fetch_optional(pool)
    .await?;
    Ok(alias.unwrap_or_else(|| name.to_string()))
}

const MAX_TAGS: usize = 8;

/// Tags are a living vocabulary: the server never checks one against a list, it
/// only puts it in a shape two callers can agree on. "PR Review" and
/// "pr review" both land on `pr-review` and so on the same row.
fn normalize_tag(tag: &str) -> Result<String, String> {
    let normalized = tag
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    let valid = (1..=32).contains(&normalized.len())
        && normalized
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(normalized)
    } else {
        Err(format!(
            "tag {tag:?} must normalise to [a-z0-9-]{{1,32}}, got {normalized:?}"
        ))
    }
}

fn normalize_tags(tags: &[String]) -> Result<Vec<String>, String> {
    if tags.len() > MAX_TAGS {
        return Err(format!("at most {MAX_TAGS} tags per document"));
    }
    let mut out: Vec<String> = Vec::with_capacity(tags.len());
    for tag in tags {
        let normalized = normalize_tag(tag)?;
        if !out.contains(&normalized) {
            out.push(normalized);
        }
    }
    Ok(out)
}

async fn replace_tags(
    tx: &mut sqlx::SqliteConnection,
    document_id: i64,
    tags: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM document_tags WHERE document_id = ?",
        document_id
    )
    .execute(&mut *tx)
    .await?;
    for tag in tags {
        sqlx::query!(
            "INSERT INTO document_tags (document_id, tag) VALUES (?, ?)",
            document_id,
            tag
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// Tags for many documents at once. They cannot join into the list query's
/// GROUP BY without multiplying its rows, so they come back as a second query
/// keyed by the ids just fetched.
async fn tags_for(
    pool: &SqlitePool,
    owner_id: i64,
) -> Result<std::collections::HashMap<i64, Vec<String>>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT t.document_id as "document_id!: i64", t.tag as "tag!: String"
           FROM document_tags t JOIN documents d ON d.id = t.document_id
           WHERE d.owner_id = ?
           ORDER BY t.tag"#,
        owner_id
    )
    .fetch_all(pool)
    .await?;

    let mut map: std::collections::HashMap<i64, Vec<String>> = std::collections::HashMap::new();
    for row in rows {
        map.entry(row.document_id).or_default().push(row.tag);
    }
    Ok(map)
}
