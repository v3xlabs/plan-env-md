use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use poem::http::{StatusCode, header};
use poem::web::cookie::{Cookie, SameSite};
use poem::web::{Data, Form, Path, RealIp};
use poem::{Request, Response, handler};
use sha2::Sha256;
use sqlx::SqlitePool;

use crate::config::{AppUrl, DocsUrl, Secret};
use crate::rate_limit::RateLimiter;
use crate::{auth, grant};

const DOC_ACCESS_COOKIE: &str = "doc_access";
const ACCESS_DAYS: u64 = 7;

/// A public id is minted at exactly this shape, so a path that does not open
/// with one names no document and never will. Worth checking before answering:
/// it saves a stranger's typo a redirect and a trip to the app for a grant.
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
pub async fn view_revision(
    req: &Request,
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    app_url: Data<&AppUrl>,
    docs_url: Data<&DocsUrl>,
    blobs: Data<&Option<crate::blobs::Blobs>>,
    Path((public_id, slug, revision)): Path<(String, String, i64)>,
) -> Response {
    serve(
        req,
        pool.0,
        secret.0,
        app_url.0,
        docs_url.0,
        blobs.0.as_ref(),
        &public_id,
        &slug,
        Some(revision),
    )
    .await
}

/// Nothing lives at the root of the documents host: a reader who arrives there
/// typed the name they know, and what they are looking for is the app.
#[handler]
pub fn docs_root(app_url: Data<&AppUrl>) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, app_url.0.0.as_str())
        .finish()
}

