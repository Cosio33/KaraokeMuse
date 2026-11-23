//! `karaoke-auth` — autenticación de KaraokeMuse.
//!
//! Port de `server/auth.js` (ADR-013: crate de librería Rust plano,
//! sin puente napi — las funciones de sesión toman un `DatabaseSync`
//! vivo que no se puede grabar por JSON).
//!
//! Las primitivas puras (`hash_password`, `verify_password`) se
//! re-exportan desde `karaoke-crypto` (ADR-012); `token_hash` también
//! se re-exporta aunque en JS sea privada, porque las rutas la
//! necesitan para depurar sesiones. Las sesiones se implementan sobre
//! `karaoke-db::Db`.

use karaoke_db::NodeError;
use karaoke_db::store::Db;
use rusqlite::{OptionalExtension, params};

pub use karaoke_crypto::{hash_password, token_hash, verify_password};

/// Fila de usuario tal como la devuelve `getUserFromToken`:
/// `SELECT id, username, role, created_at FROM users WHERE id = ?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub role: String,
    pub created_at: String,
}

/// Usuario público: lo que `publicUser` devuelve y lo que la API
/// expone (`{ id, username, role }`, sin `created_at`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicUser {
    pub id: String,
    pub username: String,
    pub role: String,
}

/// `publicUser(user)`: `None` si el usuario es `None`.
pub fn public_user(user: Option<&UserRow>) -> Option<PublicUser> {
    user.map(|u| PublicUser {
        id: u.id.clone(),
        username: u.username.clone(),
        role: u.role.clone(),
    })
}

/// Sesión creada: `{ token, expiresAt }`, igual que en JS. El token
/// es el valor que se entrega al cliente; en BD solo se guarda su
/// sha256 (`token_hash`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub token: String,
    pub expires_at: String,
}

/// `createSession(db, userId, days = 7)`: token aleatorio de 32 bytes
/// en hex, expiración `now + days` en ISO. `days` es 7 por defecto
/// en JS; aquí se pasa explícito (las rutas usan 7).
pub fn create_session(db: &mut Db, user_id: &str, days: i64) -> Result<Session, NodeError> {
    let mut buf = [0u8; 32];
    karaoke_crypto::fill_random(&mut buf)?;
    let token = hex::encode(buf);
    let expires_at = karaoke_db::time::now_plus_days_iso(days);
    db.conn.execute(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)",
        params![token_hash(&token), user_id, expires_at],
    )?;
    Ok(Session { token, expires_at })
}

/// `getUserFromToken(db, token)`: `None` si el token es vacío o
/// nulo, no existe, o expiró. Si expiró, borra la sesión (como en
/// JS) y devuelve `None`. Si es válida, devuelve la fila del usuario.
pub fn get_user_from_token(db: &mut Db, token: Option<&str>) -> Result<Option<UserRow>, NodeError> {
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    let hash = token_hash(token);
    let row: Option<(String, String)> = db
        .conn
        .query_row(
            "SELECT user_id, expires_at FROM sessions WHERE token_hash = ?1",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((user_id, expires_at)) = row else {
        return Ok(None);
    };
    // `new Date(expires_at) < new Date()` en JS. Si `expires_at` no
    // parsea, JS da `Invalid Date` (NaN) y `NaN < now` es `false`:
    // la sesión NO se considera expirada.
    let expired = match karaoke_db::time::iso_to_unix_millis(&expires_at) {
        Some(exp) => exp < karaoke_db::time::now_unix_millis(),
        None => false,
    };
    if expired {
        db.conn
            .execute("DELETE FROM sessions WHERE token_hash = ?1", [&hash])?;
        return Ok(None);
    }
    let user: Option<UserRow> = db
        .conn
        .query_row(
            "SELECT id, username, role, created_at FROM users WHERE id = ?1",
            [&user_id],
            |r| {
                Ok(UserRow {
                    id: r.get(0)?,
                    username: r.get(1)?,
                    role: r.get(2)?,
                    created_at: r.get(3)?,
                })
            },
        )
        .optional()?;
    Ok(user)
}

/// `destroySession(db, token)`: sin token (vacío o nulo) no hace
/// nada; si no, borra la sesión por su hash.
pub fn destroy_session(db: &mut Db, token: Option<&str>) -> Result<(), NodeError> {
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return Ok(());
    };
    db.conn.execute(
        "DELETE FROM sessions WHERE token_hash = ?1",
        [token_hash(token)],
    )?;
    Ok(())
}
