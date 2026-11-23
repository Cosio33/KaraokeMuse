//! Apertura de la base SQLite y seed inicial: port de `openDatabase`
//! y `seed` de `server/db.js`.
//!
//! El esquema SQL es idéntico al original (mismo texto, mismas 7
//! tablas). `PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;`
//! se ejecuta en cada apertura, como en Node (SQLite no persiste el
//! `foreign_keys` por conexión).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::NodeError;
use crate::ids::{new_id_str, random_code};
use crate::js::{self, JsValue};
use crate::time;

/// Esquema exacto de `server/db.js` (copiado tal cual).
pub const DDL: &str = "
    CREATE TABLE IF NOT EXISTS users (
      id TEXT PRIMARY KEY,
      username TEXT UNIQUE NOT NULL,
      password_hash TEXT,
      salt TEXT,
      role TEXT NOT NULL CHECK (role IN ('admin','guest')),
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS rooms (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      code TEXT UNIQUE NOT NULL,
      password_hash TEXT,
      salt TEXT,
      created_by TEXT,
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS songs (
      id TEXT PRIMARY KEY,
      title TEXT NOT NULL,
      artist TEXT NOT NULL DEFAULT '',
      platform TEXT NOT NULL CHECK (platform IN ('youtube','spotify','local')),
      url TEXT NOT NULL DEFAULT '',
      local_path TEXT NOT NULL DEFAULT '',
      thumbnail_url TEXT NOT NULL DEFAULT '',
      genre TEXT NOT NULL DEFAULT '',
      language TEXT NOT NULL DEFAULT '',
      difficulty TEXT NOT NULL DEFAULT 'media' CHECK (difficulty IN ('facil','media','dificil')),
      status TEXT NOT NULL DEFAULT 'want' CHECK (status IN ('want','practicing','mastered')),
      rating INTEGER NOT NULL DEFAULT 0 CHECK (rating BETWEEN 0 AND 5),
      is_favorite INTEGER NOT NULL DEFAULT 0,
      notes TEXT NOT NULL DEFAULT '',
      lyrics_text TEXT NOT NULL DEFAULT '',
      owner_user_id TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS queue (
      id TEXT PRIMARY KEY,
      room_id TEXT NOT NULL,
      song_id TEXT NOT NULL,
      user_id TEXT NOT NULL,
      position INTEGER NOT NULL,
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS requests (
      id TEXT PRIMARY KEY,
      video_url TEXT NOT NULL,
      title TEXT NOT NULL,
      artist TEXT NOT NULL DEFAULT '',
      difficulty TEXT NOT NULL DEFAULT 'media' CHECK (difficulty IN ('facil','media','dificil')),
      language TEXT NOT NULL DEFAULT '',
      notes TEXT NOT NULL DEFAULT '',
      status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','in_progress','done')),
      requester_user_id TEXT,
      admin_note TEXT NOT NULL DEFAULT '',
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS sessions (
      token_hash TEXT PRIMARY KEY,
      user_id TEXT NOT NULL,
      expires_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS player_state (
      room_id TEXT PRIMARY KEY,
      current_song_id TEXT,
      updated_at TEXT NOT NULL
    );
  ";

/// Handle de la base de datos. Envuelve `rusqlite::Connection`
/// (`Send`, no `Sync`: en el servidor se comparte con un `Mutex`,
/// ver `rust/research/storage-auth.md`).
pub struct Db {
    /// Conexión SQLite. Pública para que `auth` y `routes` preparen
    /// sus propias consultas, como hacen con el `DatabaseSync` de Node.
    pub conn: Connection,
    /// Ruta del archivo `karaokemuse.sqlite`.
    pub path: PathBuf,
}

/// Límites SQLite tal como los expone el objeto `DatabaseSync` de
/// Node (`db.limits`): los defaults de SQLite, sin modificar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbLimits {
    pub length: i32,
    pub sql_length: i32,
    pub column: i32,
    pub expr_depth: i32,
    pub compound_select: i32,
    pub vdbe_op: i32,
    pub function_arg: i32,
    pub attach: i32,
    pub like_pattern_length: i32,
    pub variable_number: i32,
    pub trigger_depth: i32,
}

impl Db {
    /// Igual que `db.isOpen` en Node: un `Db` solo existe abierto.
    pub fn is_open(&self) -> bool {
        true
    }

    /// Igual que `db.isTransaction` en Node (rusqlite no expone
    /// `is_transaction`: es el inverso de `is_autocommit`).
    pub fn is_transaction(&self) -> bool {
        !self.conn.is_autocommit()
    }

    /// Límites actuales (defaults de SQLite) para compararlos con el
    /// objeto grabado de `openDatabase`.
    pub fn limits(&self) -> Result<DbLimits, NodeError> {
        use rusqlite::limits::Limit;
        Ok(DbLimits {
            length: self.conn.limit(Limit::SQLITE_LIMIT_LENGTH)?,
            sql_length: self.conn.limit(Limit::SQLITE_LIMIT_SQL_LENGTH)?,
            column: self.conn.limit(Limit::SQLITE_LIMIT_COLUMN)?,
            expr_depth: self.conn.limit(Limit::SQLITE_LIMIT_EXPR_DEPTH)?,
            compound_select: self.conn.limit(Limit::SQLITE_LIMIT_COMPOUND_SELECT)?,
            vdbe_op: self.conn.limit(Limit::SQLITE_LIMIT_VDBE_OP)?,
            function_arg: self.conn.limit(Limit::SQLITE_LIMIT_FUNCTION_ARG)?,
            attach: self.conn.limit(Limit::SQLITE_LIMIT_ATTACHED)?,
            like_pattern_length: self.conn.limit(Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH)?,
            variable_number: self.conn.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER)?,
            trigger_depth: self.conn.limit(Limit::SQLITE_LIMIT_TRIGGER_DEPTH)?,
        })
    }

    /// Nombres de las tablas presentes (para verificar el esquema).
    pub fn table_names(&self) -> Result<Vec<String>, NodeError> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row?);
        }
        Ok(names)
    }
}

/// `openDatabase(dataDir)`: crea el directorio si falta
/// (`mkdirSync` recursivo), abre `karaokemuse.sqlite`, activa WAL +
/// foreign keys y crea las 7 tablas si faltan. `dataDir` no string →
/// el `ERR_INVALID_ARG_TYPE` de `fs.mkdirSync` (el primer chequeo
/// que dispara Node).
pub fn open_database(data_dir: &JsValue) -> Result<Db, NodeError> {
    let dir = match data_dir {
        JsValue::Str(s) => s.clone(),
        other => {
            return Err(NodeError::Message(js::invalid_arg_type(
                "path",
                "string or an instance of Buffer or URL",
                other,
            )));
        }
    };
    if dir.is_empty() {
        return Err(NodeError::Message(
            "ENOENT: no such file or directory, mkdir ''".to_string(),
        ));
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Err(NodeError::Message(mkdir_error_message(&e, &dir)));
    }
    let path = Path::new(&dir).join("karaokemuse.sqlite");
    let conn = Connection::open(&path).map_err(NodeError::Sql)?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
    conn.execute_batch(DDL)?;
    // Migración ADR-020: `player_state.previous_song_id` (botón
    // Anterior del Player). Idempotente: solo se añade si la columna
    // no existe. El servidor Node ignora la columna extra.
    let has_prev: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('player_state') WHERE name = 'previous_song_id'",
        [],
        |r| r.get(0),
    )?;
    if has_prev == 0 {
        conn.execute_batch("ALTER TABLE player_state ADD COLUMN previous_song_id TEXT;")?;
    }
    Ok(Db { conn, path })
}