/// A document is a directory now, so a relative `<script src="chart.js">`
/// resolves inside it. The old path answers with a 308, which preserves the
/// method, so bookmarks and the unlock POST both still land.
#[handler]
pub fn redirect_to_dir(Path((public_id, slug)): Path<(String, String)>) -> Response {
    if !is_public_id_shape(&public_id) {
        return not_found();
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
    app_url: Data<&AppUrl>,
    docs_url: Data<&DocsUrl>,
    blobs: Data<&Option<crate::blobs::Blobs>>,
    Path((public_id, slug, path)): Path<(String, String, String)>,
) -> Response {
    serve_asset(
        req,
        pool.0,
        secret.0,
        app_url.0,
        docs_url.0,
        blobs.0.as_ref(),
        &public_id,
        &slug,
        None,
        &path,
    )
    .await
}

#[handler]
pub async fn asset_revision(
    req: &Request,
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    app_url: Data<&AppUrl>,
    docs_url: Data<&DocsUrl>,
    blobs: Data<&Option<crate::blobs::Blobs>>,
    Path((public_id, slug, revision, path)): Path<(String, String, i64, String)>,
) -> Response {
    serve_asset(
        req,
        pool.0,
        secret.0,
        app_url.0,
        docs_url.0,
        blobs.0.as_ref(),
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
    app_url: &AppUrl,
    docs_url: &DocsUrl,
    blobs: Option<&crate::blobs::Blobs>,
    public_id: &str,
    slug: &str,
    revision: Option<i64>,
    path: &str,
) -> Response {
    // poem matches the directory URL itself against this wildcard with an empty
    // remainder, so that case is the document rather than a missing asset
    if path.is_empty() {
        return serve(
            req, pool, secret, app_url, docs_url, blobs, public_id, slug, revision,
        )
        .await;
    }
    if !asked_for_by_its_own_document(req, public_id, slug) {
        return not_found();
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
        r#"SELECT f.content as "content: Vec<u8>", f.object_key as "object_key: String",
                  f.content_type as "content_type!: String"
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
    // the row exists, so a body that will not load is a fault on our side. It
    // must not answer 404: that is the reply for a document the caller may not
    // see, and reusing it here would hide a broken asset as a missing one
    let content = match crate::blobs::resolve(blobs, row.content, row.object_key).await {
        Ok(content) => content,
        Err(error) => {
            tracing::warn!(document = doc.id, path, %error, "cannot read a document asset");
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CACHE_CONTROL, "no-store")
                .finish();
        }
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
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        // a stylesheet asks for its own fonts and images, and those requests
        // have to name it for the rule above to recognise them. Same-origin
        // keeps the address out of anything a document reaches off this site
        .header(header::REFERRER_POLICY, "same-origin")
        .body(content)
}

/// Every document shares one origin, so the browser will hand one document's
/// files to another document's scripts if we let it. This is where we do not.
///
/// A browser states what a request is for and what it came from in headers a
/// page cannot set, `Sec-Fetch-Dest` and `Sec-Fetch-Site`, and for anything but
/// a fetch it sets the `Referer` itself. Together they say whether a file is
/// being loaded by the document it belongs to.
///
/// What this does not catch is a `fetch`, whose referrer the calling page may
/// choose from anywhere on its own origin, and which is therefore refused
/// outright; and a document another document opened in a window, which is a
/// page rather than a file and never reaches here. Closing that last one takes
/// an origin per document, not a rule.
fn asked_for_by_its_own_document(req: &Request, public_id: &str, slug: &str) -> bool {
    let Some(dest) = req.header("sec-fetch-dest") else {
        // not a browser. An agent or a shell carries its own credential and has
        // no page behind it to protect
        return true;
    };
    let site = req.header("sec-fetch-site");
    // typed, bookmarked, or otherwise opened by the reader with nothing in
    // between: there is no page to be reading on somebody's behalf
    if site == Some("none") {
        return true;
    }
    if site != Some("same-origin") || dest == "empty" {
        return false;
    }
    let scope = format!("/{public_id}/{slug}/");
    req.header(header::REFERER)
        .and_then(url_path)
        .is_some_and(|path| path.starts_with(&scope))
}

/// The path of an absolute URL, which is all that is left to check once
/// `Sec-Fetch-Site` has established that it names this origin.
fn url_path(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://")?.1;
    let start = after_scheme.find('/')?;
    Some(&after_scheme[start..])
}

#[derive(serde::Deserialize)]
pub struct UnlockForm {
    password: String,
}

#[handler]
pub async fn unlock(
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    docs_url: Data<&DocsUrl>,
    limiter: Data<&RateLimiter>,
    real_ip: RealIp,
    Path((public_id, slug)): Path<(String, String)>,
    Form(form): Form<UnlockForm>,
) -> Response {
    do_unlock(
        pool.0, secret.0, docs_url.0, limiter.0, real_ip, public_id, slug, form,
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
    docs_url: Data<&DocsUrl>,
    limiter: Data<&RateLimiter>,
    real_ip: RealIp,
    Path((public_id, slug, path)): Path<(String, String, String)>,
    Form(form): Form<UnlockForm>,
) -> Response {
    if !path.is_empty() {
        return not_found();
    }
    do_unlock(
        pool.0, secret.0, docs_url.0, limiter.0, real_ip, public_id, slug, form,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn do_unlock(
    pool: &SqlitePool,
    secret: &Secret,
    docs_url: &DocsUrl,
    limiter: &RateLimiter,
    real_ip: RealIp,
    public_id: String,
    slug: String,
    form: UnlockForm,
) -> Response {
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
        return password_form(&public_id, &slug, true, &doc_icon(pool, &doc).await);
    }

    let expiry = unix_now() + (ACCESS_DAYS * 24 * 3600) as i64;
    let mac = access_mac(&secret.0, doc.id, expiry, &password_hash);
    let mut cookie = Cookie::new_with_str(DOC_ACCESS_COOKIE, format!("{expiry}.{mac}"));
    cookie.set_path(format!("/{public_id}/{slug}"));
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(docs_url.0.is_https());
    cookie.set_max_age(std::time::Duration::from_secs(ACCESS_DAYS * 24 * 3600));

    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, format!("/{public_id}/{slug}"))
        .header(header::SET_COOKIE, cookie.to_string())
        .finish()
}

#[allow(clippy::too_many_arguments)]
async fn serve(
    req: &Request,
    pool: &SqlitePool,
    secret: &Secret,
    app_url: &AppUrl,
    docs_url: &DocsUrl,
    blobs: Option<&crate::blobs::Blobs>,
    public_id: &str,
    slug: &str,
    revision: Option<i64>,
) -> Response {
    if !is_public_id_shape(public_id) {
        return not_found();
    }
    let path = req.uri().path().to_string();
    if let Some(response) = grant::redeem(req, secret, docs_url, &path) {
        return response;
    }

    // a document the caller may not read is indistinguishable from one that does
    // not exist, so both answers are the same, and so is the trip to the app that
    // may turn this browser into one that is allowed to read it
    let Some(doc) = fetch_doc(pool, public_id).await else {
        return grant::ask(req, app_url, &path).unwrap_or_else(not_found);
    };
    if doc.slug != slug {
        return redirect_canonical(public_id, &doc.slug, revision);
    }

    let owner = is_owner(req, pool, grant::reader(req, secret), &doc).await;
    if owner || visitor_unlocked(req, secret, &doc) {
        return document_page(
            pool, secret, app_url, docs_url, blobs, &doc, public_id, revision, owner,
        )
        .await;
    }
    if doc.published {
        return password_form(public_id, slug, false, &doc_icon(pool, &doc).await);
    }
    grant::ask(req, app_url, &path).unwrap_or_else(not_found)
}

fn visitor_unlocked(req: &Request, secret: &Secret, doc: &Doc) -> bool {
    doc.published
        && doc.password_hash.as_deref().is_some_and(|password_hash| {
            has_valid_access_cookie(req, &secret.0, doc.id, password_hash)
        })
}

/// Reading as the owner takes either the grant this origin issues to a browser
/// or an API token, which is what a script or an agent carries. Deliberately not
/// the session: that one belongs to the app.
async fn is_owner(req: &Request, pool: &SqlitePool, reader: Option<i64>, doc: &Doc) -> bool {
    if reader == Some(doc.owner_id) {
        return true;
    }
    auth::token_user(pool, req)
        .await
        .is_some_and(|user| user.id == doc.owner_id)
}

/// A document's files answer to the same gate as the document. They are plain
/// same-origin subresources of it, so whatever cookie let the page load is sent
/// with them.
async fn is_authorized(req: &Request, pool: &SqlitePool, secret: &Secret, doc: &Doc) -> bool {
    visitor_unlocked(req, secret, doc) || is_owner(req, pool, grant::reader(req, secret), doc).await
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
/// Fragment links, scrolling, and printing behave as in the bare document. The
/// document runs on this origin, which holds nothing but documents: no session,
/// no API, and no cookie the app reads.
#[allow(clippy::too_many_arguments)]
async fn document_page(
    pool: &SqlitePool,
    secret: &Secret,
    app_url: &AppUrl,
    docs_url: &DocsUrl,
    blobs: Option<&crate::blobs::Blobs>,
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

    let row = sqlx::query!(
        r#"SELECT f.content as "content: Vec<u8>", f.object_key as "object_key: String"
           FROM revision_files f
           JOIN revisions r ON r.id = f.revision_id
           WHERE r.document_id = ? AND r.revision = ? AND f.path = 'index.html'"#,
        doc.id,
        current
    )
    .fetch_optional(pool)
    .await;
    let mut body = match row {
        Ok(Some(row)) => match crate::blobs::resolve(blobs, row.content, row.object_key).await {
            Ok(body) => body,
            Err(error) => {
                tracing::warn!(document = doc.id, %error, "cannot read the document body");
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header(header::CACHE_CONTROL, "no-store")
                    .finish();
            }
        },
        Ok(None) => return not_found(),
        Err(_) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CACHE_CONTROL, "no-store")
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
    // the project's icon, so a reader's browser tab says which project this
    // document belongs to.
    //
    // Into <head>, unlike everything else here. Appending after </html> puts
    // the link in <body> once the parser reparents it, and a favicon link in
    // the body is not reliably honoured: that is why documents carried these
    // links yet showed a blank tab. A document that declared its own icon
    // meant it, so it keeps it.
    if let Some(project) = &doc.project
        && !declares_icon(&body)
    {
        let icon = favicon_fragment(pool, doc.owner_id, project).await;
        insert_into_head(&mut body, icon.as_bytes());
    }

    // the share page publishes and rotates passwords, so it belongs to the app
    // and opens on the app's origin, where the session is
    let share = if is_owner {
        let app = app_url.0.as_str();
        let slug = &doc.slug;
        format!(
            r#"<a id="planenv-share" href="{app}/share/{public_id}/{slug}" target="_blank" rel="noopener" onclick="window.open(this.href, 'planenv-share', 'width=440,height=600'); return false">Share</a>"#
        )
    } else {
        String::new()
    };

    body.extend_from_slice(
        overlay_fragment(
            app_url, docs_url, &share, public_id, doc, current, latest, &revisions, &questions,
        )
        .as_bytes(),
    );
    if !questions.is_empty() {
        body.extend_from_slice(answer_widget_fragment(app_url, secret, doc, &questions).as_bytes());
    }

    Response::builder()
        .content_type("text/html; charset=utf-8")
        // the overlay differs for the owner, so this body belongs to one reader
        .header(header::CACHE_CONTROL, "no-store")
        // no document may frame another, or itself: a frame of a page on this
        // origin is a page the framing script can read
        .header(header::CONTENT_SECURITY_POLICY, "frame-ancestors 'none'")
        // a window this document opens is cut loose from it, so a script cannot
        // open another document and read what came back. Newer browsers only,
        // which is why this is one layer and not the boundary
        .header("cross-origin-opener-policy", "noopener-allow-popups")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        // the document's own files must be able to name it as their referrer;
        // nothing off this origin ever sees the address
        .header(header::REFERRER_POLICY, "same-origin")
        .body(body)
}

/// A compact pill cluster fixed to the top-right corner. Dark on every
/// document so it needs no theme detection, hidden in print, no JavaScript:
/// the revision menu is a details element.
#[allow(clippy::too_many_arguments)]
fn overlay_fragment(
    app_url: &AppUrl,
    docs_url: &DocsUrl,
    share: &str,
    public_id: &str,
    doc: &Doc,
    current: i64,
    latest: i64,
    revisions: &[i64],
    questions: &[crate::api::question::AnsweredQuestion],
) -> String {
    let slug = &doc.slug;
    let title = html_escape(doc.title.as_deref().unwrap_or(slug));

    // the name says which host the reader is on, and the link goes where they
    // would want to go from a document, which is the app
    let app_href = html_escape(app_url.0.as_str());
    let host = docs_url.0.authority();
    let brand = match host.split_once('.') {
        Some((first, rest)) => format!("{}<b>.{}</b>", html_escape(first), html_escape(rest)),
        None => html_escape(host),
    };

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

    // the count and the segments are rendered rather than left for the widget to
    // fill in, so the control agrees with the document list before any script
    // runs, and so it is not a blank pill while the module loads
    let answered = if questions.is_empty() {
        String::new()
    } else {
        let count = questions.iter().filter(|q| q.answer.is_some()).count();
        let total = questions.len();
        let segments: String = questions
            .iter()
            .map(|q| {
                if q.answer.is_some() {
                    r#"<i class="is-on"></i>"#
                } else {
                    "<i></i>"
                }
            })
            .collect();
        // one segment per question says how many there are as well as how far
        // along the reader is, which a single bar cannot
        let done = if count == total {
            " class=\"is-done\""
        } else {
            ""
        };
        format!(
            r#"<div id="planenv-progress"{done} title="{count} of {total} answered"><span id="planenv-answered">{count} of {total}</span><span id="planenv-nav"><button type="button" class="planenv-step" data-planenv-step="-1" aria-label="Previous question">&#8592;</button><button type="button" class="planenv-step" data-planenv-step="1" aria-label="Next question">&#8594;</button></span><b id="planenv-done">?</b><span id="planenv-track">{segments}</span></div>"#
        )
    };

    format!(
        r#"
<div id="planenv-overlay">
<a id="planenv-brand" href="{app_href}" title="{title}">{brand}</a>
<div id="planenv-tools">
{answered}
<details id="planenv-revs"><summary>{summary}</summary><nav>{revision_links}</nav></details>
{share}
</div>
</div>
<style>
#planenv-overlay {{
  position: fixed; top: 0; left: 0; right: 0; z-index: 2147483647;
  display: flex; justify-content: space-between; align-items: flex-start;
  gap: 10px; padding: 10px;
  /* the span between the two ends belongs to the document, not to us */
  pointer-events: none;
  font: 12px/1 system-ui, -apple-system, "Segoe UI", sans-serif;
}}
/* the overlay is injected into somebody else's document, so it cannot borrow
   that document's reset, and every size below depends on the border being
   inside the height */
#planenv-overlay, #planenv-overlay * {{ box-sizing: border-box; }}
/* flex-start, not stretch: a pill that grew would drag Share up with it and
   leave the revision summary behind, since that one sits inside a details */
#planenv-tools {{ display: flex; gap: 6px; align-items: flex-start; }}
#planenv-overlay a, #planenv-overlay summary, #planenv-progress {{
  pointer-events: auto;
  display: flex; align-items: center;
  background: #1c1f22e8; color: #e7e5df;
  border: 1px solid #ffffff26;
  /* one height for every pill, set here and never by what a pill contains, so
     nothing in the cluster moves when a control changes state */
  height: 28px; padding: 0 12px;
  text-decoration: none; cursor: pointer;
  white-space: nowrap;
}}
#planenv-overlay a:focus-visible, #planenv-overlay summary:focus-visible {{
  outline: 2px solid #4cc2a0; outline-offset: 1px;
}}
#planenv-brand {{ font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; font-weight: 600; }}
#planenv-brand b {{ color: #4cc2a0; }}
#planenv-revs {{ position: relative; }}
#planenv-revs summary {{ list-style: none; font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; }}
#planenv-revs summary::-webkit-details-marker {{ display: none; }}
#planenv-revs nav {{
  position: absolute; right: 0; top: calc(100% + 6px);
  display: flex; flex-direction: column; min-width: 130px;
  background: #1c1f22f2; border: 1px solid #ffffff26; padding: 4px;
}}
#planenv-revs nav a {{ background: none; border: 0; padding: 0 10px; }}
#planenv-revs nav a:hover {{ background: #ffffff14; }}
#planenv-revs nav a.viewing {{ color: #4cc2a0; }}
#planenv-share {{ background: #4cc2a0; border-color: transparent; color: #10201b; font-weight: 600; }}

