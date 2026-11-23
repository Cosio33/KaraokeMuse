//! Handlers de `server/index.js`, en el mismo orden y con la misma
//! semántica que `handle` del original.
//!
//! Reglas que se siguen en todo el archivo:
//! - El cuerpo solo se lee en los handlers que llaman a `readBody`
//!   en Node; un error de cuerpo (JSON inválido, > 1 MiB, `null`)
//!   es `Err` → 500 genérico, como el `catch` de `handle`.
//! - La BD se bloquea solo alrededor de las consultas, nunca a través
//!   de un `await` (el `Mutex` es estándar).
//! - Las respuestas JSON se escriben con `crate::json::write_json`
//!   (formato numérico de JS), nunca con `serde_json::to_string`.

use std::path::Path;
use std::sync::MutexGuard;

use axum::body::Body;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use karaoke_auth::{UserRow, create_session, get_user_from_token, verify_password};
use karaoke_db::js::JsValue;
use karaoke_db::store::Db;
use karaoke_db::time::now_iso;
use karaoke_db::{new_id_str, random_code};

use crate::AppState;
use crate::http_util::{field, has_field, js_slice_utf16, query_param, read_body};
use crate::json::{
    js_number, js_string_or_empty, js_to_string, js_trim, js_truthy, query_all, query_one,
    write_json,
};
use crate::metadata::{Metadata, fetch_metadata};
use crate::validate::validate_song_input;

/// `Err(String)` → 500 `{error:'Error interno del servidor.'}` (el
/// `catch` de `handle` en Node). El `String` es el detalle para el log.
pub type RResult = Result<Response, String>;

/// Lo que necesita cada handler: método, pathname normalizado (crudo,
/// sin decodificar, como `new URL(...).pathname`), query cruda,
/// headers y cuerpo sin leer.
pub struct Ctx {
    pub method: Method,
    pub path: String,
    pub query: String,
    pub headers: HeaderMap,
    pub body: Body,
}

pub fn json_response(status: u16, body: &Value) -> Response {
    let text = write_json(body);
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CONTENT_LENGTH, text.len())
        .body(Body::from(text))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn err_json(status: u16, message: &str) -> Response {
    json_response(status, &json!({ "error": message }))
}

fn unauthenticated() -> Response {
    err_json(
        401,
        "No autenticado. Entra a la sala con el QR o inicia sesión.",
    )
}

fn lock_db(state: &AppState) -> MutexGuard<'_, Db> {
    state.db.lock().unwrap_or_else(|e| e.into_inner())
}

/// `authUser(req)`: el usuario de la sesión Bearer, o `None`.
fn auth_user(state: &AppState, headers: &HeaderMap) -> Result<Option<UserRow>, String> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|t| t.to_string());
    let mut db = lock_db(state);
    get_user_from_token(&mut db, token.as_deref()).map_err(|e| e.to_string())
}

/// `needAuth`: usuario o respuesta 401.
fn need_auth(state: &AppState, headers: &HeaderMap) -> Result<Result<UserRow, Response>, String> {
    match auth_user(state, headers)? {
        Some(u) => Ok(Ok(u)),
        None => Ok(Err(unauthenticated())),
    }
}

/// `needAdmin`: usuario admin o respuesta 401/403.
fn need_admin(state: &AppState, headers: &HeaderMap) -> Result<Result<UserRow, Response>, String> {
    match need_auth(state, headers)? {
        Ok(u) => {
            if u.role != "admin" {
                Ok(Err(err_json(
                    403,
                    "Solo el administrador puede hacer esto.",
                )))
            } else {
                Ok(Ok(u))
            }
        }
        Err(r) => Ok(Err(r)),
    }
}

/// `songOut(row)`: la fila tal cual, con `is_favorite` como booleano.
fn song_out(row: &Value) -> Value {
    let mut out = row.clone();
    if let Some(obj) = out.as_object_mut() {
        let fav = js_truthy(obj.get("is_favorite"));
        obj.insert("is_favorite".to_string(), Value::Bool(fav));
    }
    out
}

/// Un valor JSON como parámetro SQLite (`null`→NULL, bool→0/1,
/// número→INTEGER/REAL, string→TEXT). Un array/objeto no se puede
/// ligar en node:sqlite (lanza) → error → 500.
fn json_to_sql(v: &Value) -> Result<rusqlite::types::Value, String> {
    match v {
        Value::Null => Ok(rusqlite::types::Value::Null),
        Value::Bool(b) => Ok(rusqlite::types::Value::Integer(if *b { 1 } else { 0 })),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(rusqlite::types::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(rusqlite::types::Value::Real(f))
            } else {
                Err("número fuera de rango para SQLite".to_string())
            }
        }
        Value::String(s) => Ok(rusqlite::types::Value::Text(s.clone())),
        Value::Array(_) | Value::Object(_) => {
            Err("node:sqlite no liga arrays/objetos como parámetro".to_string())
        }
    }
}

