use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use poem::http::{StatusCode, header};
use poem::web::cookie::{Cookie, SameSite};
use poem::web::{Data, Form, Path, RealIp};
use poem::{Request, Response, handler};
use sha2::Sha256;
use sqlx::SqlitePool;

use crate::config::{BaseUrl, Secret};
use crate::rate_limit::RateLimiter;
use crate::{auth, static_files};

const DOC_ACCESS_COOKIE: &str = "doc_access";
const ACCESS_DAYS: u64 = 7;
/// Lets the sandboxed document load its own assets.
///
/// A document runs in an opaque origin, whose site for cookies is null, so a
/// SameSite=Lax cookie is not sent even on the document's own subresource
/// requests. Those requests carry no Origin we can trust, no Referer (the page
/// is served no-referrer), and no way to add a header, so a cookie that ignores
/// SameSite is the only thing that reaches them.
///
/// SameSite=None means any site can cause this cookie to be sent, so it is
/// bound to one document, expires in a day, and grants exactly one thing:
/// reading that document's assets. The entry document still needs real
/// authorisation, and nothing here can write.
const DOC_ASSETS_COOKIE: &str = "doc_assets";
const ASSETS_HOURS: u64 = 24;

fn is_public_id_shape(segment: &str) -> bool {
    segment.len() == 10 && segment.bytes().all(|b| b.is_ascii_alphanumeric())
}

struct Doc {
    id: i64,
    owner_id: i64,
    slug: String,
    title: Option<String>,
    project: Option<String>,
    published: bool,
    password_hash: Option<String>,
}

#[handler]
pub async fn view_latest(
    req: &Request,
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    Path((public_id, slug)): Path<(String, String)>,
) -> Response {
    serve(req, pool.0, secret.0, &public_id, &slug, None).await
}

#[handler]
pub async fn view_revision(
    req: &Request,
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    Path((public_id, slug, revision)): Path<(String, String, i64)>,
) -> Response {
    serve(req, pool.0, secret.0, &public_id, &slug, Some(revision)).await
}

/// A document is a directory now, so a relative `<script src="chart.js">`
/// resolves inside it. The old path answers with a 308, which preserves the
/// method, so bookmarks and the unlock POST both still land.
#[handler]
pub fn redirect_to_dir(Path((public_id, slug)): Path<(String, String)>) -> Response {
    if !is_public_id_shape(&public_id) {
        return static_files::serve_or_index(&format!("{public_id}/{slug}"));
    }
    Response::builder()
        .status(StatusCode::PERMANENT_REDIRECT)
        .header(header::LOCATION, format!("/{public_id}/{slug}/"))
        .finish()
}

#[handler]
pub fn redirect_revision_to_dir(
    Path((public_id, slug, revision)): Path<(String, String, i64)>,
) -> Response {
    Response::builder()
        .status(StatusCode::PERMANENT_REDIRECT)
        .header(
            header::LOCATION,
            format!("/{public_id}/{slug}/rev/{revision}/"),
        )
        .finish()
}

/// An asset of the latest revision, or of a pinned one.
#[handler]
pub async fn asset_latest(
    req: &Request,
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    Path((public_id, slug, path)): Path<(String, String, String)>,
) -> Response {
    serve_asset(req, pool.0, secret.0, &public_id, &slug, None, &path).await
}