/* The answer control. Its padding matches the pills beside it and its track is
   out of flow, so it is exactly their height in every state, including the
   square it collapses to. */
#planenv-progress {{ position: relative; gap: 10px; padding: 0 10px; }}
#planenv-answered {{ font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; }}
#planenv-nav {{ display: flex; gap: 4px; }}
.planenv-step {{
  width: 16px; height: 16px; display: grid; place-items: center;
  border: 1px solid #ffffff2e; background: none; color: inherit;
  font: 10px/1 system-ui, sans-serif; cursor: pointer; padding: 0;
}}
.planenv-step:hover, .planenv-step:focus-visible {{ background: #4cc2a0; border-color: #4cc2a0; color: #10201b; outline: 0; }}
.planenv-step[disabled] {{ opacity: .3; cursor: default; }}
.planenv-step[disabled]:hover {{ background: none; border-color: #ffffff2e; color: inherit; }}
/* flush to the bottom, edge to edge, resting on the border rather than being it */
#planenv-track {{ position: absolute; left: 0; right: 0; bottom: 0; height: 2px; display: flex; gap: 1px; }}
#planenv-track i {{ flex: 1; background: #ffffff26; }}
#planenv-track i.is-on {{ background: #4cc2a0; }}
#planenv-done {{ display: none; color: #ffffff; font-size: 14px; line-height: 1; }}

/* Answered in full: the control keeps the height and gives up the width, and
   hover or focus hands every part of it back. Reduced motion lands on the same
   square without the step in between, so the square has to read on its own. */
/* the square is the pill height on both sides, and only the width animates,
   so the row it sits in never reflows */
#planenv-progress.is-done {{
  width: 28px; padding: 0; gap: 0; justify-content: center;
  background: #4cc2a0; border-color: #4cc2a0;
  transition: width 160ms, background-color 160ms;
}}
#planenv-progress.is-done > :not(#planenv-done) {{ display: none; }}
#planenv-progress.is-done #planenv-done {{ display: block; }}
#planenv-progress.is-done:hover, #planenv-progress.is-done:focus-within {{
  width: auto; padding: 0 10px; gap: 10px;
  background: #1c1f22e8; border-color: #ffffff26;
}}
#planenv-progress.is-done:hover > :not(#planenv-done), #planenv-progress.is-done:focus-within > :not(#planenv-done) {{ display: flex; }}
#planenv-progress.is-done:hover #planenv-answered, #planenv-progress.is-done:focus-within #planenv-answered {{ display: block; }}
#planenv-progress.is-done:hover #planenv-done, #planenv-progress.is-done:focus-within #planenv-done {{ display: none; }}
@media (prefers-reduced-motion: reduce) {{
  #planenv-progress.is-done {{ transition: none; }}
}}
@media print {{ #planenv-overlay {{ display: none; }} }}
</style>
"#
    )
}

