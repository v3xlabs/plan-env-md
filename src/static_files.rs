use poem::http::{StatusCode, header};
use poem::web::Path;
use poem::{Response, handler};
use rust_embed::RustEmbed;

// debug builds read web/dist from disk at runtime; release builds embed it,
// so `pnpm build` must run before `cargo build --release`
#[derive(RustEmbed)]
#[folder = "web/dist"]
struct WebDist;

#[handler]
pub fn index() -> Response {
    index_response()
}
#[handler]
pub fn spa(Path(path): Path<String>) -> Response {
    serve_or_index(&path)
}

/// Serve an embedded file when one matches, otherwise the SPA shell so client
/// routes like /tokens resolve in the browser.
pub fn serve_or_index(path: &str) -> Response {
    let path = path.strip_prefix('/').unwrap_or(path);

    if WebDist::get(path).is_some() {
        file_response(path)
    } else {
        index_response()
    }
}

pub fn index_response() -> Response {
    file_response("index.html")
}

fn file_response(path: &str) -> Response {
    let Some(file) = WebDist::get(path) else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .content_type("text/plain; charset=utf-8")
            .body("not found (web/dist is missing from this build)");
    };
    // asset filenames are content-hashed by vite; the shell must revalidate
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    Response::builder()
        .content_type(content_type_of(path))
        .header(header::CACHE_CONTROL, cache_control)
        .body(file.data.into_owned())
}

fn content_type_of(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json" | "webmanifest") => "application/json",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}