#[handler]
pub async fn asset_revision(
    req: &Request,
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    Path((public_id, slug, revision, path)): Path<(String, String, i64, String)>,
) -> Response {
    serve_asset(
        req,
        pool.0,
        secret.0,
        &public_id,
        &slug,
        Some(revision),
        &path,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn serve_asset(
    req: &Request,
    pool: &SqlitePool,
    secret: &Secret,
    public_id: &str,
    slug: &str,
    revision: Option<i64>,
    path: &str,
) -> Response {
    // poem matches the directory URL itself against this wildcard with an empty
    // remainder, so that case is the document rather than a missing asset
    if path.is_empty() {
        return serve(req, pool, secret, public_id, slug, revision).await;
    }
    if !is_public_id_shape(public_id) {
        return static_files::serve_or_index(&format!("{public_id}/{slug}/{path}"));
    }
    let Some(doc) = fetch_doc(pool, public_id).await else {
        return not_found();
    };
    if doc.slug != slug || !is_authorized(req, pool, secret, &doc).await {
        // a private document must be indistinguishable from a missing one, and
        // so must its assets
        return not_found();
    }

    let row = sqlx::query!(
        r#"SELECT f.content as "content!: Vec<u8>", f.content_type as "content_type!: String"
           FROM revision_files f
           JOIN revisions r ON r.id = f.revision_id
           WHERE r.document_id = ? AND f.path = ?
             AND r.revision = COALESCE(?, (SELECT MAX(revision) FROM revisions
                                           WHERE document_id = ?))"#,
        doc.id,
        path,
        revision,
        doc.id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(row) = row else {
        return not_found();
    };
    // a pinned revision never changes; the latest pointer does
    let cache = if revision.is_some() {
        "private, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .content_type(row.content_type)
        .header(header::CACHE_CONTROL, cache)
        .header(header::CONTENT_SECURITY_POLICY, "sandbox")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
        .body(row.content)
}

#[derive(serde::Deserialize)]
pub struct UnlockForm {
    password: String,
}

#[handler]
pub async fn unlock(
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    base_url: Data<&BaseUrl>,
    limiter: Data<&RateLimiter>,
    real_ip: RealIp,
    Path((public_id, slug)): Path<(String, String)>,
    Form(form): Form<UnlockForm>,
) -> Response {
    do_unlock(
        pool.0, secret.0, base_url.0, limiter.0, real_ip, public_id, slug, form,
    )
    .await
}

/// The same POST at the directory URL. Poem prefers the trailing wildcard over
/// a bare `/:a/:b/`, so both methods hang off the wildcard and an empty
/// remainder means the document itself.
#[handler]
pub async fn unlock_at_dir(
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    base_url: Data<&BaseUrl>,
    limiter: Data<&RateLimiter>,
    real_ip: RealIp,
    Path((public_id, slug, path)): Path<(String, String, String)>,
    Form(form): Form<UnlockForm>,
) -> Response {
    if !path.is_empty() {
        return not_found();
    }
    do_unlock(
        pool.0, secret.0, base_url.0, limiter.0, real_ip, public_id, slug, form,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn do_unlock(
    pool: &SqlitePool,
    secret: &Secret,
    base_url: &BaseUrl,
    limiter: &RateLimiter,
    real_ip: RealIp,
    public_id: String,
    slug: String,
    form: UnlockForm,
) -> Response {
    if !is_public_id_shape(&public_id) {
        return not_found();
    }
    if !limiter.allow(real_ip.0) {
        return too_many_requests();
    }
    let Some(doc) = fetch_doc(pool, &public_id).await else {
        return not_found();
    };
    let (Some(password_hash), true) = (doc.password_hash.clone(), doc.published) else {
        return not_found();
    };
    if doc.slug != slug {
        return redirect_canonical(&public_id, &doc.slug, None);
    }

    if !auth::verify_password(form.password, password_hash.clone()).await {
        return password_form(&public_id, &slug, true);
    }

    let expiry = unix_now() + (ACCESS_DAYS * 24 * 3600) as i64;
    let mac = access_mac(&secret.0, doc.id, expiry, &password_hash);
    let mut cookie = Cookie::new_with_str(DOC_ACCESS_COOKIE, format!("{expiry}.{mac}"));
    cookie.set_path(format!("/{public_id}/{slug}"));
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(base_url.is_https());
    cookie.set_max_age(std::time::Duration::from_secs(ACCESS_DAYS * 24 * 3600));

    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, format!("/{public_id}/{slug}"))
        .header(header::SET_COOKIE, cookie.to_string())
        .finish()
}

async fn serve(
    req: &Request,
    pool: &SqlitePool,
    secret: &Secret,
    public_id: &str,
    slug: &str,
    revision: Option<i64>,
) -> Response {
    // two-segment paths that cannot be document URLs are static assets or SPA
    // client routes
    if !is_public_id_shape(public_id) {
        return static_files::serve_or_index(&format!("{public_id}/{slug}"));
    }
    let Some(doc) = fetch_doc(pool, public_id).await else {
        return not_found();
    };
    if doc.slug != slug {
        return redirect_canonical(public_id, &doc.slug, revision);
    }

    let owner = auth::user_from_request(pool, req)
        .await
        .is_some_and(|user| user.id == doc.owner_id);
    let authorized = owner || visitor_unlocked(req, secret, &doc);

    if authorized {
        return document_page(pool, secret, &doc, public_id, revision, owner).await;
    }
    if doc.published {
        return password_form(public_id, slug, false);
    }
    // a private document must be indistinguishable from a missing one
    not_found()
}

fn visitor_unlocked(req: &Request, secret: &Secret, doc: &Doc) -> bool {
    doc.published
        && doc.password_hash.as_deref().is_some_and(|password_hash| {
            has_valid_access_cookie(req, &secret.0, doc.id, password_hash)
        })
}

/// Assets answer to the same gate as the document, plus the assets cookie the
/// document page hands out, which is the only credential a sandboxed page's
/// subresource requests actually carry.
async fn is_authorized(req: &Request, pool: &SqlitePool, secret: &Secret, doc: &Doc) -> bool {
    if has_valid_assets_cookie(req, &secret.0, doc.id) || visitor_unlocked(req, secret, doc) {
        return true;
    }
    auth::user_from_request(pool, req)
        .await
        .is_some_and(|user| user.id == doc.owner_id)
}

fn assets_mac(secret: &str, doc_id: i64, expiry: i64) -> Hmac<Sha256> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(format!("assets.{doc_id}.{expiry}").as_bytes());
    mac
}

fn assets_cookie(secret: &str, public_id: &str, doc: &Doc) -> Cookie {
    let expiry = unix_now() + (ASSETS_HOURS * 3600) as i64;
    let mac = hex_encode(&assets_mac(secret, doc.id, expiry).finalize().into_bytes());
    let mut cookie = Cookie::new_with_str(DOC_ASSETS_COOKIE, format!("{expiry}.{mac}"));
    cookie.set_path(format!("/{public_id}/{}", doc.slug));
    cookie.set_http_only(true);
    // SameSite=None is the point: the document is cross-site to itself. Chrome
    // rejects it without Secure, and treats loopback as a secure context.
    cookie.set_same_site(SameSite::None);
    cookie.set_secure(true);
    cookie.set_max_age(std::time::Duration::from_secs(ASSETS_HOURS * 3600));
    cookie
}

fn has_valid_assets_cookie(req: &Request, secret: &str, doc_id: i64) -> bool {
    let Some(cookie) = req.cookie().get(DOC_ASSETS_COOKIE) else {
        return false;
    };
    let value = cookie.value_str().to_string();
    let Some((expiry, mac_hex)) = value.split_once('.') else {
        return false;
    };
    let Ok(expiry) = expiry.parse::<i64>() else {
        return false;
    };
    if expiry <= unix_now() {
        return false;
    }
    let Some(mac_bytes) = hex_decode(mac_hex) else {
        return false;
    };
    assets_mac(secret, doc_id, expiry)
        .verify_slice(&mac_bytes)
        .is_ok()
}

async fn fetch_doc(pool: &SqlitePool, public_id: &str) -> Option<Doc> {
    sqlx::query_as!(
        Doc,
        r#"SELECT id as "id!: i64", owner_id as "owner_id!: i64", slug as "slug!: String",
                  title, project, published as "published!: bool", password_hash
           FROM documents WHERE public_id = ?"#,
        public_id
    )
    .fetch_optional(pool)
    .await
    .ok()?
}

/// The document HTML served as-is, with the floating viewer overlay appended.
/// Fragment links, scrolling, and printing behave as in the bare document; the
/// sandbox CSP keeps the document's own scripts in an opaque origin.
async fn document_page(
    pool: &SqlitePool,
    secret: &Secret,
    doc: &Doc,
    public_id: &str,
    revision: Option<i64>,
    is_owner: bool,
) -> Response {
    let revisions = sqlx::query_scalar!(
        r#"SELECT revision as "revision!: i64" FROM revisions
           WHERE document_id = ? ORDER BY revision DESC"#,
        doc.id
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let Some(latest) = revisions.first().copied() else {
        return not_found();
    };
    let current = revision.unwrap_or(latest);
    if !revisions.contains(&current) {
        return not_found();
    }

    let html = sqlx::query_scalar!(
        r#"SELECT f.content as "html!: Vec<u8>" FROM revision_files f
           JOIN revisions r ON r.id = f.revision_id
           WHERE r.document_id = ? AND r.revision = ? AND f.path = 'index.html'"#,
        doc.id,
        current
    )
    .fetch_optional(pool)
    .await;
    let mut body = match html {
        Ok(Some(html)) => html,
        Ok(None) => return not_found(),
        Err(_) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .finish();
        }
    };

    // the widget, the question data and the scoped key exist only for the
    // owner: a link and password visitor gets none of the three
    let questions = if is_owner {
        crate::api::docs::answered_questions(pool, doc.id, Some(current))
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // the project's icon, so a reader's browser tab says which project this
    // document belongs to. Appended rather than merged into <head>: the parser
    // honours a link element wherever it appears, and the alternative is
    // rewriting agent HTML.
    if let Some(project) = &doc.project {
        body.extend_from_slice(
            favicon_fragment(pool, doc.owner_id, project)
                .await
                .as_bytes(),
        );
    }

    body.extend_from_slice(
        overlay_fragment(
            public_id,
            doc,
            current,
            latest,
            &revisions,
            is_owner,
            questions.len(),
        )
        .as_bytes(),
    );
    if !questions.is_empty() {
        body.extend_from_slice(answer_widget_fragment(secret, doc, &questions).as_bytes());
    }

    // allow-popups-to-escape-sandbox lets the overlay's Share button open the
    // share page in a normal-origin popup where the session cookie works; the
    // document itself stays in its opaque origin
    Response::builder()
        .content_type("text/html; charset=utf-8")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox",
        )
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
        // whoever was allowed to read the document may load its assets; see
        // DOC_ASSETS_COOKIE for why a cookie is the only thing that works here
        .header(
            header::SET_COOKIE,
            assets_cookie(&secret.0, public_id, doc).to_string(),
        )
        .body(body)
}

/// A compact pill cluster fixed to the top-right corner. Dark on every
/// document so it needs no theme detection, hidden in print, no JavaScript:
/// the revision menu is a details element.
fn overlay_fragment(
    public_id: &str,
    doc: &Doc,
    current: i64,
    latest: i64,
    revisions: &[i64],
    is_owner: bool,
    question_count: usize,
) -> String {
    let slug = &doc.slug;
    let title = html_escape(doc.title.as_deref().unwrap_or(slug));

    let revision_links: String = revisions
        .iter()
        .map(|r| {
            let href = if *r == latest {
                format!("/{public_id}/{slug}")
            } else {
                format!("/{public_id}/{slug}/rev/{r}")
            };
            let marker = if *r == latest { " (current)" } else { "" };
            let viewing = if *r == current {
                r#" class="viewing""#
            } else {
                ""
            };
            format!(r#"<a{viewing} href="{href}">rev {r}{marker}</a>"#)
        })
        .collect();

    let summary = if current == latest {
        format!("rev {current}")
    } else {
        format!("rev {current} of {latest}")
    };

    let share = if is_owner {
        format!(
            r#"<a id="planenv-share" href="/{public_id}/{slug}/share" onclick="window.open(this.href, 'planenv-share', 'width=440,height=600'); return false">Share</a>"#
        )
    } else {
        String::new()
    };

    let answered = if question_count > 0 {
        format!(r#"<span id="planenv-answered">0 of {question_count} answered</span>"#)
    } else {
        String::new()
    };

    format!(
        r#"
<div id="planenv-overlay">
<a id="planenv-brand" href="/" title="{title}">plan<b>.env.md</b></a>
{answered}
<details id="planenv-revs"><summary>{summary}</summary><nav>{revision_links}</nav></details>
{share}
</div>
<style>
#planenv-overlay {{
  position: fixed; top: 10px; right: 10px; z-index: 2147483647;
  display: flex; gap: 6px; align-items: stretch;
  font: 12px/1 system-ui, -apple-system, "Segoe UI", sans-serif;
}}
#planenv-overlay a, #planenv-overlay summary, #planenv-answered {{
  display: flex; align-items: center;
  background: #1c1f22e8; color: #e7e5df;
  border: 1px solid #ffffff26; border-radius: 999px;
  padding: 7px 12px; text-decoration: none; cursor: pointer;
  white-space: nowrap;
}}
#planenv-brand {{ font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; font-weight: 600; }}
#planenv-brand b {{ color: #4cc2a0; }}
#planenv-revs {{ position: relative; }}
#planenv-revs summary {{ list-style: none; font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; }}
#planenv-revs summary::-webkit-details-marker {{ display: none; }}
#planenv-revs nav {{
  position: absolute; right: 0; top: calc(100% + 6px);
  display: flex; flex-direction: column; min-width: 130px;
  background: #1c1f22f2; border: 1px solid #ffffff26;
  border-radius: 10px; padding: 4px;
}}
#planenv-revs nav a {{
  background: none; border: 0; border-radius: 7px; padding: 7px 10px;
}}
#planenv-revs nav a:hover {{ background: #ffffff14; }}
#planenv-revs nav a.viewing {{ color: #4cc2a0; }}
#planenv-share {{ background: #4cc2a0; border-color: transparent; color: #10201b; font-weight: 600; }}
@media print {{ #planenv-overlay {{ display: none; }} }}
</style>
"#
    )
}

