use poem::http::{Method, StatusCode, header};
use poem::{Endpoint, Request, Response};
use serde_json::{Value, json};
use sqlx::sqlite::SqlitePoolOptions;

/// No bucket, so every body stays inline. This is the shape almost every test
/// wants, and it needs no credentials and no network.
async fn test_app() -> impl Endpoint {
    let (app, _, _) = test_app_with_blobs(None).await;
    app
}

/// Returns the pool and the store alongside the app, so a test can assert on
/// where a body actually landed rather than only on what the API replies.
async fn test_app_with_blobs(
    blobs: Option<crate::blobs::Blobs>,
) -> (impl Endpoint, sqlx::SqlitePool, Option<crate::blobs::Blobs>) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("memory db");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    let app = crate::app(
        pool.clone(),
        crate::config::BaseUrl("http://test.local".to_string()),
        crate::config::Secret("test-secret".to_string()),
        blobs.clone(),
    );
    (app, pool, blobs)
}

struct Call<'a> {
    cookie: Option<&'a str>,
    bearer: Option<&'a str>,
}

const ANON: Call = Call {
    cookie: None,
    bearer: None,
};

fn with_cookie(cookie: &str) -> Call<'_> {
    Call {
        cookie: Some(cookie),
        bearer: None,
    }
}

fn with_bearer(token: &str) -> Call<'_> {
    Call {
        cookie: None,
        bearer: Some(token),
    }
}

async fn call(
    app: &impl Endpoint,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: Call<'_>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(path.parse().unwrap());
    if let Some(cookie) = auth.cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    if let Some(token) = auth.bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(value) => builder
            .content_type("application/json")
            .body(value.to_string()),
        None => builder.finish(),
    };
    app.get_response(request).await
}