fn exec_sql(
    conn: &rusqlite::Connection,
    sql: &str,
    params: Vec<rusqlite::types::Value>,
) -> Result<usize, String> {
    conn.execute(sql, rusqlite::params_from_iter(params.iter()))
        .map_err(|e| e.to_string())
}

/// El dispatch de `handle` en `server/index.js`, en el mismo orden.
pub async fn handle(state: &AppState, ctx: Ctx) -> RResult {
    let p = ctx.path.as_str();
    let method = ctx.method.clone();

    // Salud
    if p == "/api/health" && method == Method::GET {
        return Ok(json_response(
            200,
            &json!({"ok": true, "app": "KaraokeMuse", "version": "1.0.0"}),
        ));
    }

    // Login admin
    if p == "/api/auth/login" && method == Method::POST {
        return login(state, ctx).await;
    }
    if p == "/api/auth/logout" && method == Method::POST {
        let token = ctx
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(|t| t.to_string());
        {
            let mut db = lock_db(state);
            karaoke_auth::destroy_session(&mut db, token.as_deref()).map_err(|e| e.to_string())?;
        }
        return Ok(json_response(200, &json!({ "ok": true })));
    }
    if p == "/api/me" && method == Method::GET {
        let user = match need_auth(state, &ctx.headers)? {
            Ok(u) => u,
            Err(r) => return Ok(r),
        };
        return Ok(json_response(
            200,
            &json!({"user": {"id": user.id, "username": user.username, "role": user.role}}),
        ));
    }

    // Sala + unirse con QR/código
    if p == "/api/room" && method == Method::GET {
        let _user = match need_auth(state, &ctx.headers)? {
            Ok(u) => u,
            Err(r) => return Ok(r),
        };
        let room = &state.room;
        return Ok(json_response(
            200,
            &json!({"id": room.id, "name": room.name, "code": room.code,
                     "hasPassword": room.password_hash.as_deref().is_some_and(|h| !h.is_empty())}),
        ));
    }
    if p == "/api/join" && method == Method::POST {
        return join(state, ctx).await;
    }

    // Metadatos del enlace (oEmbed)
    if p == "/api/metadata" && method == Method::GET {
        let _user = match need_auth(state, &ctx.headers)? {
            Ok(u) => u,
            Err(r) => return Ok(r),
        };
        let target = query_param(&ctx.query, "url").unwrap_or_default();
        let meta = fetch_metadata(&state.client, &target).await;
        return Ok(match meta {
            Metadata::Error(e) => json_response(200, &json!({ "ok": false, "error": e })),
            Metadata::Ok(ok) => json_response(
                200,
                &json!({"ok": true, "platform": ok.platform, "title": ok.title,
                        "artist": ok.artist, "thumbnail_url": ok.thumbnail_url}),
            ),
        });
    }

    // Biblioteca
    if p == "/api/songs" && method == Method::GET {
        return songs_list(state, &ctx);
    }
    if p == "/api/songs/random" && method == Method::GET {
        return songs_random(state, &ctx);
    }
    if p == "/api/songs" && method == Method::POST {
        return songs_create(state, ctx).await;
    }
    if p.starts_with("/api/songs/") && method == Method::PATCH {
        return songs_patch(state, ctx).await;
    }
    if p.starts_with("/api/songs/") && method == Method::DELETE {
        return songs_delete(state, &ctx);
    }

    // Cola / playlist activa
    if p == "/api/queue" && method == Method::GET {
        return queue_list(state, &ctx);
    }
    if p == "/api/queue" && method == Method::POST {
        return queue_add(state, ctx).await;
    }
    if p.starts_with("/api/queue/") && method == Method::DELETE {
        return queue_delete(state, &ctx);
    }
    if p == "/api/queue/next" && method == Method::POST {
        return queue_next(state, &ctx);
    }
    if p == "/api/player" && method == Method::GET {
        return player(state, &ctx);
    }

    // Solicitudes de generación de vídeo karaoke
    if p == "/api/requests" && method == Method::GET {
        return requests_list(state, &ctx);
    }
    if p == "/api/requests" && method == Method::POST {
        return requests_create(state, ctx).await;
    }
    if p.starts_with("/api/requests/") && method == Method::PATCH {
        return requests_patch(state, ctx).await;
    }

    // Streaming de vídeos locales
    if p.starts_with("/api/media/local/") && method == Method::GET {
        return media_local(state, ctx).await;
    }

    // Estáticos (frontend)
    if method == Method::GET {
        return serve_static(state, p).await;
    }
    Ok(err_json(404, "Ruta no encontrada."))
}