/// Mensaje de error de Node `fs.mkdirSync` (`{CODE}: {desc}, mkdir
/// '{path}'`) a partir de un `io::Error`.
fn mkdir_error_message(e: &std::io::Error, dir: &str) -> String {
    use std::io::ErrorKind;
    let (code, desc) = match e.kind() {
        ErrorKind::AlreadyExists => ("EEXIST", "file already exists"),
        ErrorKind::NotFound => ("ENOENT", "no such file or directory"),
        ErrorKind::PermissionDenied => ("EACCES", "permission denied"),
        ErrorKind::NotADirectory => ("ENOTDIR", "not a directory"),
        _ => return e.to_string(),
    };
    format!("{code}: {desc}, mkdir '{dir}'")
}

/// Fila de `rooms`, tal como la devuelve `seed` (el `SELECT *`
/// completo de Node).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub code: String,
    pub password_hash: Option<String>,
    pub salt: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
}

fn row_to_room(row: &rusqlite::Row<'_>) -> rusqlite::Result<Room> {
    Ok(Room {
        id: row.get("id")?,
        name: row.get("name")?,
        code: row.get("code")?,
        password_hash: row.get("password_hash")?,
        salt: row.get("salt")?,
        created_by: row.get("created_by")?,
        created_at: row.get("created_at")?,
    })
}

