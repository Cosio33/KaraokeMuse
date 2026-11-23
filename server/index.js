// KaraokeMuse v1 — servidor HTML sin dependencias externas.
// Node 24, ESM, solo `node:http` + `node:sqlite`. Arranca con: npm start
// Variables de entorno:
//   PORT (8080), HOST (0.0.0.0), DATA_DIR (./data),
//   KARAOKE_ADMIN_USER (admin), KARAOKE_ADMIN_PASSWORD (¡cámbiala!),
//   KARAOKE_ROOM_NAME ("Sala principal"), KARAOKE_ROOM_PASSWORD (opcional).
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { openDatabase, seed, newId, randomCode } from './db.js';
import { hashPassword, verifyPassword, createSession, getUserFromToken, destroySession, publicUser } from './auth.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');
const PUBLIC = path.join(ROOT, 'public');

const PORT = Number(process.env.PORT || 8080);
const HOST = process.env.HOST || '0.0.0.0';
const DATA_DIR = process.env.DATA_DIR
  ? path.resolve(process.env.DATA_DIR)
  : path.join(ROOT, 'data');
const ADMIN_USER = process.env.KARAOKE_ADMIN_USER || 'admin';
const ADMIN_PASSWORD = process.env.KARAOKE_ADMIN_PASSWORD || 'admin123';
const ROOM_NAME = process.env.KARAOKE_ROOM_NAME || 'Sala principal';
const ROOM_PASSWORD = process.env.KARAOKE_ROOM_PASSWORD || '';

const db = openDatabase(DATA_DIR);
const { adminCreated, room } = seed(db, {
  adminUser: ADMIN_USER, adminPassword: ADMIN_PASSWORD,
  roomName: ROOM_NAME, roomPassword: ROOM_PASSWORD,
});
if (adminCreated) {
  console.log(`[KaraokeMuse] Admin creado: ${ADMIN_USER}` + (process.env.KARAOKE_ADMIN_PASSWORD
    ? ''
    : ' (contraseña por defecto "admin123" — ¡cámbiala con KARAOKE_ADMIN_PASSWORD!)'));
}
console.log(`[KaraokeMuse] Sala "${room.name}" código: ${room.code}${room.password_hash ? ' (con contraseña)' : ' (sin contraseña)'}`);

// ---------- helpers ----------
function sendJson(res, status, obj) {
  const body = JSON.stringify(obj);
  res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'content-length': Buffer.byteLength(body) });
  res.end(body);
}
function readBody(req, limit = 1024 * 1024) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    req.on('data', (c) => {
      size += c.length;
      if (size > limit) { reject(new Error('body demasiado grande')); req.destroy(); return; }
      chunks.push(c);
    });
    req.on('end', () => {
      if (!chunks.length) return resolve({});
      try { resolve(JSON.parse(Buffer.concat(chunks).toString('utf8'))); }
      catch { reject(new Error('JSON inválido')); }
    });
    req.on('error', reject);
  });
}
function bearer(req) {
  const h = req.headers.authorization || '';
  return h.startsWith('Bearer ') ? h.slice(7) : null;
}
function authUser(req) { return getUserFromToken(db, bearer(req)); }
function needAuth(req, res) {
  const user = authUser(req);
  if (!user) { sendJson(res, 401, { error: 'No autenticado. Entra a la sala con el QR o inicia sesión.' }); return null; }
  return user;
}
function needAdmin(req, res) {
  const user = needAuth(req, res);
  if (!user) return null;
  if (user.role !== 'admin') { sendJson(res, 403, { error: 'Solo el administrador puede hacer esto.' }); return null; }
  return user;
}
function songOut(row) {
  if (!row) return null;
  return { ...row, is_favorite: !!row.is_favorite };
}
const DIFFICULTIES = ['facil', 'media', 'dificil'];
const STATUSES = ['want', 'practicing', 'mastered'];
const PLATFORMS = ['youtube', 'spotify', 'local'];

