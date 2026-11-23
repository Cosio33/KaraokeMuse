//! Glue from the front door to `karaoke-routes`: read the request, call `karaoke_routes::dispatch`.
//!
//! El estado (`AppState`) se inicializa perezosamente en la primera
//! request: para entonces `main.rs` ya reseteó y sembró la BD de
//! pruebas si `KARAOKE_RESET_RECORD_DB` está fijada (replay/lift), y
//! `seed()` es idempotente (la sala existente, código KMTEST, se
//! conserva). En producción la BD ya existe y no se resetea. Si la BD
//! no abre, la request da 500 en vez de tumbar el arranque
//! (desviación respecto a Node, que falla al arrancar; ver MAP.md).
//!
//! `route` es la ruta cableada y `params` sus parámetros de path; el
//! dispatch re-deriva método, path y query de la request, así que se
//! ignoran (las rutas se cablearon sin método porque Node da 404, no
//! 405, con método incorrecto).

use std::collections::HashMap;
use std::path::PathBuf;

use axum::extract::Request;
use axum::response::Response;

/// Raíz en tiempo de compilación (último recurso): el `ROOT` de
/// `server/index.js`. NUNCA usarla directamente: en el binario
/// publicado apunta a la ruta del servidor de CI, que no existe en
/// la máquina del usuario (ADR-019).
const COMPILE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Carpeta que contiene el ejecutable, si se puede saber.
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.to_path_buf())
}

/// Raíz del proyecto en tiempo de ejecución: la primera carpeta que
/// contenga `public/`, buscando junto al ejecutable, en la carpeta
/// actual, y por último en la raíz de compilación. Así el binario
/// suelto funciona con `public/` a su lado, sin variables de entorno.
fn runtime_root() -> PathBuf {
    for b in [exe_dir(), std::env::current_dir().ok()]
        .into_iter()
        .flatten()
    {
        if b.join("public").is_dir() {
            return b;
        }
    }
    PathBuf::from(COMPILE_ROOT)
}

/// `DATA_DIR` si está fijada, si no `<raíz>/data`, como `index.js`
/// (`path.join(ROOT, 'data')`).
fn data_dir() -> String {
    std::env::var("DATA_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| runtime_root().join("data").to_string_lossy().into_owned())
}

/// `<raíz>/public`, como `index.js`, salvo `KARAOKE_PUBLIC_DIR`
/// (solo para replay/lift: el binario del stage del lift se compila
/// con `ROOT` temporal y sin `public/`; con la variable se sirve el
/// `public/` real. Ver ADR-018).
fn public_dir() -> PathBuf {
    if let Ok(d) = std::env::var("KARAOKE_PUBLIC_DIR")
        && !d.is_empty()
    {
        return PathBuf::from(d);
    }
    runtime_root().join("public")
}

/// Inicializa el estado de `karaoke-routes` (BD + sala + cliente
/// oEmbed). Idempotente: la primera llamada gana (OnceLock).
pub fn init() -> Result<&'static karaoke_routes::AppState, String> {
    karaoke_routes::AppState::init(&data_dir(), public_dir())
}

/// `route` is the route as wired (`GET /users/{id}`, or `*`), `params` its path parameters.
pub async fn handle(_route: &str, _params: HashMap<String, String>, req: Request) -> Response {
    let state = match init() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[karaoke-routes] init: {e}");
            return karaoke_routes::handlers::json_response(
                500,
                &serde_json::json!({ "error": "Error interno del servidor." }),
            );
        }
    };
    karaoke_routes::dispatch(state, req).await
}