/// The project favicon as a data URI, so it works for a link and password
/// visitor who cannot reach the owner-only favicon endpoint. Both schemes are
/// emitted with a media query; browsers pick the one matching the tab theme.
async fn favicon_fragment(pool: &SqlitePool, owner_id: i64, project: &str) -> String {
    use crate::api::projects::{Scheme, load_favicon};

    let mut links = String::new();
    for (scheme, media) in [
        (Scheme::Light, "(prefers-color-scheme: light)"),
        (Scheme::Dark, "(prefers-color-scheme: dark)"),
    ] {
        let Ok(Some((bytes, content_type))) = load_favicon(pool, owner_id, project, scheme).await
        else {
            continue;
        };
        links.push_str(&format!(
            r#"<link rel="icon" media="{media}" type="{content_type}" href="data:{content_type};base64,{}">"#,
            base64::engine::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
        ));
    }
    links
}

/// Owner-only: the question set as inert JSON, plus a scoped key that lets the
/// widget write answers from the document's opaque origin, where the session
/// cookie is never sent.
fn answer_widget_fragment(
    secret: &Secret,
    doc: &Doc,
    questions: &[crate::api::question::AnsweredQuestion],
) -> String {
    let key = crate::answer_key::mint(&secret.0, doc.id);
    let slug = &doc.slug;
    // the HTML parser ends a script block at the first "</script", wherever it
    // appears, so a question containing one would break out of the JSON island;
    // < is still the same string once JSON.parse runs
    let data = serde_json::to_string(questions)
        .unwrap_or_else(|_| "[]".to_string())
        .replace('<', "\\u003c");

    format!(
        r#"
<link rel="stylesheet" href="/_planenv/answer.css">
<script type="application/json" id="planenv-questions">{data}</script>
<script src="/_planenv/answer.js" data-planenv-key="{key}" data-planenv-slug="{slug}"></script>
"#
    )
}

