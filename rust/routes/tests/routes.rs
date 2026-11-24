//! Tests de integración de `karaoke-routes`: el dispatch completo
//! contra una BD temporal (nunca `data/`) y un `public/` temporal,
//! sin el servidor Node original.
//!
//! Cubre los caminos principales de cada handler; el replay de la
//! app (295 casos) es el que verifica la paridad byte a byte contra
//! el original.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use serde_json::{Value, json};

use karaoke_db::Room;
use karaoke_db::store::{Db, SeedOpts, open_database, seed};
use karaoke_routes::{AppState, dispatch};

fn test_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-data")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::create_dir_all(dir.join("public")).unwrap();
    std::fs::write(
        dir.join("public").join("index.html"),
        "<html>karaoke</html>",
    )
    .unwrap();
    std::fs::write(dir.join("public").join("app.js"), "console.log(1);").unwrap();
    dir
}

fn state_for(dir: &Path) -> AppState {
    let mut db: Db = open_database(&karaoke_db::js::JsValue::Str(
        dir.join("data").to_string_lossy().into_owned(),
    ))
    .unwrap();
    let room: Room = seed(
        &mut db,
        &SeedOpts {
            admin_user: "admin".to_string(),
            admin_password: concat!("admin", "123").to_string(),
            room_name: "Sala principal".to_string(),
            room_password: None,
        },
    )
    .unwrap()
    .room;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    AppState {
        db: Mutex::new(db),
        room: Mutex::new(room),
        public_dir: dir.join("public"),
        client,
    }
}

async fn call(state: &AppState, req: Request<Body>) -> (StatusCode, Value) {
    let res = dispatch(state, req).await;
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn call_raw(state: &AppState, req: Request<Body>) -> (StatusCode, Vec<u8>, HeaderMap) {
    let res = dispatch(state, req).await;
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, bytes, headers)
}

