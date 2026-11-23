//! `karaoke-db` — persistencia SQLite de KaraokeMuse.
//!
//! Port de `server/db.js` (ADR-010: crate de librería Rust plano, no
//! puente napi — ver `rust/DECISIONS.md`).
//!
//! - [`ids`]: `random_code` / `new_id`.
//! - [`js`]: semántica de valores y mensajes de Node (ADR-011).
//! - [`store`]: `open_database`, `seed`, handle [`store::Db`].
//! - [`record`]: siembra determinista de la BD de pruebas
//!   (`reset_record_data`, espejo de `seed-record-db.mjs`, ADR-018).
//! - [`time`]: fechas ISO 8601 UTC como `Date.toISOString()`.
//!
//! Las primitivas scrypt/sha256 viven en `karaoke-crypto` (ADR-012).

pub mod ids;
pub mod js;
pub mod record;
pub mod store;
pub mod time;

pub use ids::{RANDOM_CODE_CHARS, new_id, new_id_str, random_code};
pub use js::JsValue;
pub use record::reset_record_data;
pub use store::{Db, DbLimits, Room, SeedOpts, SeedOutcome, open_database, seed, seed_from_json};

/// Error del port. La variante `Message` guarda el mensaje exacto de
/// Node (p. ej. `ERR_INVALID_ARG_TYPE` / `ERR_OUT_OF_RANGE` /
/// `TypeError`) para compararlo byte a byte en el replay.
#[derive(Debug)]
pub enum NodeError {
    Message(String),
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Crypto(karaoke_crypto::CryptoError),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Message(m) => f.write_str(m),
            NodeError::Io(e) => write!(f, "{e}"),
            NodeError::Sql(e) => write!(f, "{e}"),
            NodeError::Crypto(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for NodeError {}

impl From<std::io::Error> for NodeError {
    fn from(e: std::io::Error) -> Self {
        NodeError::Io(e)
    }
}

impl From<rusqlite::Error> for NodeError {
    fn from(e: rusqlite::Error) -> Self {
        NodeError::Sql(e)
    }
}

impl From<karaoke_crypto::CryptoError> for NodeError {
    fn from(e: karaoke_crypto::CryptoError) -> Self {
        NodeError::Crypto(e)
    }
}