/// The revision's HTML with no overlay, for the preview worker.
///
/// Guarded by the socket peer address rather than `RealIp`: a caller cannot
/// make the kernel report a loopback peer, whereas `X-Forwarded-For` is theirs
/// to write. The ingress connects from a pod address, so only a process inside
/// this container reaches it.
#[handler]
pub async fn render_page(
    req: &Request,
    pool: Data<&SqlitePool>,
    Path(revision_id): Path<i64>,
) -> Response {
    if !is_loopback(req) {
        return not_found();
    }
    serve_revision_file(pool.0, revision_id, "index.html").await
}

/// The revision as the preview worker sees it: the entry document at the
/// directory URL, and its assets beneath. An empty remainder means the entry,
/// the same rule the public asset route uses.
///
/// Guarded by the socket peer address rather than `RealIp`: a caller cannot
/// make the kernel report a loopback peer, whereas `X-Forwarded-For` is theirs
/// to write. The ingress connects from a pod address, so only a process inside
/// this container reaches it.
#[handler]
pub async fn render_asset(
    req: &Request,
    pool: Data<&SqlitePool>,
    Path((revision_id, path)): Path<(i64, String)>,
) -> Response {
    if !is_loopback(req) {
        return not_found();
    }
    let path = if path.is_empty() { "index.html" } else { &path };
    serve_revision_file(pool.0, revision_id, path).await
}