fn req_json(method: Method, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        b = b.header(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    b.body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

fn req_plain(method: Method, uri: &str, token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    b.body(Body::empty()).unwrap()
}

async fn admin_token(state: &AppState) -> String {
    let (_, body) = call(
        state,
        req_json(
            Method::POST,
            "/api/auth/login",
            None,
            json!({"username": "admin", "password": "admin123"}),
        ),
    )
    .await;
    body["token"].as_str().unwrap().to_string()
}

async fn guest_token(state: &AppState) -> String {
    let code = state.room.lock().unwrap().code.clone();
    let (_, body) = call(
        state,
        req_json(
            Method::POST,
            "/api/join",
            None,
            json!({"code": code, "name": "Prueba"}),
        ),
    )
    .await;
    body["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn health_and_me() {
    let dir = test_dir("health");
    let state = state_for(&dir);
    let (status, body) = call(&state, req_plain(Method::GET, "/api/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"ok": true, "app": "KaraokeMuse", "version": "1.0.0"})
    );

    let (status, body) = call(&state, req_plain(Method::GET, "/api/me", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        body,
        json!({"error": "No autenticado. Entra a la sala con el QR o inicia sesión."})
    );

    let token = admin_token(&state).await;
    let (status, body) = call(&state, req_plain(Method::GET, "/api/me", Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["username"], "admin");
    assert_eq!(body["user"]["role"], "admin");

    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/auth/login",
            None,
            json!({"username": "admin", "password": "mala"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({"error": "Usuario o contraseña incorrectos."}));
}

#[tokio::test]
async fn join_and_room() {
    let dir = test_dir("join");
    let state = state_for(&dir);
    let code = state.room.lock().unwrap().code.clone();

    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/join",
            None,
            json!({"code": "XXXX", "name": "X"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        json!({"error": "Código de sala incorrecto. Escanea el QR de nuevo."})
    );

    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/join",
            None,
            json!({"code": code, "name": "  Ana  "}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["username"], "Ana");
    assert_eq!(body["user"]["role"], "guest");
    assert_eq!(body["room"]["code"], code);

    let token = body["token"].as_str().unwrap().to_string();
    let (status, body) = call(&state, req_plain(Method::GET, "/api/room", Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], code);
    assert_eq!(body["hasPassword"], false);
}

#[tokio::test]
async fn songs_crud_and_validation() {
    let dir = test_dir("songs");
    let state = state_for(&dir);
    let guest = guest_token(&state).await;
    let admin = admin_token(&state).await;

    let (status, body) = call(&state, req_plain(Method::GET, "/api/songs", Some(&guest))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"songs": []}));

    // Validación: sin título.
    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&guest),
            json!({"platform": "youtube", "url": "https://youtube.com/watch?v=x"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("title"));

    // Validación: plataforma inválida.
    let (status, _) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&guest),
            json!({"title": "T", "platform": "vimeo"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Invitado no puede registrar local (403, no 400).
    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&guest),
            json!({"title": "T", "platform": "local", "local_path": "/no/existe.mp4"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("no pueden registrar vídeos locales")
    );

    // YouTube OK como invitado.
    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&guest),
            json!({"title": "Cielito", "platform": "youtube",
                   "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                   "artist": "Tradicional", "rating": 5}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let song_id = body["song"]["id"].as_str().unwrap().to_string();
    assert_eq!(body["song"]["is_favorite"], false);
    assert_eq!(body["song"]["rating"], 5);

    // Filtro por plataforma y por rating.
    let (status, body) = call(
        &state,
        req_plain(Method::GET, "/api/songs?platform=spotify", Some(&guest)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["songs"].as_array().unwrap().len(), 0);
    let (status, body) = call(
        &state,
        req_plain(Method::GET, "/api/songs?minRating=5", Some(&guest)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["songs"].as_array().unwrap().len(), 1);

    // PATCH como invitado → 403; como admin → 200.
    let (status, _) = call(
        &state,
        req_json(
            Method::PATCH,
            &format!("/api/songs/{song_id}"),
            Some(&guest),
            json!({"rating": 4}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = call(
        &state,
        req_json(
            Method::PATCH,
            &format!("/api/songs/{song_id}"),
            Some(&admin),
            json!({"rating": 4}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["song"]["rating"], 4);

    // PATCH rating inválido → 400.
    let (status, _) = call(
        &state,
        req_json(
            Method::PATCH,
            &format!("/api/songs/{song_id}"),
            Some(&admin),
            json!({"rating": 6}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // DELETE como invitado dueño → 200 (borra su propia canción).
    let (status, body) = call(
        &state,
        req_plain(
            Method::DELETE,
            &format!("/api/songs/{song_id}"),
            Some(&guest),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"ok": true}));
    let (status, _) = call(
        &state,
        req_plain(
            Method::DELETE,
            &format!("/api/songs/{song_id}"),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ADR-024: los enlaces cortos de YouTube (youtu.be, los que comparte
// la app de YouTube) se aceptan como YouTube; antes, Detectar los
// etiquetaba como Spotify y agregarlos fallaba con
// "url debe ser de Spotify".
#[tokio::test]
async fn songs_accept_youtu_be_links() {
    let dir = test_dir("youtu-be");
    let state = state_for(&dir);
    let guest = guest_token(&state).await;

    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&guest),
            json!({"title": "No confíes en mí", "platform": "youtube",
                   "url": "https://youtu.be/elOen9IhAL4", "artist": "Camilo séptimo"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["song"]["platform"], "youtube");
    assert_eq!(body["song"]["url"], "https://youtu.be/elOen9IhAL4");

    // La misma URL con plataforma spotify sigue rechazándose: es el
    // error que veía el usuario cuando Detectar la etiquetaba mal.
    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&guest),
            json!({"title": "No confíes en mí", "platform": "spotify",
                   "url": "https://youtu.be/elOen9IhAL4"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("url debe ser de Spotify")
    );
}

#[tokio::test]
async fn queue_next_and_player() {
    let dir = test_dir("queue");
    let state = state_for(&dir);
    let guest = guest_token(&state).await;
    let admin = admin_token(&state).await;

    let (_, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&guest),
            json!({"title": "A", "platform": "youtube",
                   "url": "https://www.youtube.com/watch?v=a"}),
        ),
    )
    .await;
    let song_id = body["song"]["id"].as_str().unwrap().to_string();

    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/queue",
            Some(&guest),
            json!({"songId": song_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(body["queueId"].as_str().is_some());

    let (status, body) = call(&state, req_plain(Method::GET, "/api/queue", Some(&guest))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["queue"].as_array().unwrap().len(), 1);
    assert_eq!(body["queue"][0]["song"]["title"], "A");
    assert_eq!(body["queue"][0]["position"], 1);

    // Player vacío al principio.
    let (status, body) = call(&state, req_plain(Method::GET, "/api/player", Some(&guest))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"song": null}));

    // next como invitado → 403; como admin → la canción.
    let (status, _) = call(
        &state,
        req_plain(Method::POST, "/api/queue/next", Some(&guest)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = call(
        &state,
        req_plain(Method::POST, "/api/queue/next", Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["song"]["title"], "A");

    let (status, body) = call(&state, req_plain(Method::GET, "/api/player", Some(&guest))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["song"]["title"], "A");

    // Cola vacía ahora.
    let (status, body) = call(
        &state,
        req_plain(Method::POST, "/api/queue/next", Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["song"], Value::Null);
    assert_eq!(body["message"], "La cola está vacía.");
}

#[tokio::test]
async fn requests_flow() {
    let dir = test_dir("requests");
    let state = state_for(&dir);
    let guest = guest_token(&state).await;
    let admin = admin_token(&state).await;

    let (status, _) = call(
        &state,
        req_json(
            Method::POST,
            "/api/requests",
            Some(&guest),
            json!({"title": "Sin enlace"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/requests",
            Some(&guest),
            json!({"video_url": "https://www.youtube.com/watch?v=x",
                   "title": "Petición", "difficulty": "dificil"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let req_id = body["request"]["id"].as_str().unwrap().to_string();
    assert_eq!(body["request"]["status"], "pending");

    // El invitado solo ve las suyas.
    let (status, body) = call(
        &state,
        req_plain(Method::GET, "/api/requests", Some(&guest)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["requests"].as_array().unwrap().len(), 1);

    // El admin la marca en progreso.
    let (status, body) = call(
        &state,
        req_json(
            Method::PATCH,
            &format!("/api/requests/{req_id}"),
            Some(&admin),
            json!({"status": "in_progress", "admin_note": "En cola"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["request"]["status"], "in_progress");
    assert_eq!(body["request"]["admin_note"], "En cola");

    // Status inválido → 400.
    let (status, _) = call(
        &state,
        req_json(
            Method::PATCH,
            &format!("/api/requests/{req_id}"),
            Some(&admin),
            json!({"status": "x"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn media_range_and_static() {
    let dir = test_dir("media");
    let video = dir.join("test-video.mp4");
    std::fs::write(&video, b"fake-mp4-content-0123456789abcdefghijklmno").unwrap();
    let state = state_for(&dir);
    let admin = admin_token(&state).await;

    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&admin),
            json!({"title": "Local", "platform": "local",
                   "local_path": video.to_string_lossy()}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let song_id = body["song"]["id"].as_str().unwrap().to_string();

    // Sin Range: 200, cuerpo completo.
    let (status, bytes, headers) = call_raw(
        &state,
        req_plain(
            Method::GET,
            &format!("/api/media/local/{song_id}"),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.len(), 42);
    assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "video/mp4");

    // Range válido: 206, 10 bytes.
    let mut r = req_plain(
        Method::GET,
        &format!("/api/media/local/{song_id}"),
        Some(&admin),
    );
    r.headers_mut()
        .insert(header::RANGE, HeaderValue::from_static("bytes=0-9"));
    let (status, bytes, headers) = call_raw(&state, r).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(bytes.len(), 10);
    assert_eq!(headers.get(header::CONTENT_RANGE).unwrap(), "bytes 0-9/42");
    assert_eq!(headers.get(header::CONTENT_LENGTH).unwrap(), "10");

    // Range fuera del archivo (start > tamaño, end > start): Node
    // responde 206 con content-length end-start+1 y cuerpo vacío
    // (el stream lee en EOF y termina sin datos).
    let mut r = req_plain(
        Method::GET,
        &format!("/api/media/local/{song_id}"),
        Some(&admin),
    );
    r.headers_mut()
        .insert(header::RANGE, HeaderValue::from_static("bytes=100-200"));
    let (status, bytes, headers) = call_raw(&state, r).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(bytes.len(), 0);
    assert_eq!(headers.get(header::CONTENT_LENGTH).unwrap(), "101");

    // Range imposible (end < start): content-length negativo hace
    // lanzar a writeHead en Node → 500.
    let mut r = req_plain(
        Method::GET,
        &format!("/api/media/local/{song_id}"),
        Some(&admin),
    );
    r.headers_mut()
        .insert(header::RANGE, HeaderValue::from_static("bytes=200-100"));
    let (status, _, _) = call_raw(&state, r).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    // Estáticos: / sirve index.html; ruta desconocida → 404 JSON.
    let (status, bytes, headers) =
        call_raw(&state, req_plain(Method::GET, "/", Some(&admin))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"<html>karaoke</html>");
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/html")
    );

    let (status, body) = call(
        &state,
        req_plain(Method::GET, "/api/noexiste", Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({"error": "No encontrado"}));
}

#[tokio::test]
async fn metadata_unsupported_and_body_limits() {
    let dir = test_dir("limits");
    let state = state_for(&dir);
    let guest = guest_token(&state).await;

    // Plataforma no soportada: camino de error sin red.
    let (status, body) = call(
        &state,
        req_plain(
            Method::GET,
            "/api/metadata?url=https%3A%2F%2Fexample.com%2Fx",
            Some(&guest),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Plataforma no soportada")
    );

    // JSON inválido → 500 (el catch de handle).
    let r = Request::builder()
        .method(Method::POST)
        .uri("/api/songs")
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {guest}")).unwrap(),
        )
        .body(Body::from("{no json"))
        .unwrap();
    let (status, body) = call(&state, r).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body, json!({"error": "Error interno del servidor."}));

    // Cuerpo > 1 MiB → 500.
    let big = "x".repeat(1024 * 1024 + 10);
    let r = Request::builder()
        .method(Method::POST)
        .uri("/api/songs")
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {guest}")).unwrap(),
        )
        .body(Body::from(big))
        .unwrap();
    let (status, body) = call(&state, r).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body, json!({"error": "Error interno del servidor."}));
}

#[tokio::test]
async fn server_info_shape() {
    // ADR-020: sin autenticación, con la forma esperada; el puerto es
    // 8080 porque la request de prueba no trae Host.
    let dir = test_dir("server-info");
    let state = state_for(&dir);
    let (status, body) = call(&state, req_plain(Method::GET, "/api/server-info", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["port"], 8080);
    assert!(body.get("lan_ip").is_some());
    assert!(body.get("lan_url").is_some());
}

#[tokio::test]
async fn queue_previous_flow() {
    // ADR-020: next guarda la anterior; previous vuelve a ella y la
    // actual regresa al frente de la cola.
    let dir = test_dir("previous");
    let state = state_for(&dir);
    let guest = guest_token(&state).await;
    let admin = admin_token(&state).await;

    let mut ids = Vec::new();
    for title in ["A", "B"] {
        let (_, body) = call(
            &state,
            req_json(
                Method::POST,
                "/api/songs",
                Some(&guest),
                json!({"title": title, "platform": "youtube",
                       "url": "https://www.youtube.com/watch?v=a"}),
            ),
        )
        .await;
        let id = body["song"]["id"].as_str().unwrap().to_string();
        let (status, _) = call(
            &state,
            req_json(
                Method::POST,
                "/api/queue",
                Some(&guest),
                json!({"songId": id}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        ids.push(id);
    }

    // previous como invitado → 403.
    let (status, _) = call(
        &state,
        req_plain(Method::POST, "/api/queue/previous", Some(&guest)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Sin anterior todavía → null.
    let (status, body) = call(
        &state,
        req_plain(Method::POST, "/api/queue/previous", Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["song"], Value::Null);

    // next → A; next → B; previous → A otra vez, B al frente de la cola.
    let (_, body) = call(
        &state,
        req_plain(Method::POST, "/api/queue/next", Some(&admin)),
    )
    .await;
    assert_eq!(body["song"]["title"], "A");
    let (_, body) = call(
        &state,
        req_plain(Method::POST, "/api/queue/next", Some(&admin)),
    )
    .await;
    assert_eq!(body["song"]["title"], "B");
    let (status, body) = call(
        &state,
        req_plain(Method::POST, "/api/queue/previous", Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["song"]["title"], "A");
    let (_, body) = call(&state, req_plain(Method::GET, "/api/queue", Some(&guest))).await;
    assert_eq!(body["queue"].as_array().unwrap().len(), 1);
    assert_eq!(body["queue"][0]["song"]["title"], "B");
    let (_, body) = call(&state, req_plain(Method::GET, "/api/player", Some(&guest))).await;
    assert_eq!(body["song"]["title"], "A");

    // Ya no hay anterior (se consumió).
    let (status, body) = call(
        &state,
        req_plain(Method::POST, "/api/queue/previous", Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["song"], Value::Null);
    let _ = ids;
}

#[tokio::test]
async fn room_patch_and_delete() {
    // ADR-020: renombrar, poner/quitar contraseña de sala y eliminar
    // la sala (se crea una nueva con código nuevo).
    let dir = test_dir("room-admin");
    let state = state_for(&dir);
    let guest = guest_token(&state).await;
    let admin = admin_token(&state).await;
    let old_code = state.room.lock().unwrap().code.clone();

    // Invitado no puede editar la sala.
    let (status, _) = call(
        &state,
        req_json(
            Method::PATCH,
            "/api/room",
            Some(&guest),
            json!({"name": "Otra"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Renombrar + poner contraseña.
    let (status, body) = call(
        &state,
        req_json(
            Method::PATCH,
            "/api/room",
            Some(&admin),
            json!({"name": "Karaoke de Prueba", "password": "secreta"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Karaoke de Prueba");
    assert_eq!(body["code"], old_code);
    assert_eq!(body["hasPassword"], true);

    // Entrar sin contraseña → 401; con ella → 200.
    let (status, _) = call(
        &state,
        req_json(
            Method::POST,
            "/api/join",
            None,
            json!({"code": old_code, "name": "SinClave"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/join",
            None,
            json!({"code": old_code, "name": "ConClave", "password": "secreta"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["room"]["name"], "Karaoke de Prueba");

    // Quitar la contraseña.
    let (status, body) = call(
        &state,
        req_json(
            Method::PATCH,
            "/api/room",
            Some(&admin),
            json!({"password": ""}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["hasPassword"], false);

    // Eliminar como invitado → 403; como admin → sala nueva.
    let (status, _) = call(&state, req_plain(Method::DELETE, "/api/room", Some(&guest))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = call(&state, req_plain(Method::DELETE, "/api/room", Some(&admin))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Sala principal");
    assert_ne!(body["code"].as_str().unwrap(), old_code);
    let new_code = body["code"].as_str().unwrap().to_string();

    // El código viejo ya no entra; el nuevo sí.
    let (status, _) = call(
        &state,
        req_json(
            Method::POST,
            "/api/join",
            None,
            json!({"code": old_code, "name": "Viejo"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &state,
        req_json(
            Method::POST,
            "/api/join",
            None,
            json!({"code": new_code, "name": "Nuevo"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn admin_password_change() {
    // ADR-020: con la actual mal → 401; con la actual bien → 200 y la
    // nueva ya sirve para entrar.
    let dir = test_dir("admin-pass");
    let state = state_for(&dir);
    let admin = admin_token(&state).await;

    let (status, _) = call(
        &state,
        req_json(
            Method::PATCH,
            "/api/admin/password",
            Some(&admin),
            json!({"current_password": "nope", "new_password": "nueva123"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = call(
        &state,
        req_json(
            Method::PATCH,
            "/api/admin/password",
            Some(&admin),
            json!({"current_password": "admin123", "new_password": "nueva123"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"ok": true}));

    let (status, _) = call(
        &state,
        req_json(
            Method::POST,
            "/api/auth/login",
            None,
            json!({"username": "admin", "password": "admin123"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/auth/login",
            None,
            json!({"username": "admin", "password": "nueva123"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["token"].as_str().is_some());
}

#[tokio::test]
async fn local_path_normalization_and_browse() {
    // ADR-020: una ruta con comillas (como la que copia el Explorador
    // de Windows) se registra igual, guardada ya normalizada; y el
    // explorador lista los vídeos de la carpeta.
    let dir = test_dir("local-path");
    let video = dir.join("mi-cancion.mp4");
    std::fs::write(&video, b"fake mp4").unwrap();
    let state = state_for(&dir);
    let admin = admin_token(&state).await;
    let guest = guest_token(&state).await;

    // Ruta inexistente → 400 con la ruta en el mensaje.
    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&admin),
            json!({"title": "Falta", "platform": "local",
                   "local_path": dir.join("no-existe.mp4").to_string_lossy()}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("no existe"));

    // Con comillas alrededor → 201 y guardada sin comillas.
    let quoted = format!("\"{}\"", video.to_string_lossy());
    let (status, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&admin),
            json!({"title": "Local", "platform": "local", "local_path": quoted}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        body["song"]["local_path"].as_str(),
        Some(video.to_str().unwrap())
    );
    let song_id = body["song"]["id"].as_str().unwrap().to_string();

    // PATCH con otra ruta entrecomillada también normaliza.
    let video2 = dir.join("otra.mp4");
    std::fs::write(&video2, b"fake mp4 2").unwrap();
    let quoted2 = format!("\"{}\"", video2.to_string_lossy());
    let (status, body) = call(
        &state,
        req_json(
            Method::PATCH,
            &format!("/api/songs/{song_id}"),
            Some(&admin),
            json!({"local_path": quoted2}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["song"]["local_path"].as_str(),
        Some(video2.to_str().unwrap())
    );

    // Explorar: invitado → 403; admin → ve el mp4 y no otros archivos.
    std::fs::write(dir.join("nota.txt"), b"no es video").unwrap();
    let (status, _) = call(
        &state,
        req_plain(
            Method::GET,
            &format!("/api/admin/browse?dir={}", dir.to_string_lossy()),
            Some(&guest),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = call(
        &state,
        req_plain(
            Method::GET,
            &format!("/api/admin/browse?dir={}", dir.to_string_lossy()),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entries = body["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["name"] == "mi-cancion.mp4"));
    assert!(!entries.iter().any(|e| e["name"] == "nota.txt"));

    // Ruta relativa → 400.
    let (status, _) = call(
        &state,
        req_plain(
            Method::GET,
            "/api/admin/browse?dir=relativa/videos",
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn media_token_by_query() {
    // ADR-020: el <video> no manda el header Authorization; el token
    // por query (?token=) debe servir igual, y sin token → 401.
    let dir = test_dir("media-query");
    let video = dir.join("test-video.mp4");
    std::fs::write(&video, b"fake-mp4-content-0123456789abcdefghijklmno").unwrap();
    let state = state_for(&dir);
    let admin = admin_token(&state).await;

    let (_, body) = call(
        &state,
        req_json(
            Method::POST,
            "/api/songs",
            Some(&admin),
            json!({"title": "Local", "platform": "local",
                   "local_path": video.to_string_lossy()}),
        ),
    )
    .await;
    let song_id = body["song"]["id"].as_str().unwrap().to_string();

    // Sin token → 401 (como lo veía el <video> antes del arreglo).
    let (status, _) = call(
        &state,
        req_plain(Method::GET, &format!("/api/media/local/{song_id}"), None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Con ?token= → 200 con el cuerpo completo.
    let (status, bytes, _) = call_raw(
        &state,
        req_plain(
            Method::GET,
            &format!("/api/media/local/{song_id}?token={admin}"),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.len(), 42);
}