function validateSongInput(b, isGuest) {
  const errors = [];
  if (!b.title || typeof b.title !== 'string' || !b.title.trim()) errors.push('title es obligatorio');
  if (b.platform && !PLATFORMS.includes(b.platform)) errors.push('platform inválida');
  const platform = b.platform || 'youtube';
  if (isGuest && platform === 'local') errors.push('Los invitados no pueden registrar vídeos locales; solo enlaces de YouTube o Spotify');
  if (platform !== 'local') {
    if (!b.url || typeof b.url !== 'string') errors.push('url es obligatoria para enlaces');
    else if (platform === 'youtube' && !/youtube\.com|youtu\.be/.test(b.url)) errors.push('url debe ser de YouTube');
    else if (platform === 'spotify' && !/spotify\.com|open\.spotify\.com/.test(b.url)) errors.push('url debe ser de Spotify');
  } else if (!b.local_path || typeof b.local_path !== 'string') {
    errors.push('local_path es obligatorio para vídeos locales');
  }
  if (b.difficulty && !DIFFICULTIES.includes(b.difficulty)) errors.push('difficulty inválida');
  if (b.status && !STATUSES.includes(b.status)) errors.push('status inválido');
  if (b.rating !== undefined && (!Number.isInteger(b.rating) || b.rating < 0 || b.rating > 5)) errors.push('rating debe ser entero de 0 a 5');
  return errors;
}