async fn login(state: &AppState, ctx: Ctx) -> RResult {
    let b = read_body(ctx.body).await?;
    let username = js_string_or_empty(field(&b, "username"));
    let password = js_string_or_empty(field(&b, "password"));
    let (user, session) = {
        let mut db = lock_db(state);
        let row = query_one(
            &db.conn,
            "SELECT * FROM users WHERE username = ?1",
            [username.as_str()],
        )
        .map_err(|e| e.to_string())?;
        let Some(row) = row else {
            return Ok(err_json(401, "Usuario o contraseña incorrectos."));
        };
        let role = row
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let salt = row
            .get("salt")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let hash = row
            .get("password_hash")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let name = row
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let ok = role == "admin"
            && match (&salt, &hash) {
                (Some(s), Some(h)) => verify_password(&password, s, h),
                _ => false,
            };
        if !ok {
            return Ok(err_json(401, "Usuario o contraseña incorrectos."));
        }
        let s = create_session(&mut db, &id, 7).map_err(|e| e.to_string())?;
        (
            UserRow {
                id,
                username: name,
                role,
                created_at: String::new(),
            },
            s,
        )
    };
    Ok(json_response(
        200,
        &json!({"token": session.token, "expiresAt": session.expires_at,
                 "user": {"id": user.id, "username": user.username, "role": user.role}}),
    ))
}

async fn join(state: &AppState, ctx: Ctx) -> RResult {
    let b = read_body(ctx.body).await?;
    let code = js_string_or_empty(field(&b, "code"));
    let code = js_trim(&code).to_uppercase();
    if code != state.room.code {
        return Ok(err_json(
            404,
            "Código de sala incorrecto. Escanea el QR de nuevo.",
        ));
    }
    if let Some(hash) = state.room.password_hash.as_ref().filter(|h| !h.is_empty()) {
        let salt = state.room.salt.clone().unwrap_or_default();
        let password = js_string_or_empty(field(&b, "password"));
        if !verify_password(&password, &salt, hash) {
            return Ok(err_json(401, "Contraseña de sala incorrecta."));
        }
    }
    let name_raw = js_string_or_empty(field(&b, "name"));
    let name = js_slice_utf16(js_trim(&name_raw), 30);
    let name = if name.is_empty() {
        format!(
            "Invitado-{}",
            random_code(Some(&JsValue::Num(4.0))).map_err(|e| e.to_string())?
        )
    } else {
        name
    };
    let id = new_id_str("u").map_err(|e| e.to_string())?;
    let now = now_iso();
    let (token, expires_at) = {
        let mut db = lock_db(state);
        exec_sql(
            &db.conn,
            "INSERT INTO users (id, username, password_hash, salt, role, created_at) VALUES (?1, ?2, NULL, NULL, ?3, ?4)",
            vec![
                rusqlite::types::Value::Text(id.clone()),
                rusqlite::types::Value::Text(name.clone()),
                rusqlite::types::Value::Text("guest".to_string()),
                rusqlite::types::Value::Text(now.clone()),
            ],
        )?;
        let s = create_session(&mut db, &id, 7).map_err(|e| e.to_string())?;
        (s.token, s.expires_at)
    };
    let room = &state.room;
    Ok(json_response(
        200,
        &json!({"token": token, "expiresAt": expires_at,
                 "user": {"id": id, "username": name, "role": "guest"},
                 "room": {"id": room.id, "name": room.name, "code": room.code}}),
    ))
}

fn songs_list(state: &AppState, ctx: &Ctx) -> RResult {
    let _user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let q = query_param(&ctx.query, "q")
        .unwrap_or_default()
        .to_lowercase();
    let platform = query_param(&ctx.query, "platform").unwrap_or_default();
    let min_rating = crate::json::js_number(
        &query_param(&ctx.query, "minRating").unwrap_or_else(|| "0".to_string()),
    );
    let rows = {
        let db = lock_db(state);
        query_all(&db.conn, "SELECT * FROM songs ORDER BY created_at DESC", [])
            .map_err(|e| e.to_string())?
    };
    let mut songs: Vec<Value> = rows.iter().map(song_out).collect();
    if !q.is_empty() {
        songs.retain(|s| {
            let haystack = format!(
                "{} {} {}",
                s.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                s.get("artist").and_then(|v| v.as_str()).unwrap_or(""),
                s.get("genre").and_then(|v| v.as_str()).unwrap_or("")
            )
            .to_lowercase();
            haystack.contains(&q)
        });
    }
    if !platform.is_empty() {
        songs.retain(|s| s.get("platform").and_then(|v| v.as_str()) == Some(platform.as_str()));
    }
    if min_rating != 0.0 && !min_rating.is_nan() {
        songs.retain(|s| s.get("rating").and_then(|v| v.as_f64()).unwrap_or(0.0) >= min_rating);
    }
    Ok(json_response(200, &json!({ "songs": songs })))
}

fn songs_random(state: &AppState, ctx: &Ctx) -> RResult {
    let _user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let raw = query_param(&ctx.query, "count");
    let n = match raw.as_deref() {
        Some(s) if !s.is_empty() => js_number(s),
        _ => 3.0,
    };
    let count = crate::json::js_min(crate::json::js_max(n, 1.0), 10.0);
    let rows = {
        let db = lock_db(state);
        query_all(
            &db.conn,
            "SELECT * FROM songs ORDER BY RANDOM() LIMIT ?1",
            rusqlite::params![count],
        )
        .map_err(|e| e.to_string())?
    };
    let songs: Vec<Value> = rows.iter().map(song_out).collect();
    Ok(json_response(200, &json!({ "songs": songs })))
}