fn is_loopback(req: &Request) -> bool {
    req.remote_addr()
        .as_socket_addr()
        .is_some_and(|addr| addr.ip().is_loopback())
}

async fn serve_revision_file(pool: &SqlitePool, revision_id: i64, path: &str) -> Response {
    let row = sqlx::query!(
        r#"SELECT content as "content!: Vec<u8>", content_type as "content_type!: String"
           FROM revision_files WHERE revision_id = ? AND path = ?"#,
        revision_id,
        path
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    match row {
        Some(row) => Response::builder()
            .content_type(row.content_type)
            .header(header::CONTENT_SECURITY_POLICY, "sandbox allow-scripts")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(row.content),
        None => not_found(),
    }
}

/// The Share popout: a small owner-only page outside the document sandbox.
/// Publishing, rotating, and unpublishing call the existing session-only API
/// from here, where the session cookie is available.
#[handler]
pub async fn share_page(
    req: &Request,
    pool: Data<&SqlitePool>,
    base_url: Data<&BaseUrl>,
    Path((public_id, slug)): Path<(String, String)>,
) -> Response {
    if !is_public_id_shape(&public_id) {
        return not_found();
    }
    let Some(doc) = fetch_doc(pool.0, &public_id).await else {
        return not_found();
    };
    if doc.slug != slug {
        return not_found();
    }
    let is_owner = auth::user_from_request(pool.0, req)
        .await
        .is_some_and(|user| user.id == doc.owner_id);
    // non-owners get the same response as for a missing document
    if !is_owner {
        return not_found();
    }

    let title = html_escape(doc.title.as_deref().unwrap_or(&doc.slug));
    let url = format!("{}/{}/{}", base_url.0.0, public_id, doc.slug);
    let state_line = if doc.published {
        "Published. Anyone with the link and the document password can read it."
    } else {
        "Private. Only you can open it."
    };
    let publish_label = if doc.published {
        "Rotate password"
    } else {
        "Publish"
    };
    let unpublish_button = if doc.published {
        r#"<button id="unpublish" type="button" class="quiet">Unpublish</button>"#
    } else {
        ""
    };

    let html = format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Share: {title}</title>
<style>
  :root {{
    --bg: #f2f1ec; --surface: #faf9f6; --border: #d9d5cb;
    --ink: #20242a; --muted: #6e6a5f; --accent: #196d59;
    --accent-contrast: #f5faf8;
  }}
  @media (prefers-color-scheme: dark) {{
    :root {{
      --bg: #17191c; --surface: #1e2125; --border: #34383d;
      --ink: #d7d9d3; --muted: #8f9389; --accent: #4cc2a0;
      --accent-contrast: #10201b;
    }}
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0; padding: 1.5rem; background: var(--bg); color: var(--ink);
    font: 15px/1.6 system-ui, -apple-system, "Segoe UI", sans-serif;
  }}
  h1 {{ font: 600 16px ui-monospace, "SF Mono", Menlo, Consolas, monospace; margin: 0 0 0.25rem; }}
  p {{ margin: 0.5rem 0; }}
  .muted {{ color: var(--muted); font-size: 13px; }}
  .url {{
    display: flex; gap: 6px; margin: 1rem 0;
  }}
  .url input {{
    flex: 1; min-width: 0; font: 12px ui-monospace, Menlo, Consolas, monospace;
    color: var(--muted); background: var(--surface);
    border: 1px solid var(--border); border-radius: 6px; padding: 0.6em 0.8em;
  }}
  label {{ display: block; font-size: 13px; font-weight: 500; color: var(--muted); margin: 1rem 0 0.3rem; }}
  input[type="password"] {{
    width: 100%; font: inherit; color: var(--ink); background: var(--surface);
    border: 1px solid var(--border); border-radius: 6px; padding: 0.55em 0.8em;
  }}
  .actions {{ display: flex; gap: 8px; margin-top: 1rem; }}
  button {{
    font: 500 14px system-ui, sans-serif; cursor: pointer;
    border-radius: 6px; padding: 0.55em 1em; border: 0;
    background: var(--accent); color: var(--accent-contrast);
  }}
  button.quiet {{
    background: var(--surface); color: var(--ink); border: 1px solid var(--border);
  }}
  #status {{ min-height: 1.2em; font-size: 13px; margin-top: 0.75rem; }}
  #status.error {{ color: #a03030; }}
</style>
</head>
<body>
<h1>{title}</h1>
<p id="state" class="muted">{state_line}</p>
<div class="url">
  <input id="url" readonly value="{url}">
  <button id="copy" type="button" class="quiet">Copy</button>
</div>
<p class="muted">Documents are private by default. Publishing puts the document
behind one password that covers every revision, including future ones.
Rotating the password locks out everyone who has the old one.</p>
<form id="publish-form">
  <label for="password">Document password</label>
  <input id="password" type="password" required minlength="1" autocomplete="new-password">
  <div class="actions">
    <button type="submit">{publish_label}</button>
    {unpublish_button}
  </div>
</form>
<p id="status"></p>
<script>
  const status = document.querySelector("#status");
  const say = (message, isError) => {{
    status.textContent = message;
    status.className = isError ? "error" : "";
  }};

  document.querySelector("#copy").addEventListener("click", () => {{
    navigator.clipboard.writeText(document.querySelector("#url").value)
      .then(() => say("Link copied.", false));
  }});

  document.querySelector("#publish-form").addEventListener("submit", (event) => {{
    event.preventDefault();
    fetch("/api/docs/{slug}/publish", {{
      method: "POST",
      headers: {{ "Content-Type": "application/json" }},
      body: JSON.stringify({{ password: document.querySelector("#password").value }}),
    }}).then((response) => {{
      if (response.ok) location.reload();
      else say("Publishing failed (status " + response.status + ").", true);
    }});
  }});

  const unpublish = document.querySelector("#unpublish");
  if (unpublish) unpublish.addEventListener("click", () => {{
    fetch("/api/docs/{slug}/unpublish", {{ method: "POST" }}).then((response) => {{
      if (response.ok) location.reload();
      else say("Unpublishing failed (status " + response.status + ").", true);
    }});
  }});
</script>
</body>
</html>
"##
    );

    Response::builder()
        .content_type("text/html; charset=utf-8")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(html)
}

