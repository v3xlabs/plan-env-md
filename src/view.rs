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

fn is_public_id_shape(segment: &str) -> bool {
    segment.len() == 10 && segment.bytes().all(|b| b.is_ascii_alphanumeric())
}

struct Doc {
    id: i64,
    owner_id: i64,
    slug: String,
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
    if !is_public_id_shape(&public_id) {
        return not_found();
    }
    if !limiter.0.allow(real_ip.0) {
        return too_many_requests();
    }
    let Some(doc) = fetch_doc(pool.0, &public_id).await else {
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
    let mac = access_mac(&secret.0.0, doc.id, expiry, &password_hash);
    let mut cookie = Cookie::new_with_str(DOC_ACCESS_COOKIE, format!("{expiry}.{mac}"));
    cookie.set_path(format!("/{public_id}/{slug}"));
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(base_url.0.is_https());
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
    let authorized = owner
        || (doc.published
            && doc.password_hash.as_deref().is_some_and(|password_hash| {
                has_valid_access_cookie(req, &secret.0, doc.id, password_hash)
            }));

    if authorized {
        return serve_html(pool, doc.id, revision).await;
    }
    if doc.published {
        return password_form(public_id, slug, false);
    }
    // a private document must be indistinguishable from a missing one
    not_found()
}

async fn fetch_doc(pool: &SqlitePool, public_id: &str) -> Option<Doc> {
    sqlx::query_as!(
        Doc,
        r#"SELECT id as "id!: i64", owner_id as "owner_id!: i64", slug as "slug!: String",
                  published as "published!: bool", password_hash
           FROM documents WHERE public_id = ?"#,
        public_id
    )
    .fetch_optional(pool)
    .await
    .ok()?
}

async fn serve_html(pool: &SqlitePool, document_id: i64, revision: Option<i64>) -> Response {
    let html = match revision {
        Some(revision) => {
            sqlx::query_scalar!(
                r#"SELECT html as "html!: Vec<u8>" FROM revisions
               WHERE document_id = ? AND revision = ?"#,
                document_id,
                revision
            )
            .fetch_optional(pool)
            .await
        }
        None => {
            sqlx::query_scalar!(
                r#"SELECT html as "html!: Vec<u8>" FROM revisions
               WHERE document_id = ? ORDER BY revision DESC LIMIT 1"#,
                document_id
            )
            .fetch_optional(pool)
            .await
        }
    };
    match html {
        Ok(Some(html)) => Response::builder()
            .content_type("text/html; charset=utf-8")
            .header(
                header::CONTENT_SECURITY_POLICY,
                "sandbox allow-scripts allow-popups",
            )
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .header(header::REFERRER_POLICY, "no-referrer")
            .body(html),
        Ok(None) => not_found(),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .finish(),
    }
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
        Some(revision) => format!("/{public_id}/{slug}/rev/{revision}"),
        None => format!("/{public_id}/{slug}"),
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