async fn songs_create(state: &AppState, ctx: Ctx) -> RResult {
    let user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let b = read_body(ctx.body).await?;
    let is_guest = user.role == "guest";
    let errors = validate_song_input(&b, is_guest);
    if !errors.is_empty() {
        let status =
            if is_guest && field(&b, "platform") == Some(&Value::String("local".to_string())) {
                403
            } else {
                400
            };
        return Ok(err_json(status, &errors.join(". ")));
    }
    let platform = match field(&b, "platform") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => "youtube".to_string(),
    };
    if platform == "local" {
        let local_path = js_string_or_empty(field(&b, "local_path"));
        if !Path::new(&local_path).exists() {
            return Ok(err_json(
                400,
                &format!("El archivo local no existe en el servidor: {local_path}"),
            ));
        }
    }
    let now = now_iso();
    let id = new_id_str("song").map_err(|e| e.to_string())?;
    let title = js_trim(&js_string_or_empty(field(&b, "title"))).to_string();
    let difficulty = match field(&b, "difficulty") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => "media".to_string(),
    };
    let status = match field(&b, "status") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => "want".to_string(),
    };
    let rating = match field(&b, "rating") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) as i64,
        _ => 0,
    };
    let is_favorite = js_truthy(field(&b, "is_favorite"));
    let song = {
        let db = lock_db(state);
        exec_sql(
            &db.conn,
            "INSERT INTO songs (id, title, artist, platform, url, local_path, thumbnail_url, genre, language, difficulty, status, rating, is_favorite, notes, lyrics_text, owner_user_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            vec![
                rusqlite::types::Value::Text(id.clone()),
                rusqlite::types::Value::Text(title),
                rusqlite::types::Value::Text(
                    js_trim(&js_string_or_empty(field(&b, "artist"))).to_string(),
                ),
                rusqlite::types::Value::Text(platform),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "url"))),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "local_path"))),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "thumbnail_url"))),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "genre"))),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "language"))),
                rusqlite::types::Value::Text(difficulty),
                rusqlite::types::Value::Text(status),
                rusqlite::types::Value::Integer(rating),
                rusqlite::types::Value::Integer(if is_favorite { 1 } else { 0 }),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "notes"))),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "lyrics_text"))),
                rusqlite::types::Value::Text(user.id.clone()),
                rusqlite::types::Value::Text(now.clone()),
                rusqlite::types::Value::Text(now.clone()),
            ],
        )?;
        query_one(&db.conn, "SELECT * FROM songs WHERE id = ?1", [id.as_str()])
            .map_err(|e| e.to_string())?
    };
    let song = song.ok_or_else(|| "canción recién creada no encontrada".to_string())?;
    Ok(json_response(201, &json!({ "song": song_out(&song) })))
}

const SONG_FIELDS: [&str; 14] = [
    "title",
    "artist",
    "platform",
    "url",
    "local_path",
    "thumbnail_url",
    "genre",
    "language",
    "difficulty",
    "status",
    "rating",
    "is_favorite",
    "notes",
    "lyrics_text",
];

