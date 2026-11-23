//! Tests de integración de `reset_record_data` (ADR-018).
//!
//! Verifica que la siembra determinista en Rust produce exactamente
//! el mismo fixture que `rust/seed-record-db.mjs`: sala KMTEST, admin
//! con su contraseña, invitado, sesiones de tokens conocidos, 3
//! canciones, cola, solicitudes y el vídeo de 42 bytes. Y que la
//! guarda se niega a borrar un directorio que no se llame
//! `record-data`.

use std::path::PathBuf;

use karaoke_db::js::JsValue;
use karaoke_db::record::{ADMIN_TOKEN, FAR_FUTURE, FIXED_TS, GUEST_TOKEN, VIDEO_LOCAL_PATH};
use karaoke_db::{open_database, reset_record_data};

fn temp_record_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("record-tmp-{}-{name}", std::process::id()))
        .join("record-data")
}

fn count(db: &karaoke_db::Db, table: &str) -> i64 {
    db.conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn reset_creates_the_exact_record_fixture() {
    let dir = temp_record_dir("fixture");
    reset_record_data(&dir).unwrap();

    // El vídeo de pruebas existe y mide 42 bytes.
    let video = std::fs::read(dir.join("test-video.mp4")).unwrap();
    assert_eq!(video.len(), 42);
    assert_eq!(&video[..10], b"fake-mp4-c");

    let db = open_database(&JsValue::Str(dir.to_string_lossy().into_owned())).unwrap();

    // Sala de código e id fijos.
    let (id, code, name): (String, String, String) = db
        .conn
        .query_row("SELECT id, code, name FROM rooms", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(id, "room_seed001");
    assert_eq!(code, "KMTEST");
    assert_eq!(name, "Sala principal");
    assert_eq!(count(&db, "rooms"), 1);

    // Admin con su contraseña (scrypt con sal como cadena, ADR-015).
    let (salt, hash): (String, String) = db
        .conn
        .query_row(
            "SELECT salt, password_hash FROM users WHERE role = 'admin'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
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

    // Invitado sin contraseña.
    let guest: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM users WHERE id = 'u_guest001' AND role = 'guest' AND password_hash IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(guest, 1);

    // Sesiones de tokens conocidos, sin caducar.
    for (token, user) in [(ADMIN_TOKEN, "admin"), (GUEST_TOKEN, "u_guest001")] {
        let hash = karaoke_crypto::token_hash(token);
        let (user_id, expires): (String, String) = db
            .conn
            .query_row(
                "SELECT user_id, expires_at FROM sessions WHERE token_hash = ?1",
                [&hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(expires, FAR_FUTURE);
        if user == "admin" {
            let admin_id: String = db
                .conn
                .query_row("SELECT id FROM users WHERE role = 'admin'", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(user_id, admin_id);
        } else {
            assert_eq!(user_id, "u_guest001");
        }
    }
    assert_eq!(count(&db, "sessions"), 2);

    // 3 canciones, con el vídeo local apuntando al fixture.
    assert_eq!(count(&db, "songs"), 3);
    let local_path: String = db
        .conn
        .query_row(
            "SELECT local_path FROM songs WHERE id = 'song_seed003'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(local_path, VIDEO_LOCAL_PATH);
    let titles: Vec<String> = {
        let mut stmt = db
            .conn
            .prepare("SELECT title FROM songs ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(titles, ["Canción Admin", "Canción Invitado", "Vídeo Local"]);

    // Cola y solicitudes.
    assert_eq!(count(&db, "queue"), 2);
    assert_eq!(count(&db, "requests"), 2);
    let req_titles: Vec<String> = {
        let mut stmt = db
            .conn
            .prepare("SELECT title FROM requests ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(req_titles, ["Petición Invitado", "Petición Admin"]);

    // player_state apunta a la sala fija.
    let room_id: String = db
        .conn
        .query_row("SELECT room_id FROM player_state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(room_id, "room_seed001");

    // created_at fijo en las filas sembradas.
    let created: String = db
        .conn
        .query_row(
            "SELECT created_at FROM songs WHERE id = 'song_seed001'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(created, FIXED_TS);
}

#[test]
fn reset_is_idempotent() {
    let dir = temp_record_dir("idempotent");
    reset_record_data(&dir).unwrap();
    reset_record_data(&dir).unwrap();
    let db = open_database(&JsValue::Str(dir.to_string_lossy().into_owned())).unwrap();
    assert_eq!(count(&db, "songs"), 3);
    assert_eq!(count(&db, "rooms"), 1);
}

#[test]
fn reset_refuses_any_dir_not_named_record_data() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("record-guard-{}", std::process::id()))
        .join("data");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("marker.txt"), "no borrar").unwrap();
    let err = reset_record_data(&dir).unwrap_err();
    assert!(err.contains("record-data"), "{err}");
    // El directorio y su contenido sobreviven intactos.
    assert_eq!(
        std::fs::read_to_string(dir.join("marker.txt")).unwrap(),
        "no borrar"
    );
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}
