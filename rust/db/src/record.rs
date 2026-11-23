//! Siembra determinista de la BD de pruebas (`rust/record-data`).
//!
//! Espejo en Rust de `rust/seed-record-db.mjs` (ADR-018): borra el
//! directorio de datos y lo vuelve a sembrar con valores FIJOS, para
//! que el replay vea siempre el mismo estado inicial sin depender del
//! servidor Node original. El binario puro la llama al arrancar solo
//! cuando `KARAOKE_RESET_RECORD_DB` está fijada (ver `rust/app`).
//!
//! Valores fijos (idénticos a los del `.mjs`): admin `admin` /
//! `AdminTest123!`, sala `room_seed001` / código `KMTEST`, invitado
//! `u_guest001`, sesiones de tokens conocidos (`'a1'`×32 para el
//! admin, `'b2'`×32 para el invitado, solo su sha256 vive en la BD),
//! 3 canciones, 2 entradas de cola, 2 solicitudes y el vídeo de
//! pruebas de 42 bytes. El `local_path` guardado es
//! `rust/record-data/test-video.mp4`, relativo a la raíz del repo,
//! igual que en el `.mjs`.
//!
//! Seguridad: la función se NIEGA a tocar un directorio que no se
//! llame exactamente `record-data`, para que un descuido con
//! `DATA_DIR` no pueda borrar jamás la BD real (`data/`).

use std::path::Path;

use rusqlite::params;

use crate::js::JsValue;
use crate::store::{SeedOpts, open_database, seed};

/// Token conocido del admin en la BD de pruebas (64 hex = 32 bytes).
pub const ADMIN_TOKEN: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
/// Token conocido del invitado en la BD de pruebas.
pub const GUEST_TOKEN: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
/// Timestamp fijo de las filas sembradas (igual que en el `.mjs`).
pub const FIXED_TS: &str = "2026-11-20T12:00:00.000Z";
/// Caducidad de las sesiones sembradas (no caducan en el replay).
pub const FAR_FUTURE: &str = "2099-01-01T00:00:00.000Z";
/// Contenido del vídeo local de pruebas: 42 bytes, bytes 0-9 =
/// `fake-mp4-c` (TESTS.md).
pub const VIDEO_CONTENT: &str = "fake-mp4-content-0123456789abcdefghijklmno";
/// `local_path` guardado en la BD para el vídeo local (relativo a la
/// raíz del repo, como lo guarda el sembrador Node).
pub const VIDEO_LOCAL_PATH: &str = "rust/record-data/test-video.mp4";