async fn songs_patch(state: &AppState, ctx: Ctx) -> RResult {
    let _user = match need_admin(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let id = ctx.path.split('/').nth(3).unwrap_or("").to_string();
    let exists = {
        let db = lock_db(state);
        query_one(&db.conn, "SELECT * FROM songs WHERE id = ?1", [id.as_str()])
            .map_err(|e| e.to_string())?
    };
    if exists.is_none() {
        return Ok(err_json(404, "Canción no encontrada."));
    }
    let b = read_body(ctx.body).await?;
    let mut sets: Vec<String> = Vec::new();
    let mut vals: Vec<rusqlite::types::Value> = Vec::new();
    for f in SONG_FIELDS {
        if !has_field(&b, f) {
            continue;
        }
        let v = field(&b, f).unwrap();
        if f == "platform"
            && !matches!(v, Value::String(s) if ["youtube", "spotify", "local"].contains(&s.as_str()))
        {
            return Ok(err_json(400, "platform inválida"));
        }
        if f == "rating" {
            let ok = match v {
                Value::Number(n) => {
                    let x = n.as_f64().unwrap_or(f64::NAN);
                    x.fract() == 0.0 && (0.0..=5.0).contains(&x)
                }
                _ => false,
            };
            if !ok {
                return Ok(err_json(400, "rating debe ser entero de 0 a 5"));
            }
        }
        sets.push(format!("{f} = ?"));
        vals.push(if f == "is_favorite" {
            rusqlite::types::Value::Integer(if js_truthy(Some(v)) { 1 } else { 0 })
        } else {
            json_to_sql(v)?
        });
    }
    if sets.is_empty() {
        return Ok(err_json(400, "Nada que actualizar."));
    }
    sets.push("updated_at = ?".to_string());
    vals.push(rusqlite::types::Value::Text(now_iso()));
    vals.push(rusqlite::types::Value::Text(id.clone()));
    let sql = format!("UPDATE songs SET {} WHERE id = ?", sets.join(", "));
    let song = {
        let db = lock_db(state);
        exec_sql(&db.conn, &sql, vals)?;
        query_one(&db.conn, "SELECT * FROM songs WHERE id = ?1", [id.as_str()])
            .map_err(|e| e.to_string())?
    };
    let song = song.ok_or_else(|| "canción actualizada no encontrada".to_string())?;
    Ok(json_response(200, &json!({ "song": song_out(&song) })))
}

fn songs_delete(state: &AppState, ctx: &Ctx) -> RResult {
    let user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let id = ctx.path.split('/').nth(3).unwrap_or("");
    let song = {
        let db = lock_db(state);
        query_one(&db.conn, "SELECT * FROM songs WHERE id = ?1", [id]).map_err(|e| e.to_string())?
    };
    let Some(song) = song else {
        return Ok(err_json(404, "Canción no encontrada."));
    };
    if user.role != "admin" && song.get("owner_user_id") != Some(&Value::String(user.id.clone())) {
        return Ok(err_json(
            403,
            "No puedes borrar canciones de otros usuarios. Solo el admin puede hacerlo.",
        ));
    }
    {
        let db = lock_db(state);
        exec_sql(
            &db.conn,
            "DELETE FROM songs WHERE id = ?1",
            vec![rusqlite::types::Value::Text(id.to_string())],
        )?;
        exec_sql(
            &db.conn,
            "DELETE FROM queue WHERE song_id = ?1",
            vec![rusqlite::types::Value::Text(id.to_string())],
        )?;
    }
    Ok(json_response(200, &json!({ "ok": true })))
}

fn queue_list(state: &AppState, ctx: &Ctx) -> RResult {
    let _user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let rows = {
        let db = lock_db(state);
        query_all(
            &db.conn,
            "SELECT q.id AS queue_id, q.position, q.user_id, q.created_at AS queued_at, s.*, u.username AS added_by FROM queue q JOIN songs s ON s.id = q.song_id LEFT JOIN users u ON u.id = q.user_id WHERE q.room_id = ?1 ORDER BY q.position ASC",
            [state.room.id.as_str()],
        )
        .map_err(|e| e.to_string())?
    };
    let mut queue = Vec::new();
    for r in &rows {
        let mut item = serde_json::Map::new();
        item.insert(
            "queueId".to_string(),
            r.get("queue_id").cloned().unwrap_or(Value::Null),
        );
        item.insert(
            "position".to_string(),
            r.get("position").cloned().unwrap_or(Value::Null),
        );
        item.insert(
            "userId".to_string(),
            r.get("user_id").cloned().unwrap_or(Value::Null),
        );
        item.insert(
            "addedBy".to_string(),
            r.get("added_by").cloned().unwrap_or(Value::Null),
        );
        item.insert(
            "queuedAt".to_string(),
            r.get("queued_at").cloned().unwrap_or(Value::Null),
        );
        item.insert("song".to_string(), song_out(r));
        queue.push(Value::Object(item));
    }
    Ok(json_response(200, &json!({ "queue": queue })))
}

async fn queue_add(state: &AppState, ctx: Ctx) -> RResult {
    let user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let b = read_body(ctx.body).await?;
    let song_id = js_string_or_empty(field(&b, "songId"));
    let song = {
        let db = lock_db(state);
        query_one(
            &db.conn,
            "SELECT * FROM songs WHERE id = ?1",
            [song_id.as_str()],
        )
        .map_err(|e| e.to_string())?
    };
    let Some(song) = song else {
        return Ok(err_json(404, "Canción no encontrada."));
    };
    let song_id = song
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let now = now_iso();
    let qid = new_id_str("q").map_err(|e| e.to_string())?;
    {
        let db = lock_db(state);
        let max = query_one(
            &db.conn,
            "SELECT MAX(position) AS m FROM queue WHERE room_id = ?1",
            [state.room.id.as_str()],
        )
        .map_err(|e| e.to_string())?;
        let pos = match max.as_ref().and_then(|r| r.get("m")) {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
            _ => 0,
        } + 1;
        exec_sql(
            &db.conn,
            "INSERT INTO queue (id, room_id, song_id, user_id, position, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            vec![
                rusqlite::types::Value::Text(qid.clone()),
                rusqlite::types::Value::Text(state.room.id.clone()),
                rusqlite::types::Value::Text(song_id),
                rusqlite::types::Value::Text(user.id.clone()),
                rusqlite::types::Value::Integer(pos),
                rusqlite::types::Value::Text(now),
            ],
        )?;
    }
    Ok(json_response(201, &json!({ "queueId": qid })))
}

