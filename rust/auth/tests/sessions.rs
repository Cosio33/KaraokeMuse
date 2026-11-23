//! Tests de integración de las sesiones (`createSession`,
//! `getUserFromToken`, `destroySession`) con una BD temporal real.
//! Los casos grabados (`auth-cases.jsonl`) no pueden cubrirlas: toman
//! un `DatabaseSync` vivo (ADR-013).

use std::path::PathBuf;

use karaoke_auth::{create_session, destroy_session, get_user_from_token, token_hash};
use karaoke_db::js::JsValue;
use karaoke_db::{SeedOpts, open_database, seed};
use rusqlite::params;

fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("sessions-tmp")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn seeded_db(name: &str) -> (karaoke_db::Db, String, PathBuf) {
    let dir = temp_dir(name);
    let mut db = open_database(&JsValue::Str(dir.to_string_lossy().into_owned())).unwrap();
    seed(
        &mut db,
        &SeedOpts {
            admin_user: "admin".to_string(),
            admin_password: "AdminTest123!".to_string(),
            room_name: "Sala principal".to_string(),
            room_password: None,
        },
    )
    .unwrap();
    let admin_id: String = db
        .conn
        .query_row("SELECT id FROM users WHERE role = 'admin'", [], |r| {
            r.get(0)
        })
        .unwrap();
    (db, admin_id, dir)
}

#[test]
fn create_and_get_session() {
    let (mut db, admin_id, dir) = seeded_db("create-get");
    let sess = create_session(&mut db, &admin_id, 7).unwrap();
    assert_eq!(sess.token.len(), 64, "token: 32 bytes en hex");
    assert!(sess.token.chars().all(|c| c.is_ascii_hexdigit()));
    // expiresAt ≈ ahora + 7 días (tolerancia 120 s).
    let exp = karaoke_db::time::iso_to_unix_millis(&sess.expires_at).unwrap();
    let now = karaoke_db::time::now_unix_millis();
    assert!(
        (exp - now - 7 * 24 * 60 * 60 * 1000).abs() < 120_000,
        "expiresAt {} no es ahora+7d",
        sess.expires_at
    );
    // En BD solo se guarda el sha256 del token, nunca el token.
    let (stored_hash, stored_user): (String, String) = db
        .conn
        .query_row("SELECT token_hash, user_id FROM sessions", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(stored_hash, token_hash(&sess.token));
    assert_ne!(stored_hash, sess.token);
    assert_eq!(stored_user, admin_id);

    let user = get_user_from_token(&mut db, Some(&sess.token))
        .unwrap()
        .expect("sesión válida devuelve el usuario");
    assert_eq!(user.username, "admin");
    assert_eq!(user.role, "admin");
    assert_eq!(user.id, admin_id);
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn get_unknown_empty_and_null_token() {
    let (mut db, _admin_id, dir) = seeded_db("unknown");
    assert!(get_user_from_token(&mut db, None).unwrap().is_none());
    assert!(get_user_from_token(&mut db, Some("")).unwrap().is_none());
    assert!(
        get_user_from_token(&mut db, Some(&"00".repeat(32)))
            .unwrap()
            .is_none()
    );
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn destroy_session_removes_it() {
    let (mut db, admin_id, dir) = seeded_db("destroy");
    let sess = create_session(&mut db, &admin_id, 7).unwrap();
    assert!(
        get_user_from_token(&mut db, Some(&sess.token))
            .unwrap()
            .is_some()
    );
    destroy_session(&mut db, Some(&sess.token)).unwrap();
    assert!(
        get_user_from_token(&mut db, Some(&sess.token))
            .unwrap()
            .is_none()
    );
    let count: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    // Destruir sin token no falla y no toca nada.
    destroy_session(&mut db, None).unwrap();
    destroy_session(&mut db, Some("")).unwrap();
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn expired_session_is_deleted_on_lookup() {
    let (mut db, admin_id, dir) = seeded_db("expired");
    let token = "ff".repeat(32);
    db.conn
        .execute(
            "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, '2020-01-01T00:00:00.000Z')",
            params![token_hash(&token), admin_id],
        )
        .unwrap();
    assert!(
        get_user_from_token(&mut db, Some(&token))
            .unwrap()
            .is_none(),
        "sesión expirada no devuelve usuario"
    );
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE token_hash = ?1",
            [token_hash(&token)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "la sesión expirada se borra al consultarla");
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unparseable_expiry_is_not_expired() {
    // JS: new Date("no-fecha") es Invalid Date; NaN < now es false,
    // así que la sesión NO se considera expirada.
    let (mut db, admin_id, dir) = seeded_db("bad-date");
    let token = "ee".repeat(32);
    db.conn
        .execute(
            "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, 'no-fecha')",
            params![token_hash(&token), admin_id],
        )
        .unwrap();
    let user = get_user_from_token(&mut db, Some(&token)).unwrap();
    assert!(user.is_some(), "fecha inválida no expira la sesión");
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn session_for_deleted_user_returns_none() {
    let (mut db, _admin_id, dir) = seeded_db("ghost");
    let sess = create_session(&mut db, "u_noexiste", 7).unwrap();
    assert!(
        get_user_from_token(&mut db, Some(&sess.token))
            .unwrap()
            .is_none(),
        "sesión sin usuario devuelve null (el || null de JS)"
    );
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}
