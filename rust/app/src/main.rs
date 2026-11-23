//! Binario puro `karaokemuse` (ADR-018): el servidor HTTP completo
//! en Rust. No arranca el servidor Node ni proxya nada: todas las
//! rutas se responden con `karaoke-routes`.
//!
//! - `KARAOKE_RESET_RECORD_DB=1`: antes de servir, borra y vuelve a
//!   sembrar la BD de pruebas (`rust/record-data`) con el fixture
//!   determinista de `karaoke_db::reset_record_data`. Solo para
//!   replay/lift; en producción no se fija y el binario abre la BD de
//!   `DATA_DIR` (o `data/`) como siempre, sin tocarla.
//! - `RUST_ANYTHING_FALLBACK_LOG`: `app replay` cuenta los fallbacks
//!   por el archivo que señala esta variable; crearlo vacío dice
//!   "0 fallbacks" (el binario no tiene a dónde caer: todo es Rust).

mod units;

// Unidades portadas (ADR-010/ADR-013): enlazadas para que sean parte
// del build de la app.
use karaoke_auth as _;
use karaoke_db as _;

use std::net::SocketAddr;
use std::path::Path;

/// Raíz del repo (el `ROOT` de `server/index.js`).
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// `DATA_DIR` si está fijada, si no `<ROOT>/data`, como `index.js`.
/// Espejo de `units::karaoke_routes::data_dir`: aquí hace falta antes
/// de servir, para el reseteo de la BD de pruebas.
fn data_dir() -> String {
    std::env::var("DATA_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{ROOT}/data"))
}

/// Ctrl-C o SIGTERM: dejar de aceptar requests y terminar las abiertas.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("a SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[tokio::main]
async fn main() {
    // Reporte de fallbacks para `app replay`: crear el archivo vacío
    // (sin escribir nada) equivale a "0 fallbacks".
    if let Some(path) = std::env::var_os("RUST_ANYTHING_FALLBACK_LOG") {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path);
    }
    // Reseteo de la BD de pruebas, solo cuando se pide explícitamente.
    if std::env::var_os("KARAOKE_RESET_RECORD_DB").is_some_and(|v| !v.is_empty()) {
        let dir = data_dir();
        if let Err(e) = karaoke_db::reset_record_data(Path::new(&dir)) {
            eprintln!("[karaokemuse] no se pudo resetear la BD de pruebas en {dir}: {e}");
            std::process::exit(1);
        }
    }
    let app = units::router();
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    // HOST como el servidor Node (por defecto 0.0.0.0); un valor
    // inválido cae al default, igual que antes.
    let host: std::net::IpAddr = std::env::var("HOST")
        .ok()
        .filter(|h| !h.is_empty())
        .and_then(|h| h.parse().ok())
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(host, port))
        .await
        .expect("binding the port");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("serving");
}