fn queue_delete(state: &AppState, ctx: &Ctx) -> RResult {
    let user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let id = ctx.path.split('/').nth(3).unwrap_or("");
    let item = {
        let db = lock_db(state);
        query_one(&db.conn, "SELECT * FROM queue WHERE id = ?1", [id]).map_err(|e| e.to_string())?
    };
    let Some(item) = item else {
        return Ok(err_json(404, "Elemento de cola no encontrado."));
    };
    if user.role != "admin" && item.get("user_id") != Some(&Value::String(user.id.clone())) {
        return Ok(err_json(
            403,
            "No puedes quitar de la cola canciones de otros usuarios.",
        ));
    }
    {
        let db = lock_db(state);
        exec_sql(
            &db.conn,
            "DELETE FROM queue WHERE id = ?1",
            vec![rusqlite::types::Value::Text(id.to_string())],
        )?;
    }
    Ok(json_response(200, &json!({ "ok": true })))
}

fn queue_next(state: &AppState, ctx: &Ctx) -> RResult {
    let _user = match need_admin(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let first = {
        let db = lock_db(state);
        query_one(
            &db.conn,
            "SELECT * FROM queue WHERE room_id = ?1 ORDER BY position ASC LIMIT 1",
            [state.room.id.as_str()],
        )
        .map_err(|e| e.to_string())?
    };
    let Some(first) = first else {
        return Ok(json_response(
            200,
            &json!({ "song": null, "message": "La cola está vacía." }),
        ));
    };
    let first_id = first
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let first_song_id = first
        .get("song_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let song = {
        let db = lock_db(state);
        exec_sql(
            &db.conn,
            "DELETE FROM queue WHERE id = ?1",
            vec![rusqlite::types::Value::Text(first_id)],
        )?;
        let song = query_one(
            &db.conn,
            "SELECT * FROM songs WHERE id = ?1",
            [first_song_id.as_str()],
        )
        .map_err(|e| e.to_string())?;
        let current = match &song {
            Some(s) => s
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| rusqlite::types::Value::Text(s.to_string()))
                .unwrap_or(rusqlite::types::Value::Null),
            None => rusqlite::types::Value::Null,
        };
        exec_sql(
            &db.conn,
            "UPDATE player_state SET current_song_id = ?1, updated_at = ?2 WHERE room_id = ?3",
            vec![
                current,
                rusqlite::types::Value::Text(now_iso()),
                rusqlite::types::Value::Text(state.room.id.clone()),
            ],
        )?;
        song
    };
    let out = match &song {
        Some(s) => song_out(s),
        None => Value::Null,
    };
    Ok(json_response(200, &json!({ "song": out })))
}

fn player(state: &AppState, ctx: &Ctx) -> RResult {
    let _user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let song = {
        let db = lock_db(state);
        let st = query_one(
            &db.conn,
            "SELECT * FROM player_state WHERE room_id = ?1",
            [state.room.id.as_str()],
        )
        .map_err(|e| e.to_string())?;
        match st.and_then(|s| s.get("current_song_id").cloned()) {
            Some(Value::String(id)) => {
                query_one(&db.conn, "SELECT * FROM songs WHERE id = ?1", [id.as_str()])
                    .map_err(|e| e.to_string())?
            }
            _ => None,
        }
    };
    let out = match &song {
        Some(s) => song_out(s),
        None => Value::Null,
    };
    Ok(json_response(200, &json!({ "song": out })))
}

fn requests_list(state: &AppState, ctx: &Ctx) -> RResult {
    let user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let rows = {
        let db = lock_db(state);
        if user.role == "admin" {
            query_all(
                &db.conn,
                "SELECT r.*, u.username AS requester FROM requests r LEFT JOIN users u ON u.id = r.requester_user_id ORDER BY r.created_at DESC",
                [],
            )
        } else {
            query_all(
                &db.conn,
                "SELECT r.*, u.username AS requester FROM requests r LEFT JOIN users u ON u.id = r.requester_user_id WHERE r.requester_user_id = ?1 ORDER BY r.created_at DESC",
                [user.id.as_str()],
            )
        }
        .map_err(|e| e.to_string())?
    };
    Ok(json_response(200, &json!({ "requests": rows })))
}

