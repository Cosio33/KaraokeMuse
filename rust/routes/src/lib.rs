//! `server/routes` como crate lib plana `karaoke-routes`
//! (ADR-014): el `handle` de `server/index.js` no exporta funciones
//! (cierra sobre `db` y `room` y toma `(req, res)`), así que no hay
//! puente napi; el crate expone el dispatch completo y el front door
//! lo enlaza a mano con `app wire --skip-gate`.
//!
//! El estado vive en `AppState`: `Db` compartido (mutex), la sala
//! sembrada, el directorio público y un cliente reqwest (sin proxy,
//! como el `fetch` de Node).

pub mod handlers;
pub mod http_util;
pub mod json;
pub mod metadata;
pub mod port;
pub mod validate;

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use axum::response::Response;

use karaoke_db::Room;
use karaoke_db::store::{Db, SeedOpts, open_database, seed};

use handlers::{Ctx, handle};

pub struct AppState {
    pub db: Mutex<Db>,
    pub room: Room,
    pub public_dir: PathBuf,
    pub client: reqwest::Client,
}

static STATE: OnceLock<AppState> = OnceLock::new();

impl AppState {
    /// Abre la BD en `data_dir` y la siembra con las `KARAOKE_*`,
    /// igual que `index.js`. Idempotente: la primera llamada gana
    /// (OnceLock); las siguientes devuelven el mismo estado.
    pub fn init(data_dir: &str, public_dir: PathBuf) -> Result<&'static AppState, String> {
        if let Some(s) = STATE.get() {
            return Ok(s);
        }
        let mut db = open_database(&karaoke_db::js::JsValue::Str(data_dir.to_string()))
            .map_err(|e| e.to_string())?;
        let opts = SeedOpts {
            admin_user: std::env::var("KARAOKE_ADMIN_USER").unwrap_or_else(|_| "admin".to_string()),
            admin_password: std::env::var("KARAOKE_ADMIN_PASSWORD")
                .unwrap_or_else(|_| "admin123".to_string()),
            room_name: std::env::var("KARAOKE_ROOM_NAME")
                .unwrap_or_else(|_| "Sala principal".to_string()),
            room_password: std::env::var("KARAOKE_ROOM_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty()),
        };
        let room = seed(&mut db, &opts).map_err(|e| e.to_string())?.room;
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        let state = AppState {
            db: Mutex::new(db),
            room,
            public_dir,
            client,
        };
        let _ = STATE.set(state);
        Ok(STATE.get().unwrap())
    }

    /// El estado ya inicializado, si `init` se llamó antes.
    pub fn state() -> Option<&'static AppState> {
        STATE.get()
    }
}

/// Dispatch completo: convierte la `Request` de axum en el `Ctx` que
/// espera `handlers::handle` (método + pathname normalizado estilo
/// WHATWG URL + query cruda + headers + cuerpo).
///
/// Errores del `handle` de Node (cuerpo inválido, SQL, IO) → 500
/// `{error:'Error interno del servidor.'}`; los errores de dispatch
/// (URL inválida) también caen aquí, como el `catch` de `handle`.
pub async fn dispatch(state: &AppState, req: axum::extract::Request) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let (parts, body) = req.into_parts();
    let headers = parts.headers.clone();

    let raw_path = uri.path();
    let raw_query = uri.query().unwrap_or("").to_string();
    let path = http_util::normalize_pathname(raw_path);

    let ctx = Ctx {
        method: method.clone(),
        path,
        query: raw_query,
        headers,
        body,
    };
    match handle(state, ctx).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[karaoke-routes] {method} {raw_path}: {e}");
            handlers::json_response(
                500,
                &serde_json::json!({ "error": "Error interno del servidor." }),
            )
        }
    }
}

/// Handler de axum para `serve` (solo pruebas locales): el estado
/// llega como `State` y la request completa se pasa a `dispatch`.
#[axum::debug_handler]
async fn handler_fn(
    axum::extract::State(state): axum::extract::State<&'static AppState>,
    req: axum::extract::Request,
) -> Response {
    dispatch(state, req).await
}

/// Entry point para pruebas locales del crate (no se usa en el
/// front door): levanta el servidor Rust puro en `PORT`/`HOST`.
pub async fn serve(public_dir: PathBuf) -> Result<(), String> {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let state = AppState::init(&data_dir, public_dir)?;
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| format!("dirección inválida: {e}"))?;
    let app = axum::Router::new()
        .route("/", axum::routing::any(handler_fn))
        .route("/{*rest}", axum::routing::any(handler_fn))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| e.to_string())?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}