/// Borra `data_dir` y la vuelve a sembrar con el fixture de pruebas.
///
/// `data_dir` debe llamarse exactamente `record-data`; cualquier otro
/// nombre es un error y no se borra nada.
pub fn reset_record_data(data_dir: &Path) -> Result<(), String> {
    if data_dir.file_name().and_then(|n| n.to_str()) != Some("record-data") {
        return Err(format!(
            "reset_record_data: el directorio debe llamarse `record-data`, no `{}`",
            data_dir.display()
        ));
    }
    match std::fs::remove_dir_all(data_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("no se pudo borrar {}: {e}", data_dir.display())),
    }
    let mut db = open_database(&JsValue::Str(data_dir.to_string_lossy().into_owned()))
        .map_err(|e| e.to_string())?;
    let outcome = seed(
        &mut db,
        &SeedOpts {
            admin_user: "admin".to_string(),
            admin_password: "AdminTest123!".to_string(),
            room_name: "Sala principal".to_string(),
            room_password: None,
        },
    )
    .map_err(|e| e.to_string())?;
    let admin_id: String = db
        .conn
        .query_row("SELECT id FROM users WHERE role = 'admin'", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    let room_id = outcome.room.id;

    // Sala de código e id fijos (el seed genera un código aleatorio).
    db.conn
        .execute(
            "UPDATE rooms SET id = ?1, code = ?2 WHERE id = ?3",
            params!["room_seed001", "KMTEST", room_id],
        )
        .map_err(|e| e.to_string())?;
    db.conn
        .execute(
            "UPDATE player_state SET room_id = ?1 WHERE room_id = ?2",
            params!["room_seed001", room_id],
        )
        .map_err(|e| e.to_string())?;

    // Invitado de pruebas (sin contraseña: entra con el código de sala).
    db.conn
        .execute(
            "INSERT INTO users (id, username, password_hash, salt, role, created_at) VALUES (?1, ?2, NULL, NULL, ?3, ?4)",
            params!["u_guest001", "InvitadoPrueba", "guest", FIXED_TS],
        )
        .map_err(|e| e.to_string())?;

    // Sesiones de tokens conocidos, que no caducan durante la grabación.
    for (token, user_id) in [
        (ADMIN_TOKEN, admin_id.as_str()),
        (GUEST_TOKEN, "u_guest001"),
    ] {
        db.conn
            .execute(
                "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)",
                params![karaoke_crypto::token_hash(token), user_id, FAR_FUTURE],
            )
            .map_err(|e| e.to_string())?;
    }

    // Vídeo local de pruebas (42 bytes).
    debug_assert_eq!(VIDEO_CONTENT.len(), 42);
    std::fs::write(data_dir.join("test-video.mp4"), VIDEO_CONTENT).map_err(|e| e.to_string())?;

    // 3 canciones sembradas (exactamente 3: /api/songs/random devuelve
    // las 3, solo cambia el orden entre corridas).
    struct SeedSong {
        id: &'static str,
        title: &'static str,
        artist: &'static str,
        platform: &'static str,
        url: &'static str,
        local_path: &'static str,
        rating: i64,
        fav: i64,
        owner: String,
    }
    let songs = [
        SeedSong {
            id: "song_seed001",
            title: "Canción Admin",
            artist: "Artista A",
            platform: "youtube",
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            local_path: "",
            rating: 5,
            fav: 1,
            owner: admin_id.clone(),
        },
        SeedSong {
            id: "song_seed002",
            title: "Canción Invitado",
            artist: "Artista B",
            platform: "spotify",
            url: "https://open.spotify.com/track/0000000000000000000000",
            local_path: "",
            rating: 3,
            fav: 0,
            owner: "u_guest001".to_string(),
        },
        SeedSong {
            id: "song_seed003",
            title: "Vídeo Local",
            artist: "Artista C",
            platform: "local",
            url: "",
            local_path: VIDEO_LOCAL_PATH,
            rating: 5,
            fav: 0,
            owner: "u_guest001".to_string(),
        },
    ];
    for s in songs {
        db.conn
            .execute(
                "INSERT INTO songs (id, title, artist, platform, url, local_path, thumbnail_url, genre, language, difficulty, status, rating, is_favorite, notes, lyrics_text, owner_user_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', 'pop', 'español', 'media', 'want', ?7, ?8, '', '', ?9, ?10, ?10)",
                params![s.id, s.title, s.artist, s.platform, s.url, s.local_path, s.rating, s.fav, s.owner, FIXED_TS],
            )
            .map_err(|e| e.to_string())?;
    }

    // Cola sembrada: q_seed001 (invitado, pos 1) y q_seed002 (admin, pos 2).
    for (id, song_id, user_id, position) in [
        ("q_seed001", "song_seed001", "u_guest001", 1),
        ("q_seed002", "song_seed002", admin_id.as_str(), 2),
    ] {
        db.conn
            .execute(
                "INSERT INTO queue (id, room_id, song_id, user_id, position, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, "room_seed001", song_id, user_id, position, FIXED_TS],
            )
            .map_err(|e| e.to_string())?;
    }

    // Solicitudes sembradas: una del invitado y una del admin.
    for (id, title, artist, requester) in [
        (
            "req_seed001",
            "Petición Invitado",
            "Artista B",
            "u_guest001",
        ),
        (
            "req_seed002",
            "Petición Admin",
            "Artista A",
            admin_id.as_str(),
        ),
    ] {
        db.conn
            .execute(
                "INSERT INTO requests (id, video_url, title, artist, difficulty, language, notes, status, requester_user_id, admin_note, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'media', 'español', '', 'pending', ?5, '', ?6, ?6)",
                params![
                    id,
                    "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                    title,
                    artist,
                    requester,
                    FIXED_TS
                ],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