async fn requests_create(state: &AppState, ctx: Ctx) -> RResult {
    let user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let b = read_body(ctx.body).await?;
    if !js_truthy(field(&b, "video_url")) || !js_truthy(field(&b, "title")) {
        return Ok(err_json(
            400,
            "video_url (enlace del vídeo original) y title (nombre de la canción) son obligatorios.",
        ));
    }
    let difficulty_field = field(&b, "difficulty");
    if js_truthy(difficulty_field)
        && !matches!(difficulty_field, Some(Value::String(s)) if ["facil", "media", "dificil"].contains(&s.as_str()))
    {
        return Ok(err_json(400, "difficulty inválida"));
    }
    let difficulty = match difficulty_field {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => "media".to_string(),
    };
    let now = now_iso();
    let id = new_id_str("req").map_err(|e| e.to_string())?;
    let request = {
        let db = lock_db(state);
        exec_sql(
            &db.conn,
            "INSERT INTO requests (id, video_url, title, artist, difficulty, language, notes, status, requester_user_id, admin_note, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            vec![
                rusqlite::types::Value::Text(id.clone()),
                rusqlite::types::Value::Text(js_to_string(field(&b, "video_url"))),
                rusqlite::types::Value::Text(
                    js_trim(&js_to_string(field(&b, "title"))).to_string(),
                ),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "artist"))),
                rusqlite::types::Value::Text(difficulty),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "language"))),
                rusqlite::types::Value::Text(js_string_or_empty(field(&b, "notes"))),
                rusqlite::types::Value::Text("pending".to_string()),
                rusqlite::types::Value::Text(user.id.clone()),
                rusqlite::types::Value::Text(String::new()),
                rusqlite::types::Value::Text(now.clone()),
                rusqlite::types::Value::Text(now.clone()),
            ],
        )?;
        query_one(
            &db.conn,
            "SELECT * FROM requests WHERE id = ?1",
            [id.as_str()],
        )
        .map_err(|e| e.to_string())?
    };
    let request = request.ok_or_else(|| "solicitud recién creada no encontrada".to_string())?;
    Ok(json_response(201, &json!({ "request": request })))
}

async fn requests_patch(state: &AppState, ctx: Ctx) -> RResult {
    let _user = match need_admin(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let id = ctx.path.split('/').nth(3).unwrap_or("").to_string();
    let exists = {
        let db = lock_db(state);
        query_one(
            &db.conn,
            "SELECT * FROM requests WHERE id = ?1",
            [id.as_str()],
        )
        .map_err(|e| e.to_string())?
    };
    if exists.is_none() {
        return Ok(err_json(404, "Solicitud no encontrada."));
    }
    let b = read_body(ctx.body).await?;
    let status_field = field(&b, "status");
    if js_truthy(status_field)
        && !matches!(status_field, Some(Value::String(s)) if ["pending", "in_progress", "done"].contains(&s.as_str()))
    {
        return Ok(err_json(400, "status inválido"));
    }
    let status_param = match status_field {
        Some(Value::String(s)) if !s.is_empty() => rusqlite::types::Value::Text(s.clone()),
        _ => rusqlite::types::Value::Null,
    };
    let admin_note_param = match field(&b, "admin_note") {
        None | Some(Value::Null) => rusqlite::types::Value::Null,
        Some(v) => json_to_sql(v)?,
    };
    let request = {
        let db = lock_db(state);
        exec_sql(
            &db.conn,
            "UPDATE requests SET status = COALESCE(?1, status), admin_note = COALESCE(?2, admin_note), updated_at = ?3 WHERE id = ?4",
            vec![
                status_param,
                admin_note_param,
                rusqlite::types::Value::Text(now_iso()),
                rusqlite::types::Value::Text(id.clone()),
            ],
        )?;
        query_one(
            &db.conn,
            "SELECT * FROM requests WHERE id = ?1",
            [id.as_str()],
        )
        .map_err(|e| e.to_string())?
    };
    let request = request.ok_or_else(|| "solicitud actualizada no encontrada".to_string())?;
    Ok(json_response(200, &json!({ "request": request })))
}

/// `/api/media/local/{id}` con streaming por rangos nativo (nunca en
/// memoria): el `Range` se parsea como el regex de Node
/// (`/bytes=(\d*)-(\d*)/`, sin anclar), y el `content-length` es el
/// de Node (`end - start + 1`, sin recortar).
async fn media_local(state: &AppState, ctx: Ctx) -> RResult {
    let _user = match need_auth(state, &ctx.headers)? {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let id = ctx.path.split('/').nth(4).unwrap_or("");
    let song = {
        let db = lock_db(state);
        query_one(&db.conn, "SELECT * FROM songs WHERE id = ?1", [id]).map_err(|e| e.to_string())?
    };
    let Some(song) = song else {
        return Ok(err_json(404, "Vídeo local no encontrado."));
    };
    if song.get("platform").and_then(|v| v.as_str()) != Some("local") {
        return Ok(err_json(404, "Vídeo local no encontrado."));
    }
    let local_path = match song.get("local_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return Err("local_path no es texto (existsSync lanzaría en JS)".to_string()),
    };
    if !Path::new(&local_path).exists() {
        return Ok(err_json(
            404,
            "El archivo local ya no está en el disco del servidor.",
        ));
    }
    let size = tokio::fs::metadata(&local_path)
        .await
        .map_err(|e| e.to_string())?
        .len();
    let ext = Path::new(&local_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mime = match ext.as_str() {
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        _ => "video/mp4",
    };
    let range = ctx
        .headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let Some(range) = range else {
        let file = tokio::fs::File::open(&local_path)
            .await
            .map_err(|e| e.to_string())?;
        let stream = file_stream(file, size);
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, size)
            .body(Body::from_stream(stream))
            .map_err(|e| e.to_string());
    };
    let (start, end) = parse_range(&range, size);
    let len = end - start + 1.0;
    if len < 0.0 {
        // `writeHead` con content-length negativo lanza en Node →
        // 500 (el `catch` de `handle`).
        return Err(format!("rango inválido: bytes {start}-{end}"));
    }
    if len == 0.0 {
        // content-length 0 válido: Node escribe el 206 y el stream
        // lanza (start > end) con los headers ya enviados → cuerpo vacío.
        return Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, mime)
            .header(
                header::CONTENT_RANGE,
                format!(
                    "bytes {}-{}/{}",
                    karaoke_db::js::js_number_to_string(start),
                    karaoke_db::js::js_number_to_string(end),
                    size
                ),
            )
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, 0)
            .body(Body::empty())
            .map_err(|e| e.to_string());
    }
    let mut file = tokio::fs::File::open(&local_path)
        .await
        .map_err(|e| e.to_string())?;
    file.seek(std::io::SeekFrom::Start(start as u64))
        .await
        .map_err(|e| e.to_string())?;
    let take = (end.min(size as f64 - 1.0) - start + 1.0) as u64;
    let stream = file_stream(file, take);
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, mime)
        .header(
            header::CONTENT_RANGE,
            format!(
                "bytes {}-{}/{}",
                karaoke_db::js::js_number_to_string(start),
                karaoke_db::js::js_number_to_string(end),
                size
            ),
        )
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            header::CONTENT_LENGTH,
            karaoke_db::js::js_number_to_string(len),
        )
        .body(Body::from_stream(stream))
        .map_err(|e| e.to_string())
}

