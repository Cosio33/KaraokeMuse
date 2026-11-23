//! Written by `rust-anything app lift`: every route is answered in Rust.

use std::collections::HashMap;

use axum::Router;
use axum::extract::Request;
use axum::response::Response;
use axum::routing::any;

mod karaoke_routes;

/// The parameters of `route` (`/users/{id}`, `/files/{*rest}`) in `path`, percent-decoded; one
/// whose escapes are not UTF-8 stays as sent.
fn path_params(route: &str, path: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut segments = path.trim_start_matches('/').split('/');
    for part in route.trim_start_matches('/').split('/') {
        if let Some(name) = part.strip_prefix("{*").and_then(|p| p.strip_suffix('}')) {
            let rest: Vec<&str> = segments.by_ref().collect();
            out.insert(name.to_string(), decode(&rest.join("/")));
            break;
        }
        let segment = segments.next().unwrap_or_default();
        if let Some(name) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
            out.insert(name.to_string(), decode(segment));
        }
    }
    out
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |at: usize| bytes.get(at).and_then(|b| char::from(*b).to_digit(16));
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (hex(i + 1), hex(i + 2))
        {
            out.push((high * 16 + low) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

pub fn router() -> Router {
    Router::new()
        .route("/api/auth/login", any(route_1))
        .route("/api/auth/logout", any(route_2))
        .route("/api/health", any(route_0))
        .route("/api/join", any(route_5))
        .route("/api/me", any(route_3))
        .route("/api/media/local/{*rest}", any(route_16))
        .route("/api/metadata", any(route_6))
        .route("/api/player", any(route_11))
        .route("/api/queue", any(route_9))
        .route("/api/queue/next", any(route_10))
        .route("/api/queue/{*rest}", any(route_14))
        .route("/api/requests", any(route_12))
        .route("/api/requests/{*rest}", any(route_15))
        .route("/api/room", any(route_4))
        .route("/api/songs", any(route_7))
        .route("/api/songs/random", any(route_8))
        .route("/api/songs/{*rest}", any(route_13))
        .fallback(fallback)
}

async fn route_0(req: Request) -> Response {
    karaoke_routes::handle("/api/health", HashMap::new(), req).await
}

async fn route_1(req: Request) -> Response {
    karaoke_routes::handle("/api/auth/login", HashMap::new(), req).await
}

async fn route_2(req: Request) -> Response {
    karaoke_routes::handle("/api/auth/logout", HashMap::new(), req).await
}

async fn route_3(req: Request) -> Response {
    karaoke_routes::handle("/api/me", HashMap::new(), req).await
}

async fn route_4(req: Request) -> Response {
    karaoke_routes::handle("/api/room", HashMap::new(), req).await
}

async fn route_5(req: Request) -> Response {
    karaoke_routes::handle("/api/join", HashMap::new(), req).await
}

async fn route_6(req: Request) -> Response {
    karaoke_routes::handle("/api/metadata", HashMap::new(), req).await
}

async fn route_7(req: Request) -> Response {
    karaoke_routes::handle("/api/songs", HashMap::new(), req).await
}

async fn route_8(req: Request) -> Response {
    karaoke_routes::handle("/api/songs/random", HashMap::new(), req).await
}

async fn route_9(req: Request) -> Response {
    karaoke_routes::handle("/api/queue", HashMap::new(), req).await
}

async fn route_10(req: Request) -> Response {
    karaoke_routes::handle("/api/queue/next", HashMap::new(), req).await
}

async fn route_11(req: Request) -> Response {
    karaoke_routes::handle("/api/player", HashMap::new(), req).await
}

async fn route_12(req: Request) -> Response {
    karaoke_routes::handle("/api/requests", HashMap::new(), req).await
}

async fn route_13(req: Request) -> Response {
    karaoke_routes::handle(
        "/api/songs/{*rest}",
        path_params("/api/songs/{*rest}", req.uri().path()),
        req,
    )
    .await
}

async fn route_14(req: Request) -> Response {
    karaoke_routes::handle(
        "/api/queue/{*rest}",
        path_params("/api/queue/{*rest}", req.uri().path()),
        req,
    )
    .await
}

async fn route_15(req: Request) -> Response {
    karaoke_routes::handle(
        "/api/requests/{*rest}",
        path_params("/api/requests/{*rest}", req.uri().path()),
        req,
    )
    .await
}

async fn route_16(req: Request) -> Response {
    karaoke_routes::handle(
        "/api/media/local/{*rest}",
        path_params("/api/media/local/{*rest}", req.uri().path()),
        req,
    )
    .await
}

async fn fallback(req: Request) -> Response {
    karaoke_routes::handle("*", HashMap::new(), req).await
}
