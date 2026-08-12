use poem::http::{Method, StatusCode, header};
use poem::{Endpoint, Request, Response};
use serde_json::{Value, json};
use sqlx::sqlite::SqlitePoolOptions;

async fn test_app() -> impl Endpoint {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("memory db");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    crate::app(
        pool,
        crate::config::BaseUrl("http://test.local".to_string()),
        crate::config::Secret("test-secret".to_string()),
    )
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

    let exactly_max = "a".repeat(crate::api::docs::MAX_HTML_BYTES);
    let response = push(&app, &token, "big", &exactly_max).await;
    assert_eq!(response.status(), StatusCode::OK);

    let too_big = "a".repeat(crate::api::docs::MAX_HTML_BYTES + 1);
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
    let doc_path = format!("/{id}/plan");

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
        json!(format!("http://test.local{doc_path}"))
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
        &format!("{doc_path}/rev/1"),
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
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/{id}/real-slug")
    );

    let response = call(
        &app,
        Method::GET,
        &format!("/{id}/wrong-slug/rev/1"),
        None,
        with_cookie(&cookie),
    )
    .await;
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/{id}/real-slug/rev/1")
    );

    // an unknown public id is a plain 404
    let response = call(
        &app,
        Method::GET,
        "/AAAAAAAAAA/whatever",
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

    // a doc-shaped miss stays a hard 404
    let response = call(&app, Method::GET, "/AAAAAAAAAA/nope", None, ANON).await;
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
        &format!("/{id}/shared-plan"),
        None,
        with_cookie(&cookie),
    )
    .await;
    let text = response.into_body().into_string().await.unwrap();
    assert!(text.contains(&share_path));
    assert!(text.contains("window.open"));
}