// ---------- archivos estáticos ----------
const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png', '.json': 'application/json', '.webmanifest': 'application/manifest+json' };
function serveStatic(req, res, urlPath) {
  const rel = urlPath === '/' ? 'index.html' : urlPath.replace(/^\/+/, '');
  const file = path.resolve(PUBLIC, rel);
  if (!file.startsWith(PUBLIC + path.sep) && file !== PUBLIC) { sendJson(res, 403, { error: 'Ruta no permitida' }); return; }
  if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) { sendJson(res, 404, { error: 'No encontrado' }); return; }
  const ext = path.extname(file);
  res.writeHead(200, { 'content-type': MIME[ext] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
}

// ---------- metadatos oEmbed (YouTube / Spotify) ----------
async function fetchMetadata(url) {
  let endpoint = null;
  if (/youtube\.com|youtu\.be/.test(url)) endpoint = `https://www.youtube.com/oembed?url=${encodeURIComponent(url)}&format=json`;
  else if (/spotify\.com|open\.spotify\.com/.test(url)) endpoint = `https://open.spotify.com/oembed?url=${encodeURIComponent(url)}`;
  else return { error: 'Plataforma no soportada para metadatos automáticos. Escribe título y artista manualmente.' };
  try {
    const r = await fetch(endpoint, { signal: AbortSignal.timeout(8000), headers: { 'user-agent': 'KaraokeMuse/1.0' } });
    if (!r.ok) return { error: 'No se pudieron leer los metadatos del enlace. Escribe título y artista manualmente.' };
    const j = await r.json();
    return {
      platform: /youtube/.test(url) ? 'youtube' : 'spotify',
      title: j.title || '', artist: j.author_name || '',
      thumbnail_url: j.thumbnail_url || '',
    };
  } catch {
    return { error: 'Sin red para leer metadatos. Escribe título y artista manualmente.' };
  }
}

// ---------- rutas API ----------
async function handle(req, res) {
  const url = new URL(req.url, `http://${req.headers.host || 'localhost'}`);
  const p = url.pathname;
  const method = req.method;

  try {
    // Salud
    if (p === '/api/health' && method === 'GET') return sendJson(res, 200, { ok: true, app: 'KaraokeMuse', version: '1.0.0' });

    // Login admin
    if (p === '/api/auth/login' && method === 'POST') {
      const b = await readBody(req);
      const user = db.prepare('SELECT * FROM users WHERE username = ?').get(String(b.username || ''));
      if (!user || user.role !== 'admin' || !verifyPassword(String(b.password || ''), user.salt, user.password_hash)) {
        return sendJson(res, 401, { error: 'Usuario o contraseña incorrectos.' });
      }
      const s = createSession(db, user.id);
      return sendJson(res, 200, { token: s.token, expiresAt: s.expiresAt, user: publicUser(user) });
    }
    if (p === '/api/auth/logout' && method === 'POST') {
      destroySession(db, bearer(req));
      return sendJson(res, 200, { ok: true });
    }
    if (p === '/api/me' && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      return sendJson(res, 200, { user: publicUser(user) });
    }

    // Sala + unirse con QR/código
    if (p === '/api/room' && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      return sendJson(res, 200, { id: room.id, name: room.name, code: room.code, hasPassword: !!room.password_hash });
    }
    if (p === '/api/join' && method === 'POST') {
      const b = await readBody(req);
      const code = String(b.code || '').trim().toUpperCase();
      if (code !== room.code) return sendJson(res, 404, { error: 'Código de sala incorrecto. Escanea el QR de nuevo.' });
      if (room.password_hash && !verifyPassword(String(b.password || ''), room.salt, room.password_hash)) {
        return sendJson(res, 401, { error: 'Contraseña de sala incorrecta.' });
      }
      const name = String(b.name || '').trim().slice(0, 30) || `Invitado-${randomCode(4)}`;
      const id = newId('u');
      db.prepare('INSERT INTO users (id, username, password_hash, salt, role, created_at) VALUES (?, ?, NULL, NULL, ?, ?)')
        .run(id, name, 'guest', new Date().toISOString());
      const s = createSession(db, id);
      return sendJson(res, 200, { token: s.token, expiresAt: s.expiresAt, user: { id, username: name, role: 'guest' }, room: { id: room.id, name: room.name, code: room.code } });
    }

    // Metadatos del enlace (oEmbed)
    if (p === '/api/metadata' && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      const target = url.searchParams.get('url') || '';
      const meta = await fetchMetadata(target);
      if (meta.error) return sendJson(res, 200, { ok: false, error: meta.error });
      return sendJson(res, 200, { ok: true, ...meta });
    }

    // Biblioteca
    if (p === '/api/songs' && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      const q = (url.searchParams.get('q') || '').toLowerCase();
      const platform = url.searchParams.get('platform') || '';
      const minRating = Number(url.searchParams.get('minRating') || 0);
      let rows = db.prepare('SELECT * FROM songs ORDER BY created_at DESC').all().map(songOut);
      if (q) rows = rows.filter((s) => `${s.title} ${s.artist} ${s.genre}`.toLowerCase().includes(q));
      if (platform) rows = rows.filter((s) => s.platform === platform);
      if (minRating) rows = rows.filter((s) => s.rating >= minRating);
      return sendJson(res, 200, { songs: rows });
    }
    if (p === '/api/songs/random' && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      const count = Math.min(Math.max(Number(url.searchParams.get('count') || 3), 1), 10);
      const rows = db.prepare('SELECT * FROM songs ORDER BY RANDOM() LIMIT ?').all(count).map(songOut);
      return sendJson(res, 200, { songs: rows });
    }
    if (p === '/api/songs' && method === 'POST') {
      const user = needAuth(req, res); if (!user) return;
      const b = await readBody(req);
      const isGuest = user.role === 'guest';
      const errors = validateSongInput(b, isGuest);
      if (errors.length) return sendJson(res, isGuest && b.platform === 'local' ? 403 : 400, { error: errors.join('. ') });
      const platform = b.platform || 'youtube';
      if (platform === 'local' && !fs.existsSync(b.local_path)) {
        return sendJson(res, 400, { error: `El archivo local no existe en el servidor: ${b.local_path}` });
      }
      const now = new Date().toISOString();
      const id = newId('song');
      db.prepare(`INSERT INTO songs (id, title, artist, platform, url, local_path, thumbnail_url, genre, language, difficulty, status, rating, is_favorite, notes, lyrics_text, owner_user_id, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
        .run(id, b.title.trim(), String(b.artist || '').trim(), platform,
          String(b.url || ''), String(b.local_path || ''), String(b.thumbnail_url || ''),
          String(b.genre || ''), String(b.language || ''), b.difficulty || 'media',
          b.status || 'want', b.rating || 0, b.is_favorite ? 1 : 0,
          String(b.notes || ''), String(b.lyrics_text || ''), user.id, now, now);
      return sendJson(res, 201, { song: songOut(db.prepare('SELECT * FROM songs WHERE id = ?').get(id)) });
    }
    if (p.startsWith('/api/songs/') && method === 'PATCH') {
      const user = needAdmin(req, res); if (!user) return; // editar: solo admin
      const id = p.split('/')[3];
      const song = db.prepare('SELECT * FROM songs WHERE id = ?').get(id);
      if (!song) return sendJson(res, 404, { error: 'Canción no encontrada.' });
      const b = await readBody(req);
      const fields = ['title', 'artist', 'platform', 'url', 'local_path', 'thumbnail_url', 'genre', 'language', 'difficulty', 'status', 'rating', 'is_favorite', 'notes', 'lyrics_text'];
      const sets = [], vals = [];
      for (const f of fields) {
        if (b[f] === undefined) continue;
        if (f === 'platform' && !PLATFORMS.includes(b[f])) return sendJson(res, 400, { error: 'platform inválida' });
        if (f === 'rating' && (!Number.isInteger(b[f]) || b[f] < 0 || b[f] > 5)) return sendJson(res, 400, { error: 'rating debe ser entero de 0 a 5' });
        sets.push(`${f} = ?`); vals.push(f === 'is_favorite' ? (b[f] ? 1 : 0) : b[f]);
      }
      if (!sets.length) return sendJson(res, 400, { error: 'Nada que actualizar.' });
      sets.push('updated_at = ?'); vals.push(new Date().toISOString());
      db.prepare(`UPDATE songs SET ${sets.join(', ')} WHERE id = ?`).run(...vals, id);
      return sendJson(res, 200, { song: songOut(db.prepare('SELECT * FROM songs WHERE id = ?').get(id)) });
    }
    if (p.startsWith('/api/songs/') && method === 'DELETE') {
      const user = needAuth(req, res); if (!user) return;
      const id = p.split('/')[3];
      const song = db.prepare('SELECT * FROM songs WHERE id = ?').get(id);
      if (!song) return sendJson(res, 404, { error: 'Canción no encontrada.' });
      // Invitado: solo puede borrar canciones que él subió. Admin: cualquiera.
      if (user.role !== 'admin' && song.owner_user_id !== user.id) {
        return sendJson(res, 403, { error: 'No puedes borrar canciones de otros usuarios. Solo el admin puede hacerlo.' });
      }
      db.prepare('DELETE FROM songs WHERE id = ?').run(id);
      db.prepare('DELETE FROM queue WHERE song_id = ?').run(id);
      return sendJson(res, 200, { ok: true });
    }

    // Cola / playlist activa
    if (p === '/api/queue' && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      const rows = db.prepare(`
        SELECT q.id AS queue_id, q.position, q.user_id, q.created_at AS queued_at,
               s.*, u.username AS added_by
        FROM queue q JOIN songs s ON s.id = q.song_id
        LEFT JOIN users u ON u.id = q.user_id
        WHERE q.room_id = ? ORDER BY q.position ASC`).all(room.id);
      return sendJson(res, 200, { queue: rows.map((r) => ({ queueId: r.queue_id, position: r.position, userId: r.user_id, addedBy: r.added_by, queuedAt: r.queued_at, song: songOut(r) })) });
    }
    if (p === '/api/queue' && method === 'POST') {
      const user = needAuth(req, res); if (!user) return;
      const b = await readBody(req);
      const song = db.prepare('SELECT * FROM songs WHERE id = ?').get(String(b.songId || ''));
      if (!song) return sendJson(res, 404, { error: 'Canción no encontrada.' });
      const max = db.prepare('SELECT MAX(position) AS m FROM queue WHERE room_id = ?').get(room.id);
      const id = newId('q');
      db.prepare('INSERT INTO queue (id, room_id, song_id, user_id, position, created_at) VALUES (?, ?, ?, ?, ?, ?)')
        .run(id, room.id, song.id, user.id, (max?.m ?? 0) + 1, new Date().toISOString());
      return sendJson(res, 201, { queueId: id });
    }
    if (p.startsWith('/api/queue/') && method === 'DELETE') {
      const user = needAuth(req, res); if (!user) return;
      const id = p.split('/')[3];
      const item = db.prepare('SELECT * FROM queue WHERE id = ?').get(id);
      if (!item) return sendJson(res, 404, { error: 'Elemento de cola no encontrado.' });
      if (user.role !== 'admin' && item.user_id !== user.id) {
        return sendJson(res, 403, { error: 'No puedes quitar de la cola canciones de otros usuarios.' });
      }
      db.prepare('DELETE FROM queue WHERE id = ?').run(id);
      return sendJson(res, 200, { ok: true });
    }
    if (p === '/api/queue/next' && method === 'POST') {
      const user = needAdmin(req, res); if (!user) return; // avanzar la cola: solo admin/anfitrión
      const first = db.prepare('SELECT * FROM queue WHERE room_id = ? ORDER BY position ASC LIMIT 1').get(room.id);
      if (!first) return sendJson(res, 200, { song: null, message: 'La cola está vacía.' });
      db.prepare('DELETE FROM queue WHERE id = ?').run(first.id);
      const song = db.prepare('SELECT * FROM songs WHERE id = ?').get(first.song_id);
      db.prepare('UPDATE player_state SET current_song_id = ?, updated_at = ? WHERE room_id = ?')
        .run(song ? song.id : null, new Date().toISOString(), room.id);
      return sendJson(res, 200, { song: songOut(song) });
    }
    if (p === '/api/player' && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      const st = db.prepare('SELECT * FROM player_state WHERE room_id = ?').get(room.id);
      const song = st?.current_song_id ? db.prepare('SELECT * FROM songs WHERE id = ?').get(st.current_song_id) : null;
      return sendJson(res, 200, { song: songOut(song) });
    }

    // Solicitudes de generación de vídeo karaoke (el generador sigue en la VM de Muse)
    if (p === '/api/requests' && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      const rows = user.role === 'admin'
        ? db.prepare('SELECT r.*, u.username AS requester FROM requests r LEFT JOIN users u ON u.id = r.requester_user_id ORDER BY r.created_at DESC').all()
        : db.prepare('SELECT r.*, u.username AS requester FROM requests r LEFT JOIN users u ON u.id = r.requester_user_id WHERE r.requester_user_id = ? ORDER BY r.created_at DESC').all(user.id);
      return sendJson(res, 200, { requests: rows });
    }
    if (p === '/api/requests' && method === 'POST') {
      const user = needAuth(req, res); if (!user) return;
      const b = await readBody(req);
      if (!b.video_url || !b.title) return sendJson(res, 400, { error: 'video_url (enlace del vídeo original) y title (nombre de la canción) son obligatorios.' });
      if (b.difficulty && !DIFFICULTIES.includes(b.difficulty)) return sendJson(res, 400, { error: 'difficulty inválida' });
      const now = new Date().toISOString();
      const id = newId('req');
      db.prepare('INSERT INTO requests (id, video_url, title, artist, difficulty, language, notes, status, requester_user_id, admin_note, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)')
        .run(id, String(b.video_url), String(b.title).trim(), String(b.artist || ''), b.difficulty || 'media',
          String(b.language || ''), String(b.notes || ''), 'pending', user.id, '', now, now);
      return sendJson(res, 201, { request: db.prepare('SELECT * FROM requests WHERE id = ?').get(id) });
    }
    if (p.startsWith('/api/requests/') && method === 'PATCH') {
      const user = needAdmin(req, res); if (!user) return;
      const id = p.split('/')[3];
      const reqRow = db.prepare('SELECT * FROM requests WHERE id = ?').get(id);
      if (!reqRow) return sendJson(res, 404, { error: 'Solicitud no encontrada.' });
      const b = await readBody(req);
      if (b.status && !['pending', 'in_progress', 'done'].includes(b.status)) return sendJson(res, 400, { error: 'status inválido' });
      db.prepare('UPDATE requests SET status = COALESCE(?, status), admin_note = COALESCE(?, admin_note), updated_at = ? WHERE id = ?')
        .run(b.status || null, b.admin_note ?? null, new Date().toISOString(), id);
      return sendJson(res, 200, { request: db.prepare('SELECT * FROM requests WHERE id = ?').get(id) });
    }

    // Streaming de vídeos locales (solo admin los registra; se sirven por rangos, nunca se copian)
    if (p.startsWith('/api/media/local/') && method === 'GET') {
      const user = needAuth(req, res); if (!user) return;
      const id = p.split('/')[4];
      const song = db.prepare('SELECT * FROM songs WHERE id = ?').get(id);
      if (!song || song.platform !== 'local') return sendJson(res, 404, { error: 'Vídeo local no encontrado.' });
      if (!fs.existsSync(song.local_path)) return sendJson(res, 404, { error: 'El archivo local ya no está en el disco del servidor.' });
      const stat = fs.statSync(song.local_path);
      const ext = path.extname(song.local_path).toLowerCase();
      const type = ext === '.webm' ? 'video/webm' : ext === '.mkv' ? 'video/x-matroska' : 'video/mp4';
      const range = req.headers.range;
      if (range) {
        const m = /bytes=(\d*)-(\d*)/.exec(range);
        const start = m && m[1] ? Number(m[1]) : 0;
        const end = m && m[2] ? Number(m[2]) : stat.size - 1;
        res.writeHead(206, { 'content-type': type, 'content-range': `bytes ${start}-${end}/${stat.size}`, 'accept-ranges': 'bytes', 'content-length': end - start + 1 });
        fs.createReadStream(song.local_path, { start, end }).pipe(res);
      } else {
        res.writeHead(200, { 'content-type': type, 'accept-ranges': 'bytes', 'content-length': stat.size });
        fs.createReadStream(song.local_path).pipe(res);
      }
      return;
    }

    // Estáticos (frontend)
    if (method === 'GET') return serveStatic(req, res, p);
    return sendJson(res, 404, { error: 'Ruta no encontrada.' });
  } catch (err) {
    console.error('[KaraokeMuse]', err);
    if (!res.headersSent) sendJson(res, 500, { error: 'Error interno del servidor.' });
  }
}

const server = http.createServer((req, res) => { handle(req, res); });
server.listen(PORT, HOST, () => {
  console.log(`[KaraokeMuse] Servidor en http://${HOST === '0.0.0.0' ? 'localhost' : HOST}:${PORT}`);
  console.log(`[KaraokeMuse] QR / unirse: http://localhost:${PORT}/?room=${room.code}`);
  console.log(`[KaraokeMuse] Datos en: ${DATA_DIR}`);
});