fn has_valid_access_cookie(req: &Request, secret: &str, doc_id: i64, password_hash: &str) -> bool {
    let Some(cookie) = req.cookie().get(DOC_ACCESS_COOKIE) else {
        return false;
    };
    let value = cookie.value_str().to_string();
    let Some((expiry, mac_hex)) = value.split_once('.') else {
        return false;
    };
    let Ok(expiry) = expiry.parse::<i64>() else {
        return false;
    };
    if expiry <= unix_now() {
        return false;
    }
    let Some(mac_bytes) = hex_decode(mac_hex) else {
        return false;
    };
    mac_for(secret, doc_id, expiry, password_hash)
        .verify_slice(&mac_bytes)
        .is_ok()
}

// including the password hash in the MAC input means rotating the password or
// re-publishing invalidates every outstanding visitor cookie without server
// state
fn mac_for(secret: &str, doc_id: i64, expiry: i64, password_hash: &str) -> Hmac<Sha256> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(format!("{doc_id}.{expiry}.{password_hash}").as_bytes());
    mac
}

fn access_mac(secret: &str, doc_id: i64, expiry: i64, password_hash: &str) -> String {
    hex_encode(
        &mac_for(secret, doc_id, expiry, password_hash)
            .finalize()
            .into_bytes(),
    )
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

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_secs() as i64
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn not_found() -> Response {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .content_type("text/plain; charset=utf-8")
        .body("not found")
}

fn too_many_requests() -> Response {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .content_type("text/plain; charset=utf-8")
        .body("too many attempts from this address; try again later")
}

fn redirect_canonical(public_id: &str, slug: &str, revision: Option<i64>) -> Response {
    let location = match revision {
        Some(revision) => format!("/{public_id}/{slug}/rev/{revision}/"),
        None => format!("/{public_id}/{slug}/"),
    };
    Response::builder()
        .status(StatusCode::PERMANENT_REDIRECT)
        .header(header::LOCATION, location)
        .finish()
}

// deliberately not part of the SPA: works without JavaScript and never loads
// app code for visitors
fn password_form(public_id: &str, slug: &str, wrong_password: bool) -> Response {
    let error = if wrong_password {
        r#"<p class="error">Wrong password.</p>"#
    } else {
        ""
    };
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{slug}</title>
<style>
  body {{ font: 16px/1.6 system-ui, sans-serif; display: grid; place-items: center;
         min-height: 100vh; margin: 0; background: #f2f1ec; color: #20242a; }}
  form {{ width: min(320px, 90vw); text-align: center; }}
  h1 {{ font-size: 18px; font-family: ui-monospace, monospace; }}
  input, button {{ width: 100%; box-sizing: border-box; font: inherit;
                   padding: 0.6em 0.8em; margin-top: 0.5em; }}
  .error {{ color: #a03030; font-size: 14px; }}
  @media (prefers-color-scheme: dark) {{
    body {{ background: #17191c; color: #d7d9d3; }}
    input {{ background: #1e2125; color: inherit; border: 1px solid #34383d; }}
  }}
</style>
</head>
<body>
<form method="post" action="/{public_id}/{slug}">
  <h1>{slug}</h1>
  <p>This document is protected.</p>
  {error}
  <input type="password" name="password" placeholder="document password" autofocus required>
  <button type="submit">Open document</button>
</form>
</body>
</html>
"#
    );
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .content_type("text/html; charset=utf-8")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(html)
}