/// The project's icon links for a document, or nothing when it has no project
/// or that project has no icon.
async fn doc_icon(pool: &SqlitePool, doc: &Doc) -> String {
    match &doc.project {
        Some(project) => favicon_fragment(pool, doc.owner_id, project).await,
        None => String::new(),
    }
}

/// Puts `fragment` just after the document's opening `<head>` tag.
///
/// A byte scan rather than a parse, for the same reason as `declares_icon`.
/// A document with no `<head>` gets the fragment at the very front, where the
/// parser will build one around it, which is still inside the head rather than
/// stranded in the body.
fn insert_into_head(body: &mut Vec<u8>, fragment: &[u8]) {
    let at = find_head_open(body).unwrap_or(0);
    body.splice(at..at, fragment.iter().copied());
}

/// The byte just past `<head ...>`, if the document opens one.
fn find_head_open(body: &[u8]) -> Option<usize> {
    let lowered = body.to_ascii_lowercase();
    let at = lowered.windows(5).position(|window| window == b"<head")?;
    // `<head>` or `<head lang=...>`, but not `<header>`
    let after = body.get(at + 5)?;
    if !matches!(after, b'>' | b' ' | b'\t' | b'\r' | b'\n') {
        return None;
    }
    let close = lowered[at..].iter().position(|byte| *byte == b'>')?;
    Some(at + close + 1)
}