fn session_cookie_of(response: &Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn json_body(response: Response) -> Value {
    let text = response.into_body().into_string().await.unwrap();
    serde_json::from_str(&text).unwrap()
}

async fn register(app: &impl Endpoint, username: &str, code: Option<&str>) -> Response {
    let mut body = json!({ "username": username, "password": "password123" });
    if let Some(code) = code {
        body["invite_code"] = json!(code);
    }
    call(app, Method::POST, "/api/auth/register", Some(body), ANON).await
}

#[tokio::test]
async fn first_user_bootstraps_then_invites_gate_registration() {
    let app = test_app().await;

    let response = register(&app, "admin", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let admin_cookie = session_cookie_of(&response);
    let body = json_body(response).await;
    assert_eq!(body["is_admin"], json!(true));

    let response = register(&app, "second", None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = call(
        &app,
        Method::POST,
        "/api/invites",
        None,
        with_cookie(&admin_cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let code = json_body(response).await["code"]
        .as_str()
        .unwrap()
        .to_string();

    let response = register(&app, "second", Some(&code)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["is_admin"], json!(false));

    let response = register(&app, "third", Some(&code)).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = register(&app, "second", None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn duplicate_username_conflicts_only_with_valid_invite() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let admin_cookie = session_cookie_of(&response);

    // without a valid invite the username is never checked, so no enumeration
    let response = register(&app, "admin", None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = call(
        &app,
        Method::POST,
        "/api/invites",
        None,
        with_cookie(&admin_cookie),
    )
    .await;
    let code = json_body(response).await["code"]
        .as_str()
        .unwrap()
        .to_string();

    let response = register(&app, "admin", Some(&code)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);

    // the failed attempt must not have burned the invite
    let response = register(&app, "member", Some(&code)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn login_and_session_lifecycle() {
    let app = test_app().await;
    register(&app, "admin", None).await;

    let response = call(
        &app,
        Method::POST,
        "/api/auth/login",
        Some(json!({ "username": "admin", "password": "wrong-password" })),
        ANON,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = call(
        &app,
        Method::POST,
        "/api/auth/login",
        Some(json!({ "username": "admin", "password": "password123" })),
        ANON,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = session_cookie_of(&response);

    let response = call(
        &app,
        Method::GET,
        "/api/auth/me",
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["username"], json!("admin"));

    let response = call(&app, Method::GET, "/api/auth/me", None, ANON).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = call(
        &app,
        Method::POST,
        "/api/auth/logout",
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = call(
        &app,
        Method::GET,
        "/api/auth/me",
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_tokens_authenticate_but_cannot_manage() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let cookie = session_cookie_of(&response);

    let response = call(
        &app,
        Method::POST,
        "/api/tokens",
        Some(json!({ "name": "agent" })),
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let token = body["token"].as_str().unwrap().to_string();
    let token_id = body["id"].as_i64().unwrap();
    assert!(token.starts_with("pem_"));

    let response = call(&app, Method::GET, "/api/auth/me", None, with_bearer(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);

    // a PAT must not mint more PATs
    let response = call(
        &app,
        Method::POST,
        "/api/tokens",
        Some(json!({ "name": "sneaky" })),
        with_bearer(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = call(&app, Method::GET, "/api/tokens", None, with_cookie(&cookie)).await;
    let body = json_body(response).await;
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert!(body[0].get("token").is_none());

    let response = call(
        &app,
        Method::DELETE,
        &format!("/api/tokens/{token_id}"),
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = call(&app, Method::GET, "/api/auth/me", None, with_bearer(&token)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invites_are_admin_only() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let admin_cookie = session_cookie_of(&response);

    let response = call(
        &app,
        Method::POST,
        "/api/invites",
        None,
        with_cookie(&admin_cookie),
    )
    .await;
    let code = json_body(response).await["code"]
        .as_str()
        .unwrap()
        .to_string();

    let response = register(&app, "member", Some(&code)).await;
    let member_cookie = session_cookie_of(&response);

    let response = call(
        &app,
        Method::POST,
        "/api/invites",
        None,
        with_cookie(&member_cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = call(
        &app,
        Method::GET,
        "/api/invites",
        None,
        with_cookie(&admin_cookie),
    )
    .await;
    let body = json_body(response).await;
    assert_eq!(body[0]["used_by"], json!("member"));
}

async fn push(app: &impl Endpoint, token: &str, slug: &str, html: &str) -> Response {
    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/docs/{slug}").parse().unwrap())
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .content_type("text/html")
        .body(html.to_string());
    app.get_response(request).await
}

/// Push through the multipart path, which is the only one carrying metadata.
async fn push_with_meta(
    app: &impl Endpoint,
    token: &str,
    slug: &str,
    html: &str,
    meta: Value,
) -> Response {
    const BOUNDARY: &str = "planenvtestboundary";
    let body = format!(
        "--{BOUNDARY}\r\n\
         Content-Disposition: form-data; name=\"meta\"\r\n\
         Content-Type: application/json\r\n\r\n\
         {meta}\r\n\
         --{BOUNDARY}\r\n\
         Content-Disposition: form-data; name=\"index.html\"\r\n\
         Content-Type: text/html\r\n\r\n\
         {html}\r\n\
         --{BOUNDARY}--\r\n"
    );
    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/docs/{slug}").parse().unwrap())
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .content_type(&format!("multipart/form-data; boundary={BOUNDARY}"))
        .body(body);
    app.get_response(request).await
}

fn one_question() -> Value {
    json!({ "questions": [{
        "key": "P4",
        "anchor": "P4",
        "prompt": "Accept the trailing slash URL change?",
        "options": [
            { "value": "accept", "label": "Accept" },
            { "value": "defer", "label": "Defer" }
        ]
    }]})
}

/// The key the viewer hands the widget, read back out of the served document.
async fn scoped_key(app: &impl Endpoint, cookie: &str, id: &str, slug: &str) -> Option<String> {
    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/{id}/{slug}/").parse().unwrap())
        .header(header::COOKIE, cookie)
        .finish();
    let html = app
        .get_response(request)
        .await
        .into_body()
        .into_string()
        .await
        .unwrap();
    let start = html.find("data-planenv-key=\"")? + "data-planenv-key=\"".len();
    let rest = &html[start..];
    Some(rest[..rest.find('"')?].to_string())
}

async fn answer(
    app: &impl Endpoint,
    slug: &str,
    key: &str,
    body: Value,
    auth: Call<'_>,
) -> Response {
    call(
        app,
        Method::PUT,
        &format!("/api/docs/{slug}/answers/{key}"),
        Some(body),
        auth,
    )
    .await
}

async fn agent_token(app: &impl Endpoint, cookie: &str) -> String {
    let response = call(
        app,
        Method::POST,
        "/api/tokens",
        Some(json!({ "name": "agent" })),
        with_cookie(cookie),
    )
    .await;
    json_body(response).await["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn push_creates_then_appends_revisions_at_same_url() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let cookie = session_cookie_of(&response);
    let token = agent_token(&app, &cookie).await;

    let response = push(&app, &token, "my-plan", "<h1>rev one</h1>").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 10);
    assert_eq!(body["revision"], json!(1));
    assert_eq!(
        body["url"],
        json!(format!("http://test.local/{id}/my-plan"))
    );

    let response = push(&app, &token, "my-plan", "<h1>rev two</h1>").await;
    let body = json_body(response).await;
    assert_eq!(body["id"], json!(id));
    assert_eq!(body["revision"], json!(2));

    let response = call(
        &app,
        Method::GET,
        "/api/docs/my-plan/raw",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap(),
        "sandbox allow-scripts allow-popups"
    );
    let text = response.into_body().into_string().await.unwrap();
    assert_eq!(text, "<h1>rev two</h1>");

    let response = call(
        &app,
        Method::GET,
        "/api/docs/my-plan/revisions/1/raw",
        None,
        with_bearer(&token),
    )
    .await;
    let text = response.into_body().into_string().await.unwrap();
    assert_eq!(text, "<h1>rev one</h1>");

    let response = call(
        &app,
        Method::GET,
        "/api/docs/my-plan",
        None,
        with_bearer(&token),
    )
    .await;
    let body = json_body(response).await;
    assert_eq!(body["revisions"].as_array().unwrap().len(), 2);

    let response = call(&app, Method::GET, "/api/docs", None, with_bearer(&token)).await;
    let body = json_body(response).await;
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["revision_count"], json!(2));
    assert_eq!(body[0]["published"], json!(false));
}

#[tokio::test]
async fn push_validates_slug_and_size() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let cookie = session_cookie_of(&response);
    let token = agent_token(&app, &cookie).await;

    let response = push(&app, &token, "Bad_Slug", "<p>hi</p>").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let exactly_max = "a".repeat(crate::api::upload::MAX_ENTRY_BYTES);
    let response = push(&app, &token, "big", &exactly_max).await;
    assert_eq!(response.status(), StatusCode::OK);

    let too_big = "a".repeat(crate::api::upload::MAX_ENTRY_BYTES + 1);
    let response = push(&app, &token, "too-big", &too_big).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let response = push(&app, &token, "no-doc", "").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn same_slug_is_isolated_per_account() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let admin_cookie = session_cookie_of(&response);
    let admin_token = agent_token(&app, &admin_cookie).await;

    let response = call(
        &app,
        Method::POST,
        "/api/invites",
        None,
        with_cookie(&admin_cookie),
    )
    .await;
    let code = json_body(response).await["code"]
        .as_str()
        .unwrap()
        .to_string();
    let response = register(&app, "member", Some(&code)).await;
    let member_cookie = session_cookie_of(&response);
    let member_token = agent_token(&app, &member_cookie).await;

    let response = push(&app, &admin_token, "plan", "<p>admin doc</p>").await;
    let admin_id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let response = push(&app, &member_token, "plan", "<p>member doc</p>").await;
    let member_id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(admin_id, member_id);

    let response = call(
        &app,
        Method::GET,
        "/api/docs/plan/raw",
        None,
        with_bearer(&member_token),
    )
    .await;
    let text = response.into_body().into_string().await.unwrap();
    assert_eq!(text, "<p>member doc</p>");
}

#[tokio::test]
async fn push_title_query_parameter_sticks() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let cookie = session_cookie_of(&response);
    let token = agent_token(&app, &cookie).await;

    let request = Request::builder()
        .method(Method::PUT)
        .uri("/api/docs/titled?title=My%20Plan".parse().unwrap())
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .content_type("text/html")
        .body("<p>doc</p>".to_string());
    let response = app.get_response(request).await;
    assert_eq!(response.status(), StatusCode::OK);

    // a later push without a title keeps the existing one
    push(&app, &token, "titled", "<p>doc two</p>").await;
    let response = call(
        &app,
        Method::GET,
        "/api/docs/titled",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(json_body(response).await["title"], json!("My Plan"));
}

async fn unlock(app: &impl Endpoint, path: &str, password: &str) -> Response {
    let request = Request::builder()
        .method(Method::POST)
        .uri(path.parse().unwrap())
        .content_type("application/x-www-form-urlencoded")
        .body(format!("password={password}"));
    app.get_response(request).await
}

fn access_cookie_of(response: &Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn public_view_password_gate_lifecycle() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let cookie = session_cookie_of(&response);
    let token = agent_token(&app, &cookie).await;

    let response = push(&app, &token, "plan", "<h1>rev one</h1>").await;
    let id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    push(&app, &token, "plan", "<h1>rev two</h1>").await;
    let doc_path = format!("/{id}/plan/");

    // private: visitor sees 404; the owner sees the document itself with the
    // overlay (revision menu, Share) appended
    let response = call(&app, Method::GET, &doc_path, None, ANON).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = call(&app, Method::GET, &doc_path, None, with_cookie(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = response.into_body().into_string().await.unwrap();
    assert!(text.starts_with("<h1>rev two</h1>"));
    assert!(text.contains("planenv-overlay"));
    assert!(text.contains("rev 2 (current)"));
    assert!(text.contains(">Share</a>"));

    // publish behind a password
    let response = call(
        &app,
        Method::POST,
        "/api/docs/plan/publish",
        Some(json!({ "password": "letmein" })),
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["url"],
        // the bare URL stays the emitted permalink; it 308s to the directory form
        json!(format!("http://test.local/{id}/plan"))
    );

    // a PAT must not be able to publish
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/docs/plan/publish".parse().unwrap())
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .content_type("application/json")
        .body(json!({ "password": "sneaky" }).to_string());
    let response = app.get_response(request).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // visitor now gets the password form
    let response = call(&app, Method::GET, &doc_path, None, ANON).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let text = response.into_body().into_string().await.unwrap();
    assert!(text.contains("<form"));

    // wrong password: form again, no cookie
    let response = unlock(&app, &doc_path, "nope").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get(header::SET_COOKIE).is_none());

    // right password: redirect with access cookie; the document serves with
    // the sandbox intact and the overlay carries no Share button for visitors
    let response = unlock(&app, &doc_path, "letmein").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let access = access_cookie_of(&response);
    let response = call(&app, Method::GET, &doc_path, None, with_cookie(&access)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap(),
        "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox"
    );
    let text = response.into_body().into_string().await.unwrap();
    assert!(text.starts_with("<h1>rev two</h1>"));
    assert!(text.contains("planenv-overlay"));
    assert!(!text.contains(">Share</a>"));

    // the same cookie opens pinned revisions
    let response = call(
        &app,
        Method::GET,
        &format!("{doc_path}rev/1/"),
        None,
        with_cookie(&access),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = response.into_body().into_string().await.unwrap();
    assert!(text.starts_with("<h1>rev one</h1>"));
    assert!(text.contains("rev 1 of 2"));

    // rotating the password kills outstanding cookies
    call(
        &app,
        Method::POST,
        "/api/docs/plan/publish",
        Some(json!({ "password": "new-password" })),
        with_cookie(&cookie),
    )
    .await;
    let response = call(&app, Method::GET, &doc_path, None, with_cookie(&access)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // fresh unlock with the new password, then unpublish hides the document
    let response = unlock(&app, &doc_path, "new-password").await;
    let access = access_cookie_of(&response);
    let response = call(
        &app,
        Method::POST,
        "/api/docs/plan/unpublish",
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = call(&app, Method::GET, &doc_path, None, with_cookie(&access)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn stale_slug_redirects_to_canonical_url() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let cookie = session_cookie_of(&response);
    let token = agent_token(&app, &cookie).await;

    let response = push(&app, &token, "real-slug", "<p>doc</p>").await;
    let id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let response = call(
        &app,
        Method::GET,
        &format!("/{id}/wrong-slug"),
        None,
        with_cookie(&cookie),
    )
    .await;
    // the bare URL redirects to its directory form without a database hit, and
    // the canonical slug redirect happens there
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/{id}/wrong-slug/")
    );
    let response = call(
        &app,
        Method::GET,
        &format!("/{id}/wrong-slug/"),
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/{id}/real-slug/")
    );

    let response = call(
        &app,
        Method::GET,
        &format!("/{id}/wrong-slug/rev/1/"),
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/{id}/real-slug/rev/1/")
    );

    // an unknown public id is a plain 404
    let response = call(
        &app,
        Method::GET,
        "/AAAAAAAAAA/whatever/",
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn docs_page_and_spa_fallback_routing() {
    let app = test_app().await;

    // scalar api reference with the spec embedded inline
    let response = call(&app, Method::GET, "/docs", None, ANON).await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = response.into_body().into_string().await.unwrap();
    assert!(text.contains("api-reference"));
    assert!(text.contains("plan-env-md"));

    // two-segment paths that cannot be a public_id serve the SPA shell,
    // exactly like a one-segment client route
    let spa_route = call(&app, Method::GET, "/login", None, ANON).await;
    let client_detail = call(&app, Method::GET, "/documents/anything", None, ANON).await;
    assert_eq!(spa_route.status(), client_detail.status());

    // a doc-shaped path redirects to its directory form without touching the
    // database, so the redirect itself signals nothing; the miss is a hard 404
    let response = call(&app, Method::GET, "/AAAAAAAAAA/nope", None, ANON).await;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    let response = call(&app, Method::GET, "/AAAAAAAAAA/nope/", None, ANON).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn embedded_assets_serve_with_correct_headers() {
    let app = test_app().await;
    let asset = std::fs::read_dir("web/dist/assets")
        .expect("web/dist/assets exists; run `pnpm build` in web/ first")
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .into_string()
        .unwrap();

    let response = call(&app, Method::GET, &format!("/assets/{asset}"), None, ANON).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.content_type(),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=31536000, immutable"
    );
}

async fn login_attempt(app: &impl Endpoint, forwarded_for: &str, password: &str) -> Response {
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login".parse().unwrap())
        .header("x-forwarded-for", forwarded_for)
        .content_type("application/json")
        .body(json!({ "username": "admin", "password": password }).to_string());
    app.get_response(request).await
}

#[tokio::test]
async fn login_attempts_are_rate_limited_per_ip() {
    let app = test_app().await;
    register(&app, "admin", None).await;

    let mut statuses = Vec::new();
    for _ in 0..12 {
        statuses.push(
            login_attempt(&app, "10.9.9.9", "wrong-password")
                .await
                .status(),
        );
    }
    assert!(
        statuses[..10]
            .iter()
            .all(|s| *s == StatusCode::UNAUTHORIZED)
    );
    assert!(
        statuses[10..]
            .iter()
            .all(|s| *s == StatusCode::TOO_MANY_REQUESTS)
    );

    // the bucket is per address: another client is unaffected, and a correct
    // password from the throttled address is still refused
    let response = login_attempt(&app, "10.9.9.10", "wrong-password").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = login_attempt(&app, "10.9.9.9", "password123").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn unlock_attempts_are_rate_limited_per_ip() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let cookie = session_cookie_of(&response);
    let token = agent_token(&app, &cookie).await;
    let response = push(&app, &token, "gated", "<p>doc</p>").await;
    let id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        &app,
        Method::POST,
        "/api/docs/gated/publish",
        Some(json!({ "password": "letmein" })),
        with_cookie(&cookie),
    )
    .await;

    let attempt = |password: &'static str| {
        let path = format!("/{id}/gated");
        let app = &app;
        async move {
            let request = Request::builder()
                .method(Method::POST)
                .uri(path.parse().unwrap())
                .header("x-forwarded-for", "10.7.7.7")
                .content_type("application/x-www-form-urlencoded")
                .body(format!("password={password}"));
            app.get_response(request).await
        }
    };

    let mut statuses = Vec::new();
    for _ in 0..12 {
        statuses.push(attempt("wrong").await.status());
    }
    assert!(
        statuses[..10]
            .iter()
            .all(|s| *s == StatusCode::UNAUTHORIZED)
    );
    assert!(
        statuses[10..]
            .iter()
            .all(|s| *s == StatusCode::TOO_MANY_REQUESTS)
    );

    // even the right password is throttled once the bucket is empty
    let response = attempt("letmein").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn share_page_is_owner_only() {
    let app = test_app().await;
    let response = register(&app, "admin", None).await;
    let cookie = session_cookie_of(&response);
    let token = agent_token(&app, &cookie).await;
    let response = push(&app, &token, "shared-plan", "<p>doc</p>").await;
    let id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let share_path = format!("/{id}/shared-plan/share");

    // owner sees the share controls
    let response = call(&app, Method::GET, &share_path, None, with_cookie(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = response.into_body().into_string().await.unwrap();
    assert!(text.contains("Publish"));
    assert!(text.contains("/api/docs/shared-plan/publish"));

    // anonymous callers and password visitors get the not-found treatment
    let response = call(&app, Method::GET, &share_path, None, ANON).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    call(
        &app,
        Method::POST,
        "/api/docs/shared-plan/publish",
        Some(json!({ "password": "letmein" })),
        with_cookie(&cookie),
    )
    .await;
    let response = unlock(&app, &format!("/{id}/shared-plan"), "letmein").await;
    let access = access_cookie_of(&response);
    let response = call(&app, Method::GET, &share_path, None, with_cookie(&access)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // the owner's overlay share button opens the popout, not a page change
    let response = call(
        &app,
        Method::GET,
        &format!("/{id}/shared-plan/"),
        None,
        with_cookie(&cookie),
    )
    .await;
    let text = response.into_body().into_string().await.unwrap();
    assert!(text.contains(&share_path));
    assert!(text.contains("window.open"));
}

#[tokio::test]
async fn answers_reject_agent_tokens_but_accept_sessions_and_scoped_keys() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    let response = push_with_meta(&app, &token, "plan", "<h1>plan</h1>", one_question()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    // the token that pushed the document cannot answer its questions: an answer
    // records what a human decided
    let response = answer(
        &app,
        "plan",
        "P4",
        json!({ "selected": ["accept"] }),
        with_bearer(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = answer(&app, "plan", "P4", json!({ "selected": ["accept"] }), ANON).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = answer(
        &app,
        "plan",
        "P4",
        json!({ "selected": ["accept"] }),
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let key = scoped_key(&app, &cookie, &id, "plan")
        .await
        .expect("owner gets a scoped key");
    let response = answer(
        &app,
        "plan",
        "P4",
        json!({ "selected": ["defer"], "notes": "after the previews" }),
        with_bearer(&key),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = call(
        &app,
        Method::GET,
        "/api/docs/plan/questions",
        None,
        with_bearer(&token),
    )
    .await;
    let body = json_body(response).await;
    assert_eq!(body[0]["answer"]["selected"], json!(["defer"]));
    assert_eq!(body[0]["answer"]["notes"], json!("after the previews"));
}

#[tokio::test]
async fn a_scoped_key_is_bound_to_one_document() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    let response = push_with_meta(&app, &token, "first", "<h1>first</h1>", one_question()).await;
    let first_id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    push_with_meta(&app, &token, "second", "<h1>second</h1>", one_question()).await;

    let key = scoped_key(&app, &cookie, &first_id, "first").await.unwrap();

    let response = answer(
        &app,
        "second",
        "P4",
        json!({ "selected": ["accept"] }),
        with_bearer(&key),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let tampered = format!("{}00", &key[..key.len() - 2]);
    let response = answer(
        &app,
        "first",
        "P4",
        json!({ "selected": ["accept"] }),
        with_bearer(&tampered),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_answer_must_fit_the_question_that_was_asked() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;
    push_with_meta(&app, &token, "plan", "<h1>plan</h1>", one_question()).await;

    for body in [
        json!({ "selected": [] }),
        json!({ "selected": ["nonsense"] }),
        json!({ "selected": ["accept", "defer"] }),
        json!({ "selected": ["other"] }),
        json!({ "selected": ["accept"], "other_text": "sneaky" }),
    ] {
        let response = answer(&app, "plan", "P4", body.clone(), with_cookie(&cookie)).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "expected {body} to be rejected"
        );
    }

    // a question the revision does not ask cannot be answered at all
    let response = answer(
        &app,
        "plan",
        "invented",
        json!({ "selected": ["accept"] }),
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_answer_survives_a_revision_that_asks_again() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;
    push_with_meta(&app, &token, "plan", "<h1>one</h1>", one_question()).await;
    answer(
        &app,
        "plan",
        "P4",
        json!({ "selected": ["accept"] }),
        with_cookie(&cookie),
    )
    .await;

    // revision 2 asks P4 again and adds P9
    let mut meta = one_question();
    meta["questions"].as_array_mut().unwrap().push(json!({
        "key": "P9",
        "prompt": "Approve the preview worker?",
        "options": [
            { "value": "yes", "label": "Yes" },
            { "value": "no", "label": "No" }
        ]
    }));
    push_with_meta(&app, &token, "plan", "<h1>two</h1>", meta).await;

    let response = call(
        &app,
        Method::GET,
        "/api/docs/plan/questions",
        None,
        with_bearer(&token),
    )
    .await;
    let body = json_body(response).await;
    assert_eq!(body[0]["key"], json!("P4"));
    assert_eq!(body[0]["answer"]["selected"], json!(["accept"]));
    assert_eq!(body[1]["key"], json!("P9"));
    assert_eq!(body[1]["answer"], json!(null));

    // revision 3 stops asking anything; the answer is orphaned, not deleted
    push_with_meta(&app, &token, "plan", "<h1>three</h1>", json!({})).await;
    let response = call(
        &app,
        Method::GET,
        "/api/docs/plan/questions",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(json_body(response).await, json!([]));
}

#[tokio::test]
async fn visitors_get_no_widget_no_key_and_no_questions() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;
    let response = push_with_meta(&app, &token, "plan", "<h1>plan</h1>", one_question()).await;
    let id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    call(
        &app,
        Method::POST,
        "/api/docs/plan/publish",
        Some(json!({ "password": "visitorpass" })),
        with_cookie(&cookie),
    )
    .await;

    let response = unlock(&app, &format!("/{id}/plan"), "visitorpass").await;
    let access = access_cookie_of(&response);
    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/{id}/plan/").parse().unwrap())
        .header(header::COOKIE, &access)
        .finish();
    let html = app
        .get_response(request)
        .await
        .into_body()
        .into_string()
        .await
        .unwrap();
    assert!(!html.contains("planq_"), "visitor must not receive a key");
    assert!(!html.contains("/_planenv/answer"));
    assert!(!html.contains(r#"id="planenv-questions""#));

    // and the owner viewing the same document does get all three
    assert!(scoped_key(&app, &cookie, &id, "plan").await.is_some());
}

#[tokio::test]
async fn a_question_may_not_declare_the_reserved_written_answer() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    let meta = json!({ "questions": [{
        "key": "P4",
        "prompt": "Accept?",
        "options": [
            { "value": "other", "label": "Something else" },
            { "value": "accept", "label": "Accept" }
        ]
    }]});
    let response = push_with_meta(&app, &token, "plan", "<h1>plan</h1>", meta).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_question_cannot_break_out_of_the_json_island() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    let meta = json!({ "questions": [{
        "key": "P4",
        "prompt": "</script><script>window.stolen=document.currentScript</script>",
        "options": [
            { "value": "accept", "label": "Accept" },
            { "value": "defer", "label": "Defer" }
        ]
    }]});
    let response = push_with_meta(&app, &token, "plan", "<h1>plan</h1>", meta).await;
    assert_eq!(response.status(), StatusCode::OK);
    let id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/{id}/plan/").parse().unwrap())
        .header(header::COOKIE, &cookie)
        .finish();
    let html = app
        .get_response(request)
        .await
        .into_body()
        .into_string()
        .await
        .unwrap();

    let island = html
        .split_once(r#"<script type="application/json" id="planenv-questions">"#)
        .expect("question island")
        .1;
    let island = island.split_once("</script>").expect("island ends").0;
    assert!(
        !island.contains('<'),
        "no raw < may survive into the island: {island}"
    );
    // and it still parses back to the prompt that was pushed
    let parsed: Value = serde_json::from_str(island).expect("island is valid JSON");
    assert_eq!(
        parsed[0]["prompt"],
        json!("</script><script>window.stolen=document.currentScript</script>")
    );
}

/// Push a file set through multipart, one part per path.
async fn push_files(
    app: &impl Endpoint,
    token: &str,
    slug: &str,
    files: &[(&str, &str)],
) -> Response {
    const BOUNDARY: &str = "planenvfileboundary";
    let mut body = String::new();
    for (path, content) in files {
        body.push_str(&format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{path}\"\r\n\r\n{content}\r\n"
        ));
    }
    body.push_str(&format!("--{BOUNDARY}--\r\n"));
    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/docs/{slug}").parse().unwrap())
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .content_type(&format!("multipart/form-data; boundary={BOUNDARY}"))
        .body(body);
    app.get_response(request).await
}

#[tokio::test]
async fn a_revision_serves_its_assets_beside_the_document() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    let response = push_files(
        &app,
        &token,
        "multi",
        &[
            ("index.html", "<h1>multi</h1>"),
            ("style.css", "h1{color:red}"),
            ("img/dot.svg", "<svg/>"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let id = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    for (path, content_type) in [
        ("style.css", "text/css; charset=utf-8"),
        ("img/dot.svg", "image/svg+xml"),
    ] {
        let response = call(
            &app,
            Method::GET,
            &format!("/{id}/multi/{path}"),
            None,
            with_cookie(&cookie),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{path} should be served");
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            content_type,
            "content type comes from the extension, not the caller"
        );
    }

    // a private document's assets are as invisible as the document itself
    let response = call(
        &app,
        Method::GET,
        &format!("/{id}/multi/style.css"),
        None,
        ANON,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = call(
        &app,
        Method::GET,
        &format!("/{id}/multi/nothing.css"),
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_file_set_must_fit_its_rules() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    for files in [
        // no entry document
        &[("style.css", "x")][..],
        // reserved by the document's own URLs
        &[("index.html", "<h1>x</h1>"), ("share", "x")][..],
        &[("index.html", "<h1>x</h1>"), ("rev/2.css", "x")][..],
        // traversal
        &[("index.html", "<h1>x</h1>"), ("../secret.css", "x")][..],
        &[("index.html", "<h1>x</h1>"), ("/etc/passwd.txt", "x")][..],
        // extension not on the allowlist
        &[("index.html", "<h1>x</h1>"), ("payload.exe", "x")][..],
    ] {
        let response = push_files(&app, &token, "bad", files).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "expected {files:?} to be rejected"
        );
    }

    // and nothing was written by any of them
    let response = call(
        &app,
        Method::GET,
        "/api/docs/bad",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_assets_cookie_is_bound_to_one_document() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    let first = push_files(
        &app,
        &token,
        "first",
        &[("index.html", "<h1>a</h1>"), ("a.css", "a")],
    )
    .await;
    let first_id = json_body(first).await["id"].as_str().unwrap().to_string();
    let second = push_files(
        &app,
        &token,
        "second",
        &[("index.html", "<h1>b</h1>"), ("b.css", "b")],
    )
    .await;
    let second_id = json_body(second).await["id"].as_str().unwrap().to_string();

    // viewing a document hands out the cookie its subresources need, because a
    // sandboxed page sends no SameSite=Lax cookie of its own
    let response = call(
        &app,
        Method::GET,
        &format!("/{first_id}/first/"),
        None,
        with_cookie(&cookie),
    )
    .await;
    let assets = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| {
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_string()
        })
        .find(|value| value.starts_with("doc_assets="))
        .expect("the document page issues an assets cookie");

    let response = call(
        &app,
        Method::GET,
        &format!("/{first_id}/first/a.css"),
        None,
        with_cookie(&assets),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    // the same cookie is worthless against another document
    let response = call(
        &app,
        Method::GET,
        &format!("/{second_id}/second/b.css"),
        None,
        with_cookie(&assets),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_alias_is_another_name_for_one_project() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    let response = call(
        &app,
        Method::PUT,
        "/api/projects/open-lavatory/aliases/openlv",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // a push naming the alias lands in the canonical project
    push_with_meta(
        &app,
        &token,
        "lav",
        "<h1>lav</h1>",
        json!({ "project": "openlv" }),
    )
    .await;
    let response = call(
        &app,
        Method::GET,
        "/api/docs/lav",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(json_body(response).await["project"], json!("open-lavatory"));

    // and setting an icon through the alias does not mint a project for it
    let response = call(
        &app,
        Method::PUT,
        "/api/projects/openlv/favicon",
        None,
        with_bearer(&token),
    )
    .await;
    // an empty body is refused, so nothing is created either way
    assert!(response.status().is_client_error());

    let response = call(
        &app,
        Method::GET,
        "/api/projects",
        None,
        with_bearer(&token),
    )
    .await;
    let projects = json_body(response).await;
    let slugs: Vec<&str> = projects
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, vec!["open-lavatory"]);
    assert_eq!(projects[0]["aliases"], json!(["openlv"]));

    // a name is either a project or an alias, never both
    let response = call(
        &app,
        Method::PUT,
        "/api/projects/something-else/aliases/open-lavatory",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_project_listing_is_scoped_and_capped() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    for slug in ["a", "b", "c"] {
        push_with_meta(
            &app,
            &token,
            slug,
            "<h1>x</h1>",
            json!({ "project": "alpha" }),
        )
        .await;
    }
    push_with_meta(
        &app,
        &token,
        "other",
        "<h1>x</h1>",
        json!({ "project": "beta" }),
    )
    .await;

    let response = call(
        &app,
        Method::GET,
        "/api/docs?project=alpha",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(json_body(response).await.as_array().unwrap().len(), 3);

    // newest first, so a limit reads the most recent work
    let response = call(
        &app,
        Method::GET,
        "/api/docs?project=alpha&limit=2",
        None,
        with_bearer(&token),
    )
    .await;
    let body = json_body(response).await;
    assert_eq!(body.as_array().unwrap().len(), 2);
    assert_eq!(body[0]["slug"], json!("c"));
}

/// The preview worker reaches the render route over a real loopback socket, so
/// covering it needs a real socket: `RequestState` is private to poem, and an
/// in-process request has no peer address to satisfy the guard with.
#[tokio::test]
async fn the_render_route_serves_the_entry_and_its_assets_over_loopback() {
    use std::io::{Read, Write};

    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;
    push_files(
        &app,
        &token,
        "shot",
        &[
            ("index.html", "<h1>entry</h1>"),
            ("style.css", "h1{color:red}"),
        ],
    )
    .await;

    use poem::listener::{Acceptor, Listener};
    let acceptor = poem::listener::TcpListener::bind("127.0.0.1:0")
        .into_acceptor()
        .await
        .expect("bind");
    let port = acceptor.local_addr()[0]
        .as_socket_addr()
        .expect("socket addr")
        .port();
    tokio::spawn(async move {
        let _ = poem::Server::new_with_acceptor(acceptor).run(app).await;
    });

    let fetch = move |path: String| {
        std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
            write!(
                stream,
                "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            response
        })
        .join()
        .expect("request thread")
    };

    // the first push is revision 1; an empty remainder must mean the entry
    // document rather than a file with no name
    let entry = tokio::task::spawn_blocking(move || fetch("/_render/1/".to_string()))
        .await
        .unwrap();
    assert!(entry.starts_with("HTTP/1.1 200 OK"), "entry: {entry}");
    assert!(entry.contains("<h1>entry</h1>"), "entry: {entry}");
    // and it carries no overlay, so a thumbnail shows the document alone
    assert!(!entry.contains("planenv-overlay"), "entry: {entry}");

    let asset = tokio::task::spawn_blocking(move || fetch("/_render/1/style.css".to_string()))
        .await
        .unwrap();
    assert!(asset.starts_with("HTTP/1.1 200 OK"), "asset: {asset}");
    assert!(asset.contains("h1{color:red}"), "asset: {asset}");
}

#[tokio::test]
async fn refreshing_a_preview_requeues_a_stored_one() {
    let app = test_app().await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;
    push(&app, &token, "plan", "<h1>plan</h1>").await;

    let response = call(
        &app,
        Method::POST,
        "/api/docs/plan/preview/refresh",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let response = call(
        &app,
        Method::POST,
        "/api/docs/nothing/preview/refresh",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// A body over the inline limit goes to the bucket, and every read path still
/// returns it. Without this the split storage is only ever exercised in
/// production, where the failure mode is a document that will not open.
#[tokio::test]
async fn a_large_body_lands_in_the_bucket_and_still_reads_back() {
    let (app, pool, _) = test_app_with_blobs(Some(crate::blobs::Blobs::in_memory())).await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    let filler = "x".repeat(crate::blobs::INLINE_LIMIT as usize + 1);
    let html = format!("<h1>big</h1><!--{filler}-->");
    assert_eq!(push(&app, &token, "big", &html).await.status(), StatusCode::OK);

    let (content, object_key): (Option<Vec<u8>>, Option<String>) =
        sqlx::query_as("SELECT content, object_key FROM revision_files WHERE path = 'index.html'")
            .fetch_one(&pool)
            .await
            .expect("the row exists");
    assert!(content.is_none(), "the bytes should not be inline");
    assert!(object_key.is_some(), "the row should carry a key");

    let raw = call(
        &app,
        Method::GET,
        "/api/docs/big/raw",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(raw.status(), StatusCode::OK);
    assert!(
        raw.into_body().into_string().await.unwrap().contains("<h1>big</h1>"),
        "the raw route should resolve the body out of the bucket"
    );
}

/// The sweep moves a superseded revision out and leaves the latest one inline,
/// which is the whole point of tiering by supersession rather than by size.
#[tokio::test]
async fn the_sweep_demotes_superseded_revisions_only() {
    let blobs = crate::blobs::Blobs::in_memory();
    let (app, pool, _) = test_app_with_blobs(Some(blobs.clone())).await;
    let cookie = session_cookie_of(&register(&app, "admin", None).await);
    let token = agent_token(&app, &cookie).await;

    push(&app, &token, "plan", "<h1>first</h1>").await;
    push(&app, &token, "plan", "<h1>second</h1>").await;

    let moved = crate::demote::sweep_all(&pool, Some(&blobs))
        .await
        .expect("the sweep runs");
    assert_eq!(moved, 1, "only the superseded revision should move");

    let inline: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM revision_files WHERE content IS NOT NULL")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(inline, 1, "the latest revision stays inline");

    // the pinned URL for the demoted revision must still serve its body
    let old = call(
        &app,
        Method::GET,
        "/api/docs/plan/revisions/1/raw",
        None,
        with_bearer(&token),
    )
    .await;
    assert_eq!(old.status(), StatusCode::OK);
    assert!(
        old.into_body().into_string().await.unwrap().contains("<h1>first</h1>"),
        "a superseded revision must read back out of the bucket"
    );
}