/// Opciones de `seed`, las mismas que `server/index.js` pasa desde
/// las variables de entorno (`KARAOKE_ADMIN_USER`, etc.).
pub struct SeedOpts {
    pub admin_user: String,
    pub admin_password: String,
    pub room_name: String,
    pub room_password: Option<String>,
}

/// Resultado de `seed`: `{ adminCreated, room }`, igual que en JS.
pub struct SeedOutcome {
    pub admin_created: bool,
    pub room: Room,
}

/// `seed(db, opts)`: crea el admin y la sala por defecto si no
/// existen, y asegura la fila de `player_state`. Idempotente: la
/// segunda llamada no crea nada (`adminCreated: false`, misma sala).
pub fn seed(db: &mut Db, opts: &SeedOpts) -> Result<SeedOutcome, NodeError> {
    let now = time::now_iso();
    let mut admin_created = false;
    let admin: Option<String> = db
        .conn
        .query_row("SELECT id FROM users WHERE role = ?1", ["admin"], |row| {
            row.get(0)
        })
        .optional()?;
    if admin.is_none() {
        let (salt, hash) = karaoke_crypto::hash_password(&opts.admin_password)?;
        let user_id = new_id_str("u")?;
        db.conn.execute(
            "INSERT INTO users (id, username, password_hash, salt, role, created_at) VALUES (?1, ?2, ?3, ?4, 'admin', ?5)",
            params![user_id, opts.admin_user, hash, salt, now],
        )?;
        admin_created = true;
    }
    let room: Option<Room> = db
        .conn
        .query_row(
            "SELECT * FROM rooms ORDER BY created_at LIMIT 1",
            [],
            row_to_room,
        )
        .optional()?;
    let room = match room {
        Some(room) => room,
        None => {
            let id = new_id_str("room")?;
            let code = random_code(Some(&JsValue::Num(6.0)))?;
            let (password_hash, salt) = match &opts.room_password {
                Some(pw) => {
                    let (salt, hash) = karaoke_crypto::hash_password(pw)?;
                    (Some(hash), Some(salt))
                }
                None => (None, None),
            };
            db.conn.execute(
                "INSERT INTO rooms (id, name, code, password_hash, salt, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                params![id, opts.room_name, code, password_hash, salt, now],
            )?;
            db.conn
                .query_row("SELECT * FROM rooms WHERE id = ?1", [&id], row_to_room)?
        }
    };
    db.conn.execute(
        "INSERT OR IGNORE INTO player_state (room_id, current_song_id, updated_at) VALUES (?1, NULL, ?2)",
        params![room.id, now],
    )?;
    Ok(SeedOutcome {
        admin_created,
        room,
    })
}

/// Envoltorio JSON de `seed` solo para reproducir los casos grabados:
/// JSON no puede expresar un `DatabaseSync` vivo, así que cualquier
/// primer argumento JSON da el mismo error que Node daría al llamar
/// `db.prepare` sobre ese valor: `null` →
/// `Cannot read properties of null (reading 'prepare')`;
/// `undefined` → `... of undefined ...`; cualquier otro valor →
/// `db.prepare is not a function`. El `seed` real es `seed(&mut Db,
/// &SeedOpts)` y se prueba en `tests/seed.rs`.
pub fn seed_from_json(db_arg: &JsValue, _opts_arg: &JsValue) -> Result<SeedOutcome, NodeError> {
    match db_arg {
        JsValue::Null => Err(NodeError::Message(
            "Cannot read properties of null (reading 'prepare')".to_string(),
        )),
        JsValue::Undefined => Err(NodeError::Message(
            "Cannot read properties of undefined (reading 'prepare')".to_string(),
        )),
        _ => Err(NodeError::Message(
            "db.prepare is not a function".to_string(),
        )),
    }
}