/// Whether the document already asks for a tab icon of its own.
///
/// A scan of the bytes rather than a parse, because the alternative is parsing
/// agent HTML to answer one question. Both ways of being wrong are cheap: a
/// missed declaration appends the project icon over the document's own, and a
/// spurious one leaves the tab on the browser default.
fn declares_icon(body: &[u8]) -> bool {
    let lowered = String::from_utf8_lossy(body).to_ascii_lowercase();
    lowered
        .match_indices("rel=")
        .any(|(at, _)| lowered[at..].split('"').nth(1).is_some_and(is_icon_rel))
}

/// `rel` is a space separated set, and the icon keywords may sit anywhere in
/// it, so `rel="shortcut icon"` and `rel="apple-touch-icon"` both count.
fn is_icon_rel(rel: &str) -> bool {
    rel.split_whitespace()
        .any(|word| word == "icon" || word == "shortcut" || word.ends_with("-icon"))
}

/// The project favicon as a data URI, so it works for a link and password
/// visitor who cannot reach the owner-only favicon endpoint. Both schemes are
/// emitted with a media query; browsers pick the one matching the tab theme.
async fn favicon_fragment(pool: &SqlitePool, owner_id: i64, project: &str) -> String {
    use crate::api::projects::{Scheme, load_favicon};

    let mut links = String::new();
    // dark first, so a browser that ignores `media` on an icon link takes the
    // last one and lands on the light-scheme icon, which suits the light tab
    // strip most of them default to
    for (scheme, media) in [
        (Scheme::Dark, "(prefers-color-scheme: dark)"),
        (Scheme::Light, "(prefers-color-scheme: light)"),
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
/// widget write answers from the docs origin, where the session cookie is never
/// sent. The API it writes to lives on the app origin, so the widget is handed
/// that address rather than resolving one relative to the document.
///
/// The stylesheet is inlined rather than linked, like the overlay's is. A linked
/// one is a second request that can fail on its own, and a widget whose script
/// arrived without its styles renders as unstyled controls in the document.
fn answer_widget_fragment(
    app_url: &AppUrl,
    secret: &Secret,
    doc: &Doc,
    questions: &[crate::api::question::AnsweredQuestion],
) -> String {
    let key = crate::answer_key::mint(&secret.0, doc.id);
    let slug = &doc.slug;
    let app = app_url.0.as_str();
    // the HTML parser ends a script block at the first "</script", wherever it
    // appears, so a question containing one would break out of the JSON island;
    // < is still the same string once JSON.parse runs
    let data = serde_json::to_string(questions)
        .unwrap_or_else(|_| "[]".to_string())
        .replace('<', "\\u003c");

    format!(
        r#"
<style>{css}</style>
<script type="application/json" id="planenv-questions">{data}</script>
<script src="/_planenv/answer.js" data-planenv-key="{key}" data-planenv-slug="{slug}" data-planenv-api="{app}"></script>
"#,
        css = include_str!("answer.css"),
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
    blobs: Data<&Option<crate::blobs::Blobs>>,
    Path(revision_id): Path<i64>,
) -> Response {
    if !is_loopback(req) {
        return not_found();
    }
    serve_revision_file(pool.0, blobs.0.as_ref(), revision_id, "index.html").await
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
    blobs: Data<&Option<crate::blobs::Blobs>>,
    Path((revision_id, path)): Path<(i64, String)>,
) -> Response {
    if !is_loopback(req) {
        return not_found();
    }
    let path = if path.is_empty() { "index.html" } else { &path };
    serve_revision_file(pool.0, blobs.0.as_ref(), revision_id, path).await
}

fn is_loopback(req: &Request) -> bool {
    req.remote_addr()
        .as_socket_addr()
        .is_some_and(|addr| addr.ip().is_loopback())
}

async fn serve_revision_file(
    pool: &SqlitePool,
    blobs: Option<&crate::blobs::Blobs>,
    revision_id: i64,
    path: &str,
) -> Response {
    let row = sqlx::query!(
        r#"SELECT content as "content: Vec<u8>", object_key as "object_key: String",
                  content_type as "content_type!: String"
           FROM revision_files WHERE revision_id = ? AND path = ?"#,
        revision_id,
        path
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    match row {
        // a failure here is what silently renders a preview without its assets,
        // so it is logged rather than folded into the not found case
        Some(row) => match crate::blobs::resolve(blobs, row.content, row.object_key).await {
            Ok(content) => Response::builder()
                .content_type(row.content_type)
                .header(header::CONTENT_SECURITY_POLICY, "sandbox allow-scripts")
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                .body(content),
            Err(error) => {
                tracing::warn!(revision_id, path, %error, "cannot read a revision file");
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .finish()
            }
        },
        None => not_found(),
    }
}

/// The Share popout: a small owner-only page. Publishing, rotating and
/// unpublishing call the session-only API, so this page lives on the app origin
/// where the session cookie is, and not with the document it is about.
#[handler]
pub async fn share_page(
    req: &Request,
    pool: Data<&SqlitePool>,
    docs_url: Data<&DocsUrl>,
    Path((public_id, slug)): Path<(String, String)>,
) -> Response {
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
    let url = format!("{}/{}/{}", docs_url.0.0.as_str(), public_id, doc.slug);
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
  <div class="url">
    <input id="password" type="text" required minlength="1" autocomplete="off" spellcheck="false">
    <button id="generate" type="button" class="quiet">Generate</button>
  </div>
  <div class="actions">
    <button type="submit">{publish_label}</button>
    <button id="one-step" type="button" class="quiet">Generate and copy link</button>
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
  const field = document.querySelector("#password");

  // the same alphabet and length as the dashboard's share dialog, so a
  // password does not tell you which surface produced it. No look-alike
  // characters, since these get read aloud and retyped
  const ALPHABET = "abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  const LENGTH = 20;
  const generate = () => {{
    // bytes at or above the last whole multiple of the alphabet would make the
    // first few characters likelier than the rest, so they are drawn again
    const ceiling = 256 - (256 % ALPHABET.length);
    const out = [];
    while (out.length < LENGTH) {{
      for (const byte of crypto.getRandomValues(new Uint8Array(LENGTH))) {{
        if (byte < ceiling && out.length < LENGTH) out.push(ALPHABET[byte % ALPHABET.length]);
      }}
    }}
    return out.join("");
  }};

  // the gate reads the password out of the fragment, which never reaches the
  // server and so stays out of the request log
  const linkWithPassword = (password) =>
    document.querySelector("#url").value + "#k=" + encodeURIComponent(password);

  const publish = (password) =>
    fetch("/api/docs/{slug}/publish", {{
      method: "POST",
      headers: {{ "Content-Type": "application/json" }},
      body: JSON.stringify({{ password: password }}),
    }});

  document.querySelector("#copy").addEventListener("click", () => {{
    navigator.clipboard.writeText(document.querySelector("#url").value)
      .then(() => say("Link copied, without the password.", false));
  }});

  document.querySelector("#generate").addEventListener("click", () => {{
    field.value = generate();
    field.focus();
    say("Generated. Publish to put it in force.", false);
  }});

  // the page renders its state on the server, so a publish reloads rather than
  // patching the state line, the button label and the unpublish button by hand.
  // The fragment survives the reload and is what reports the copy afterwards.
  const publishThenCopy = (password) =>
    publish(password).then((response) => {{
      if (!response.ok) return say("Publishing failed (status " + response.status + ").", true);
      return navigator.clipboard.writeText(linkWithPassword(password)).then(() => {{
        location.hash = "copied";
        location.reload();
      }});
    }});

  document.querySelector("#one-step").addEventListener("click", () => {{
    const password = generate();
    field.value = password;
    publishThenCopy(password);
  }});

  document.querySelector("#publish-form").addEventListener("submit", (event) => {{
    event.preventDefault();
    publishThenCopy(field.value);
  }});

  if (location.hash === "#copied") {{
    history.replaceState(null, "", location.pathname);
    say("Link with password copied. Send it as one piece.", false);
  }}

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

pub fn hex_encode(bytes: &[u8]) -> String {
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

pub fn unix_now() -> i64 {
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

/// A document and its assets answer 404 to anyone who may not read them, so
/// this reply depends on who is asking and must never be stored by a shared
/// cache. Without `no-store` a CDN caches it against the URL: one request from
/// a reader without credentials then serves that 404 to everyone, and a
/// document renders with its stylesheets missing until the entry expires.
fn not_found() -> Response {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .content_type("text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body("not found")
}

fn too_many_requests() -> Response {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .content_type("text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
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
/// The gate, which also accepts the password from the URL fragment so a share
/// link can carry it.
///
/// The fragment and not a query parameter: a fragment is never sent to the
/// server, so the password stays out of the access log and out of any
/// `Referer` a document's own subresources might send. It is cleared from the
/// address bar before the form is submitted, so it does not linger there or in
/// the back stack. With scripting off the reader simply types the password,
/// which is the behaviour this page had before.
///
/// `icon` carries the project's tab icon, already rendered as link elements.
/// This page is where a stranger following a shared link lands, so it is the
/// one place the icon says whose document they are about to open.
fn password_form(public_id: &str, slug: &str, wrong_password: bool, icon: &str) -> Response {
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
{icon}
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
<script>
(function () {{
  var key = new URLSearchParams(location.hash.slice(1)).get('k');
  if (!key) return;
  var form = document.forms[0];
  form.password.value = key;
  // drop it from the address bar before navigating, so a shared screen or a
  // glance at the history does not hand the password over
  history.replaceState(null, '', location.pathname + location.search);
  form.submit();
}})();
</script>
</body>
</html>
"#
    );
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .content_type("text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(html)
}
