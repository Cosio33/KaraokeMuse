//! Binario de pruebas de `karaoke-routes`: servidor Rust puro
//! (sin proxy al original) para `cargo test` y depuración local.
//!
//! Uso: `cargo run -p karaoke-routes -- DATA_DIR=/tmp/km DATA_DIR_PUBLIC=public`
//! (o `DATA_DIR` y `PUBLIC_DIR` por entorno).

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let public = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "public".to_string());
    if let Err(e) = karaoke_routes::serve(PathBuf::from(public)).await {
        eprintln!("karaoke-routes: {e}");
        std::process::exit(1);
    }
}