/// `/bytes=(\d*)-(\d*)/` sin anclar, como en Node. Sin coincidencia:
/// `(0, size-1)`. Los números se devuelven como `f64` (JS).
fn parse_range(header: &str, size: u64) -> (f64, f64) {
    let default = (0.0, size.saturating_sub(1) as f64);
    let Some(mut i) = header.find("bytes=").map(|x| x + "bytes=".len()) else {
        return default;
    };
    let bytes = header.as_bytes();
    let mut start_s = String::new();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        start_s.push(bytes[i] as char);
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'-' {
        return default;
    }
    i += 1;
    let mut end_s = String::new();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        end_s.push(bytes[i] as char);
        i += 1;
    }
    let start = if start_s.is_empty() {
        0.0
    } else {
        start_s.parse::<f64>().unwrap_or(0.0)
    };
    let end = if end_s.is_empty() {
        size.saturating_sub(1) as f64
    } else {
        end_s
            .parse::<f64>()
            .unwrap_or(size.saturating_sub(1) as f64)
    };
    (start, end)
}

const MIME: [(&str, &str); 7] = [
    (".html", "text/html; charset=utf-8"),
    (".js", "text/javascript; charset=utf-8"),
    (".css", "text/css; charset=utf-8"),
    (".svg", "image/svg+xml"),
    (".png", "image/png"),
    (".json", "application/json"),
    (".webmanifest", "application/manifest+json"),
];

/// `serveStatic` de `server/index.js`: `path.resolve(PUBLIC, rel)`
/// con guarda anti-traversal, 404 JSON si falta o es directorio, y
/// streaming del archivo (sin `content-length`, como Node).
async fn serve_static(state: &AppState, path: &str) -> RResult {
    let rel = if path == "/" {
        "index.html".to_string()
    } else {
        path.trim_start_matches('/').to_string()
    };
    let mut file = state.public_dir.clone();
    for seg in rel.split('/') {
        file.push(seg);
    }
    if file != state.public_dir && !file.starts_with(&state.public_dir) {
        return Ok(err_json(403, "Ruta no permitida"));
    }
    let meta = match tokio::fs::metadata(&file).await {
        Ok(m) => m,
        Err(_) => return Ok(err_json(404, "No encontrado")),
    };
    if meta.is_dir() {
        return Ok(err_json(404, "No encontrado"));
    }
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let mime = MIME
        .iter()
        .find(|(k, _)| *k == ext)
        .map(|(_, v)| *v)
        .unwrap_or("application/octet-stream");
    let f = tokio::fs::File::open(&file)
        .await
        .map_err(|e| e.to_string())?;
    let stream = file_stream(f, meta.len());
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .body(Body::from_stream(stream))
        .map_err(|e| e.to_string())
}

/// Stream de un archivo ya abierto: hasta `take` bytes en bloques de
/// 64 KiB. Nunca acumula el archivo en memoria.
fn file_stream(
    file: tokio::fs::File,
    take: u64,
) -> impl futures_util::stream::Stream<Item = Result<Bytes, std::io::Error>> {
    futures_util::stream::unfold(
        (file, take, vec![0u8; 65536]),
        |(mut file, mut left, mut buf)| async move {
            if left == 0 {
                return None;
            }
            let want = (buf.len() as u64).min(left) as usize;
            match file.read(&mut buf[..want]).await {
                Ok(0) => None,
                Ok(n) => {
                    left -= n as u64;
                    Some((Ok(Bytes::copy_from_slice(&buf[..n])), (file, left, buf)))
                }
                Err(e) => Some((Err(e), (file, 0, buf))),
            }
        },
    )
}
