// Sembrador determinista para grabar los casos de `app record --http`.
//
// Crea desde cero una BD de pruebas en <dataDir> con valores FIJOS, para
// que las dos corridas de `app record` vean exactamente el mismo estado
// inicial y los casos estáticos puedan referenciar ids, tokens y el
// código de sala. Usa los propios módulos del servidor original
// (server/db.js, server/auth.js): el esquema es el de la app real.
//
// NUNCA apuntar esto a data/ (la BD real). Solo rust/record-data/.
//
// Uso: node rust/seed-record-db.mjs <dataDir>
import { openDatabase, seed } from '../server/db.js';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const dataDir = process.argv[2];
if (!dataDir) { console.error('uso: seed-record-db.mjs <dataDir>'); process.exit(2); }

// Tokens conocidos (64 hex = 32 bytes). Solo su sha256 vive en la BD.
export const ADMIN_TOKEN = 'a1'.repeat(32);
export const GUEST_TOKEN = 'b2'.repeat(32);
const FIXED_TS = '2026-11-20T12:00:00.000Z';
const FAR_FUTURE = '2099-01-01T00:00:00.000Z';

fs.rmSync(dataDir, { recursive: true, force: true });
const db = openDatabase(dataDir);
const { room } = seed(db, {
  adminUser: 'admin', adminPassword: 'AdminTest123!',
  roomName: 'Sala principal', roomPassword: '',
});
const admin = db.prepare("SELECT * FROM users WHERE role = 'admin'").get();

// Sala de código e id fijos (el seed genera un código aleatorio).
db.prepare('UPDATE rooms SET id = ?, code = ? WHERE id = ?')
  .run('room_seed001', 'KMTEST', room.id);
db.prepare('UPDATE player_state SET room_id = ? WHERE room_id = ?')
  .run('room_seed001', room.id);

const sha256 = (t) => crypto.createHash('sha256').update(t).digest('hex');

// Invitado de pruebas (sin contraseña: entra con el código de sala).
db.prepare(`INSERT INTO users (id, username, password_hash, salt, role, created_at)
  VALUES (?, ?, NULL, NULL, ?, ?)`)
  .run('u_guest001', 'InvitadoPrueba', 'guest', FIXED_TS);

// Sesiones de tokens conocidos, que no caducan durante la grabación.
db.prepare('INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)')
  .run(sha256(ADMIN_TOKEN), admin.id, FAR_FUTURE);
db.prepare('INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)')
  .run(sha256(GUEST_TOKEN), 'u_guest001', FAR_FUTURE);

// Vídeo local de pruebas: 42 bytes, bytes 0-9 = 'fake-mp4-c' (TESTS.md).
const videoRel = 'rust/record-data/test-video.mp4';
const videoContent = 'fake-mp4-content-0123456789abcdefghijklmno';
if (videoContent.length !== 42) throw new Error(`vídeo de pruebas: ${videoContent.length} bytes, deben ser 42`);
fs.writeFileSync(path.join(dataDir, 'test-video.mp4'), videoContent);

// 3 canciones sembradas (exactamente 3: /api/songs/random devuelve las 3,
// solo cambia el orden entre corridas -> ruido `order`, nunca `lines`).
const songs = [
  { id: 'song_seed001', title: 'Canción Admin', artist: 'Artista A', platform: 'youtube',
    url: 'https://www.youtube.com/watch?v=dQw4w9WgXcQ', rating: 5, owner: admin.id, fav: 1 },
  { id: 'song_seed002', title: 'Canción Invitado', artist: 'Artista B', platform: 'spotify',
    url: 'https://open.spotify.com/track/0000000000000000000000', rating: 3, owner: 'u_guest001', fav: 0 },
  { id: 'song_seed003', title: 'Vídeo Local', artist: 'Artista C', platform: 'local',
    url: '', local_path: videoRel, rating: 5, owner: 'u_guest001', fav: 0 },
];
for (const s of songs) {
  db.prepare(`INSERT INTO songs (id, title, artist, platform, url, local_path, thumbnail_url,
    genre, language, difficulty, status, rating, is_favorite, notes, lyrics_text,
    owner_user_id, created_at, updated_at)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
    .run(s.id, s.title, s.artist, s.platform, s.url, s.local_path || '',
      '', 'pop', 'español', 'media', 'want', s.rating, s.fav, '', '', s.owner, FIXED_TS, FIXED_TS);
}

// Cola sembrada: q_seed001 (invitado, pos 1) y q_seed002 (admin, pos 2).
db.prepare('INSERT INTO queue (id, room_id, song_id, user_id, position, created_at) VALUES (?, ?, ?, ?, ?, ?)')
  .run('q_seed001', 'room_seed001', 'song_seed001', 'u_guest001', 1, FIXED_TS);
db.prepare('INSERT INTO queue (id, room_id, song_id, user_id, position, created_at) VALUES (?, ?, ?, ?, ?, ?)')
  .run('q_seed002', 'room_seed001', 'song_seed002', admin.id, 2, FIXED_TS);

// Solicitudes sembradas: una del invitado y una del admin.
db.prepare(`INSERT INTO requests (id, video_url, title, artist, difficulty, language, notes,
  status, requester_user_id, admin_note, created_at, updated_at)
  VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
  .run('req_seed001', 'https://www.youtube.com/watch?v=dQw4w9WgXcQ', 'Petición Invitado',
    'Artista B', 'media', 'español', '', 'pending', 'u_guest001', '', FIXED_TS, FIXED_TS);
db.prepare(`INSERT INTO requests (id, video_url, title, artist, difficulty, language, notes,
  status, requester_user_id, admin_note, created_at, updated_at)
  VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
  .run('req_seed002', 'https://www.youtube.com/watch?v=dQw4w9WgXcQ', 'Petición Admin',
    'Artista A', 'media', 'español', '', 'pending', admin.id, '', FIXED_TS, FIXED_TS);

console.log('[seed-record-db] BD de grabación lista en', dataDir);
console.log('[seed-record-db] Sala KMTEST, admin AdminTest123!, tokens conocidos.');
