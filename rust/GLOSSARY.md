# GLOSSARY — Port de KaraokeMuse a Rust

Términos del dominio de la app original y su nombre en Rust, para que
todas las unidades nombren lo mismo de la misma manera.

| Término (original) | Significado | En Rust |
| --- | --- | --- |
| Sala / room | La sala de karaoke; una por servidor. Tiene código de 6 caracteres para unirse por QR | `Room` (tabla `rooms`), `room.code` |
| Código de sala | 6 caracteres (alfabeto sin 0/O/1/I) para entrar a la sala | `room_code: String` |
| Admin / administrador | Usuario con rol `admin`; único que edita, avanza la cola y gestiona solicitudes | `Role::Admin`, `role = "admin"` |
| Invitado / guest | Usuario con rol `guest`; entra con el código de sala; solo ve y agrega enlaces, y borra lo suyo | `Role::Guest`, `role = "guest"` |
| Canción / song | Entrada de la biblioteca: título, artista, plataforma (youtube/spotify/local), url o local_path, género, idioma, dificultad, estado, rating 0–5, favorita, notas, lyrics_text | `Song` (tabla `songs`), `song_out` → `SongOut` (`is_favorite: bool`) |
| Dificultad | `facil` / `media` / `dificil` | `Difficulty` (enum, default `media`) |
| Estado (de canción) | `want` / `practicing` / `mastered` (quiero aprenderla / en práctica / la domino) | `SongStatus` (enum, default `want`) |
| Cola / playlist activa | Orden de reproducción; cada elemento apunta a una canción y a quien la agregó | `QueueItem` (tabla `queue`), respuesta `queueId`, `position`, `addedBy`, `queuedAt` |
| Reproductor / Player | Canción actual en reproducción (`player_state`) | `PlayerState`, `current_song_id` |
| "A cantar" | 3 canciones aleatorias (`/api/songs/random`) | `random` (ORDER BY RANDOM() LIMIT ?) |
| Solicitud | Petición de invitado para que el admin genere un vídeo karaoke: video_url, título, artista, dificultad, idioma, notas, estado | `VideoRequest` (tabla `requests`), estados `pending` / `in_progress` / `done` |
| Sesión | Token opaco de 32 bytes aleatorios (hex, 64 chars); en BD se guarda su sha256 | `Session` (tabla `sessions`, `token_hash`), `expires_at` |
| Contraseña | scrypt (Node: `scryptSync(password, salt, 64)`, salt 16 bytes hex) | `hash_password` / `verify_password` (crate `scrypt`) |
| Vídeo local | Archivo de vídeo en el disco del servidor, enlazado (nunca copiado); solo el admin lo registra | `LocalVideo` (`platform = "local"`, `local_path`) |
| Metadatos / oEmbed | Título/artista/miniatura leídos de YouTube o Spotify por URL | `fetch_metadata` (reqwest), errores con mensaje en español y `ok: false` |
| Bearer token | Header `Authorization: Bearer <token>` | `bearer_token` (slice tras `"Bearer "`) |
| Datos / DATA_DIR | Directorio con `karaokemuse.sqlite` (`./data` por defecto) | `data_dir`, env `DATA_DIR` |
| Salud / health | `GET /api/health` → `{ok, app, version}` | ruta `GET /api/health` |

## Nombres de módulos (Rust)

| Original | Rust |
| --- | --- |
| `server/db.js` | crate `karaoke-db` → módulos `db` (`open_database`, `seed`, `random_code`, `new_id`) |
| `server/auth.js` | crate `karaoke-auth` → módulo `auth` (`hash_password`, `verify_password`, `create_session`, `get_user_from_token`, `destroy_session`, `public_user`) |
| `server/index.js` | crate `karaoke-routes` (handlers) + `rust/app` (axum, entry point) |
| `public/` | `public/` (estático, sin portar) |
