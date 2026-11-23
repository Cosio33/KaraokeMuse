//! Tests de integración de `seed` con una BD válida.
//!
//! Los casos grabados (`cases.jsonl`) no cubren `seed` con BD válida
//! (89/89 son errores por no poder pasar un `DatabaseSync` vivo por
//! JSON — ADR-010), así que el camino feliz se prueba aquí: siembra
//! real, idempotencia y verificación de la contraseña con scrypt.

use std::path::PathBuf;

use karaoke_db::js::JsValue;
use karaoke_db::{SeedOpts, open_database, seed};

fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("seed-tmp")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn default_opts() -> SeedOpts {
    SeedOpts {
        admin_user: "admin".to_string(),
        admin_password: "AdminTest123!".to_string(),
        room_name: "Sala principal".to_string(),
        room_password: None,
    }
}

#[test]
fn seed_creates_admin_room_and_player_state() {
    let dir = temp_dir("basic");
    let mut db = open_database(&JsValue::Str(dir.to_string_lossy().into_owned())).unwrap();
    let out = seed(&mut db, &default_opts()).unwrap();
    assert!(out.admin_created);
    assert_eq!(out.room.name, "Sala principal");
    assert_eq!(out.room.code.len(), 6);
    assert!(
        out.room
            .code
            .chars()
            .all(|c| karaoke_db::RANDOM_CODE_CHARS.contains(c)),
        "código de sala con el alfabeto de randomCode"
    );
    assert_eq!(out.room.password_hash, None);
    assert_eq!(out.room.created_by, None);

    let users: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
        .unwrap();
    assert_eq!(users, 1);
    let (username, role, salt, hash): (String, String, String, String) = db
        .conn
        .query_row(
            "SELECT username, role, salt, password_hash FROM users WHERE role = 'admin'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(username, "admin");
    assert_eq!(role, "admin");
    assert!(karaoke_crypto::verify_password(
        "AdminTest123!",
        &salt,
        &hash
    ));
    assert!(!karaoke_crypto::verify_password(
        "AdminTest123?",
        &salt,
        &hash
    ));

    let rooms: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM rooms", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rooms, 1);
    let (room_id, current): (String, Option<String>) = db
        .conn
        .query_row(
            "SELECT room_id, current_song_id FROM player_state",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(room_id, out.room.id);
    assert_eq!(current, None);

    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn seed_is_idempotent() {
    let dir = temp_dir("idempotent");
    let mut db = open_database(&JsValue::Str(dir.to_string_lossy().into_owned())).unwrap();
    let first = seed(&mut db, &default_opts()).unwrap();
    assert!(first.admin_created);
    let second = seed(&mut db, &default_opts()).unwrap();
    assert!(!second.admin_created, "segunda siembra no crea admin");
    assert_eq!(second.room.id, first.room.id, "misma sala");
    assert_eq!(second.room.code, first.room.code);
    let users: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
        .unwrap();
    let rooms: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM rooms", [], |r| r.get(0))
        .unwrap();
    let states: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM player_state", [], |r| r.get(0))
        .unwrap();
    assert_eq!((users, rooms, states), (1, 1, 1));
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn seed_with_room_password() {
    let dir = temp_dir("room-pw");
    let mut db = open_database(&JsValue::Str(dir.to_string_lossy().into_owned())).unwrap();
    let opts = SeedOpts {
        room_password: Some("SalaSecreta99".to_string()),
        ..default_opts()
    };
    let out = seed(&mut db, &opts).unwrap();
    let (hash, salt): (Option<String>, Option<String>) = db
        .conn
        .query_row(
            "SELECT password_hash, salt FROM rooms WHERE id = ?1",
            [&out.room.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let (hash, salt) = (hash.unwrap(), salt.unwrap());
    assert!(karaoke_crypto::verify_password(
        "SalaSecreta99",
        &salt,
        &hash
    ));
    assert!(!karaoke_crypto::verify_password(
        "salasecreta99",
        &salt,
        &hash
    ));
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}
