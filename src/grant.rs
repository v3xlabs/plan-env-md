//! How a reader proves, on the docs origin, which account they are.
//!
//! The session lives on the app origin and never leaves it: a document runs its
//! author's scripts, and a script that can make the browser send the session is
//! a script that can act as the reader. So the docs origin gets a credential of
//! its own, minted by the app, that says one thing and grants one thing: this
//! browser reads as user N, and may read what user N owns. Nothing about it
//! writes, and nothing about it works on the app origin.
//!
//! The exchange is a redirect. The docs origin sends a reader with no cookie to
//! the app, the app reads its own session, and it sends the reader back with a
//! signed grant in the URL, which the docs origin turns into a cookie and drops
//! from the address bar. A reader with no session comes back as nobody, which
//! is what keeps a stranger from bouncing between the two origins forever.

use hmac::{Hmac, Mac};
use poem::http::{StatusCode, header};
use poem::web::cookie::{Cookie, SameSite};
use poem::web::{Data, Query};
use poem::{Request, Response, handler};
use sha2::Sha256;
use sqlx::SqlitePool;

use crate::auth;
use crate::config::{AppUrl, DocsUrl, Secret};
use crate::view::{hex_encode, unix_now};

const READER_COOKIE: &str = "doc_reader";
/// A reader keeps a grant about as long as a working week, so opening a
/// document does not bounce through the app every day.
const READER_DAYS: i64 = 7;
/// Nobody's grant is short. It exists only to stop the redirect repeating, and
/// an owner who signs in should not have to wait out a stale one.
const ANONYMOUS_SECONDS: i64 = 60;
/// The grant travels in a URL, so it lives just long enough to be redeemed.
const GRANT_SECONDS: i64 = 30;
const ANONYMOUS: i64 = 0;

/// The two are separate secrets in effect: a grant cannot be replayed as a
/// reader cookie, and a reader cookie cannot be handed back as a grant.
const GRANT: &str = "grant";
const READER: &str = "reader";

#[derive(serde::Deserialize)]
pub struct Next {
    next: String,
}

/// The app's half: read the session, hand back a grant for the docs origin.
///
/// `next` is a path rather than a URL, and the docs origin is prepended here,
/// so this cannot be aimed at somebody else's site.
#[handler]
pub async fn issue(
    req: &Request,
    pool: Data<&SqlitePool>,
    secret: Data<&Secret>,
    docs_url: Data<&DocsUrl>,
    Query(Next { next }): Query<Next>,
) -> Response {
    if !is_document_path(&next) {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .content_type("text/plain; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .body("next must be a document path");
    }

    let user = auth::user_from_request(pool.0, req).await;
    let grant = sign(
        &secret.0.0,
        GRANT,
        user.map_or(ANONYMOUS, |user| user.id),
        GRANT_SECONDS,
    );

    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(
            header::LOCATION,
            format!("{}{next}?g={grant}", docs_url.0.0.as_str()),
        )
        .header(header::CACHE_CONTROL, "no-store")
        .finish()
}

/// A path this service would serve a document at. Rejects anything that would
/// leave the origin, including the protocol-relative `//elsewhere.example`.
fn is_document_path(next: &str) -> bool {
    next.starts_with('/')
        && !next.starts_with("//")
        && next
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
}

/// The docs origin's half: turn a grant in the URL into a reader cookie, then
/// send the reader to the address they asked for, so the grant stays out of the
/// address bar, the history and any bookmark.
pub fn redeem(req: &Request, secret: &Secret, docs_url: &DocsUrl, path: &str) -> Option<Response> {
    let grant = query_value(req.uri().query()?, "g")?;
    let user_id = verify(&secret.0, GRANT, &grant)?;
    let seconds = if user_id == ANONYMOUS {
        ANONYMOUS_SECONDS
    } else {
        READER_DAYS * 24 * 3600
    };

    let mut cookie = Cookie::new_with_str(READER_COOKIE, sign(&secret.0, READER, user_id, seconds));
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(docs_url.0.is_https());
    cookie.set_max_age(std::time::Duration::from_secs(seconds as u64));

    Some(
        Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header(header::LOCATION, path)
            .header(header::SET_COOKIE, cookie.to_string())
            .header(header::CACHE_CONTROL, "no-store")
            .finish(),
    )
}

/// Send a reader who holds no cookie to the app to fetch a grant. Only for a
/// navigation: a subresource is asked for after the page it belongs to was
/// served, so it already carries the cookie, and a client that is not a browser
/// gets the plain reply rather than a redirect it cannot use.
pub fn ask(req: &Request, app_url: &AppUrl, path: &str) -> Option<Response> {
    if req.cookie().get(READER_COOKIE).is_some() {
        return None;
    }
    if req.header("sec-fetch-dest") != Some("document") {
        return None;
    }

    Some(
        Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header(
                header::LOCATION,
                format!("{}/grant?next={path}", app_url.0.as_str()),
            )
            .header(header::CACHE_CONTROL, "no-store")
            .finish(),
    )
}

/// Which account this browser reads as on the docs origin, if any.
pub fn reader(req: &Request, secret: &Secret) -> Option<i64> {
    let cookie = req.cookie().get(READER_COOKIE)?;
    let user_id = verify(&secret.0, READER, cookie.value_str())?;
    (user_id != ANONYMOUS).then_some(user_id)
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.to_string())
}

fn sign(secret: &str, purpose: &str, user_id: i64, seconds: i64) -> String {
    let expiry = unix_now() + seconds;
    format!(
        "{user_id}.{expiry}.{}",
        mac(secret, purpose, user_id, expiry)
    )
}

/// Both halves carry their own expiry inside the signature, so neither can be
/// stretched by a client that simply keeps sending it.
fn verify(secret: &str, purpose: &str, token: &str) -> Option<i64> {
    let (user_id, rest) = token.split_once('.')?;
    let (expiry, given) = rest.split_once('.')?;
    let user_id = user_id.parse::<i64>().ok()?;
    let expiry = expiry.parse::<i64>().ok()?;
    if expiry <= unix_now() || mac(secret, purpose, user_id, expiry) != given {
        return None;
    }
    Some(user_id)
}

fn mac(secret: &str, purpose: &str, user_id: i64, expiry: i64) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(format!("{purpose}.{user_id}.{expiry}").as_bytes());
    hex_encode(&mac.finalize().into_bytes())
}
