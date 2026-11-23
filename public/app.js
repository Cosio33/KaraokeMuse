// KaraokeMuse — frontend sin compilación. Toda la autorización vive en el servidor.
const $ = (id) => document.getElementById(id);
let token = localStorage.getItem('km_token') || '';
let me = null;
let room = null;
let songs = [];

async function api(path, opts = {}) {
  const r = await fetch(path, {
    ...opts,
    headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}), ...(opts.headers || {}) },
  });
  const data = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(data.error || `Error ${r.status}`);
  return data;
}
function stars(n) { return '★'.repeat(n || 0) + '☆'.repeat(5 - (n || 0)); }
function esc(s) { return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])); }

function youtubeId(url) {
  const m = String(url || '').match(/(?:v=|youtu\.be\/|shorts\/|embed\/)([\w-]{6,})/);
  return m ? m[1] : null;
}
function embedUrl(song) {
  if (song.platform === 'youtube') {
    const id = youtubeId(song.url);
    return id ? `https://www.youtube.com/embed/${id}?autoplay=1&rel=0` : null;
  }
  if (song.platform === 'spotify') {
    const m = String(song.url || '').match(/open\.spotify\.com\/(track|album|playlist|episode)\/([\w]+)/);
    return m ? `https://open.spotify.com/embed/${m[1]}/${m[2]}?utm_source=generator` : null;
  }
  return null;
}
function platformLabel(p) { return p === 'youtube' ? '▶️ YouTube' : p === 'spotify' ? '🎧 Spotify' : '💾 Vídeo local'; }

function showApp() {
  $('authView').classList.add('hidden');
  $('appView').classList.remove('hidden');
  $('sessionBar').classList.remove('hidden');
  $('sessionInfo').textContent = `${me.username} (${me.role === 'admin' ? 'admin' : 'invitado'}) · Sala ${room ? room.name : ''}`;
  $('adminTab').style.display = me.role === 'admin' ? '' : 'none';
  if (me.role !== 'admin') switchTab('library');
  drawQR();
  refreshAll();
}
function showAuth() {
  $('authView').classList.remove('hidden');
  $('appView').classList.add('hidden');
  $('sessionBar').classList.add('hidden');
}
function drawQR() {
  if (!room) return;
  $('roomName').textContent = room.name;
  $('roomCode').textContent = room.code;
  const joinUrl = `${location.origin}/?room=${room.code}`;
  try {
    const qr = qrcode(0, 'M');
    qr.addData(joinUrl);
    qr.make();
    $('qrBox').innerHTML = qr.createSvgTag({ cellSize: 4, margin: 8 });
  } catch (e) { $('qrBox').innerHTML = `<p>${esc(joinUrl)}</p>`; }
}
function switchTab(name) {
  document.querySelectorAll('.tab').forEach((b) => b.classList.toggle('active', b.dataset.tab === name));
  document.querySelectorAll('.tabpane').forEach((s) => s.classList.add('hidden'));
  $(`tab-${name}`).classList.remove('hidden');
  if (name === 'queue') loadQueue();
  if (name === 'player') loadPlayer();
  if (name === 'sing') shuffleSing();
  if (name === 'requests') loadRequests();
  if (name === 'admin') loadAdminRequests();
}
document.querySelectorAll('.tab').forEach((b) => b.addEventListener('click', () => switchTab(b.dataset.tab)));

async function refreshAll() { await loadSongs(); await loadQueue(); await loadPlayer(); }

async function loadSongs() {
  const q = $('fSearch').value.trim();
  const platform = $('fPlatform').value;
  const minRating = $('fRating').value;
  const params = new URLSearchParams();
  if (q) params.set('q', q);
  if (platform) params.set('platform', platform);
  if (minRating) params.set('minRating', minRating);
  const d = await api(`/api/songs?${params}`);
  songs = d.songs;
  $('songList').innerHTML = songs.length ? songs.map(songCard).join('') : '<p>No hay canciones todavía. Agrega la primera con el formulario de abajo.</p>';
}
function songCard(s) {
  const thumb = s.thumbnail_url ? `<img src="${esc(s.thumbnail_url)}" alt="">` : '<img alt="">';
  return `<div class="song">${thumb}<div class="info"><b>${esc(s.title)}</b>
    <div class="meta">${esc(s.artist || 'Sin artista')} · ${platformLabel(s.platform)} · ${esc(s.genre || '')} ${esc(s.language || '')} · ${esc(s.difficulty)} · ${stars(s.rating)}${s.is_favorite ? ' ⭐' : ''}</div>
    <div class="meta">${esc(s.status === 'want' ? 'Quiero aprenderla' : s.status === 'practicing' ? 'En práctica' : 'La domino')}</div>
    ${s.notes ? `<div class="meta">📝 ${esc(s.notes)}</div>` : ''}</div>
    <div class="actions">
      <button class="btn small" onclick="addToQueue('${s.id}')">+ Cola</button>
      ${s.url ? `<a class="btn small" href="${esc(s.url)}" target="_blank" rel="noopener">Abrir</a>` : ''}
      <button class="btn small" onclick="deleteSong('${s.id}')">🗑 Borrar</button>
    </div></div>`;
}
window.addToQueue = async (id) => {
  try { await api('/api/queue', { method: 'POST', body: JSON.stringify({ songId: id }) }); await loadQueue(); alert('Agregada a la cola 🎶'); }
  catch (e) { alert(e.message); }
};
window.deleteSong = async (id) => {
  if (!confirm('¿Borrar esta canción?')) return;
  try { await api(`/api/songs/${id}`, { method: 'DELETE' }); await loadSongs(); await loadQueue(); }
  catch (e) { alert(e.message); }
};

async function loadQueue() {
  const d = await api('/api/queue');
  $('queueCount').textContent = d.queue.length;
  $('queueList').innerHTML = d.queue.length ? d.queue.map((q, i) => `
    <div class="song"><div class="info"><b>${i + 1}. ${esc(q.song.title)}</b>
    <div class="meta">${esc(q.song.artist || '')} · ${platformLabel(q.song.platform)} · la puso ${esc(q.addedBy || 'alguien')}</div></div>
    <div class="actions"><button class="btn small" onclick="removeQueue('${q.queueId}')">Quitar</button></div></div>`).join('')
    : '<p>La cola está vacía. Agrega canciones desde la Biblioteca.</p>';
}
window.removeQueue = async (id) => {
  try { await api(`/api/queue/${id}`, { method: 'DELETE' }); await loadQueue(); }
  catch (e) { alert(e.message); }
};

async function loadPlayer() {
  const d = await api('/api/player');
  const box = $('playerBox');
  if (!d.song) { box.innerHTML = '<p>Aún no hay canción sonando. Pide la siguiente desde la Cola.</p>'; $('lyricsBox').classList.add('hidden'); return; }
  const s = d.song;
  box.innerHTML = `<h3>${esc(s.title)} — ${esc(s.artist || '')}</h3>` + (
    s.platform === 'local'
      ? `<video src="/api/media/local/${s.id}" controls autoplay></video>`
      : embedUrl(s)
        ? `<iframe src="${esc(embedUrl(s))}" allow="autoplay; encrypted-media; fullscreen" allowfullscreen></iframe>`
        : `<p><a href="${esc(s.url)}" target="_blank" rel="noopener">Abrir en ${esc(s.platform)}</a> (no se pudo embeber este enlace).</p>`);
  if (s.lyrics_text) { $('lyricsBox').classList.remove('hidden'); $('lyricsText').textContent = s.lyrics_text; }
  else $('lyricsBox').classList.add('hidden');
}

async function shuffleSing() {
  try {
    const d = await api('/api/songs/random?count=3');
    $('singList').innerHTML = d.songs.length ? d.songs.map((s, i) => `
      <div class="song"><div class="info"><b>${i + 1}. ${esc(s.title)}</b>
      <div class="meta">${esc(s.artist || '')} · ${platformLabel(s.platform)} · ${esc(s.difficulty)} · ${stars(s.rating)}</div></div>
      <div class="actions"><button class="btn small" onclick="addToQueue('${s.id}')">+ Cola</button></div></div>`).join('')
      : '<p>Tu repertorio está vacío. Agrega canciones en la Biblioteca para sortearlas aquí.</p>';
  } catch (e) { $('singList').innerHTML = `<p>${esc(e.message)}</p>`; }
}

async function loadRequests() {
  try {
    const d = await api('/api/requests');
    $('reqList').innerHTML = d.requests.length ? d.requests.map((r) => `
      <div class="song"><div class="info"><b>${esc(r.title)}</b>
      <div class="meta">${esc(r.artist || '')} · ${esc(r.difficulty)} · ${esc(r.language || '')} · Estado: <strong>${r.status === 'pending' ? 'Pendiente' : r.status === 'in_progress' ? 'En proceso' : 'Hecho'}</strong></div>
      ${r.admin_note ? `<div class="meta">Admin: ${esc(r.admin_note)}</div>` : ''}</div></div>`).join('')
      : '<p>No has pedido ningún vídeo todavía.</p>';
  } catch (e) { $('reqList').innerHTML = `<p>${esc(e.message)}</p>`; }
}
async function loadAdminRequests() {
  try {
    const d = await api('/api/requests');
    $('adminReqList').innerHTML = d.requests.length ? d.requests.map((r) => `
      <div class="song"><div class="info"><b>${esc(r.title)}</b>
      <div class="meta">${esc(r.artist || '')} · ${esc(r.difficulty)} · ${esc(r.language || '')} · la pidió ${esc(r.requester || '')}</div>
      <div class="meta"><a href="${esc(r.video_url)}" target="_blank" rel="noopener">${esc(r.video_url)}</a></div>
      ${r.notes ? `<div class="meta">📝 ${esc(r.notes)}</div>` : ''}
      <label>Estado <select onchange="setReqStatus('${r.id}', this.value)">
        <option value="pending" ${r.status === 'pending' ? 'selected' : ''}>Pendiente</option>
        <option value="in_progress" ${r.status === 'in_progress' ? 'selected' : ''}>En proceso</option>
        <option value="done" ${r.status === 'done' ? 'selected' : ''}>Hecho</option></select></label>
      <label>Nota del admin <input value="${esc(r.admin_note || '')}" onchange="setReqNote('${r.id}', this.value)"></label>
      </div></div>`).join('')
      : '<p>No hay solicitudes.</p>';
  } catch (e) { $('adminReqList').innerHTML = `<p>${esc(e.message)}</p>`; }
}
window.setReqStatus = async (id, status) => { try { await api(`/api/requests/${id}`, { method: 'PATCH', body: JSON.stringify({ status }) }); } catch (e) { alert(e.message); } };
window.setReqNote = async (id, note) => { try { await api(`/api/requests/${id}`, { method: 'PATCH', body: JSON.stringify({ admin_note: note }) }); } catch (e) { alert(e.message); } };

// Auth
$('joinBtn').addEventListener('click', async () => {
  try {
    const d = await api('/api/join', { method: 'POST', body: JSON.stringify({ code: $('joinCode').value, name: $('joinName').value, password: $('joinPassword').value }) });
    token = d.token; localStorage.setItem('km_token', token); me = d.user; room = d.room;
    showApp();
  } catch (e) { $('joinMsg').textContent = e.message; }
});
$('loginBtn').addEventListener('click', async () => {
  try {
    const d = await api('/api/auth/login', { method: 'POST', body: JSON.stringify({ username: $('adminUser').value, password: $('adminPass').value }) });
    token = d.token; localStorage.setItem('km_token', token); me = d.user;
    room = (await api('/api/room')).id ? await api('/api/room') : null;
    showApp();
  } catch (e) { $('loginMsg').textContent = e.message; }
});
$('logoutBtn').addEventListener('click', async () => {
  try { await api('/api/auth/logout', { method: 'POST' }); } catch { /* da igual */ }
  token = ''; me = null; room = null; localStorage.removeItem('km_token'); showAuth();
});

// Biblioteca: filtros y alta
['fSearch', 'fPlatform', 'fRating'].forEach((id) => $(id).addEventListener('input', () => loadSongs().catch(() => {})));
$('fClear').addEventListener('click', () => { $('fSearch').value = ''; $('fPlatform').value = ''; $('fRating').value = '0'; loadSongs(); });
$('detectBtn').addEventListener('click', async () => {
  const url = $('sUrl').value.trim();
  if (!url) { $('detectMsg').textContent = 'Pega primero el enlace.'; return; }
  $('detectMsg').textContent = 'Buscando metadatos…';
  try {
    const d = await api(`/api/metadata?url=${encodeURIComponent(url)}`);
    if (!d.ok) { $('detectMsg').textContent = d.error; return; }
    $('sTitle').value = d.title || ''; $('sArtist').value = d.artist || '';
    $('detectMsg').textContent = `Detectado: ${d.title} — ${d.artist} (${d.platform}).`;
    $('addSongBtn').dataset.thumb = d.thumbnail_url || '';
    $('addSongBtn').dataset.platform = d.platform || '';
  } catch (e) { $('detectMsg').textContent = e.message; }
});
$('addSongBtn').addEventListener('click', async () => {
  const platform = $('addSongBtn').dataset.platform || (/spotify/.test($('sUrl').value) ? 'spotify' : 'youtube');
  try {
    await api('/api/songs', { method: 'POST', body: JSON.stringify({
      title: $('sTitle').value, artist: $('sArtist').value, platform,
      url: $('sUrl').value, thumbnail_url: $('addSongBtn').dataset.thumb || '',
      genre: $('sGenre').value, language: $('sLang').value, difficulty: $('sDiff').value,
      status: $('sStatus').value, rating: Number($('sRating').value || 0),
      is_favorite: $('sFav').checked, notes: $('sNotes').value,
    }) });
    $('addMsg').textContent = 'Canción agregada ✅';
    ['sUrl', 'sTitle', 'sArtist', 'sGenre', 'sLang', 'sNotes'].forEach((id) => { $(id).value = ''; });
    delete $('addSongBtn').dataset.thumb; delete $('addSongBtn').dataset.platform;
    await loadSongs();
  } catch (e) { $('addMsg').textContent = e.message; }
});

// Cola: siguiente (admin)
$('nextBtn').addEventListener('click', async () => {
  try { const d = await api('/api/queue/next', { method: 'POST' }); await loadQueue(); await loadPlayer(); if (!d.song) alert('La cola está vacía.'); }
  catch (e) { alert(e.message); }
});
$('shuffleBtn').addEventListener('click', shuffleSing);

// Solicitudes
$('reqBtn').addEventListener('click', async () => {
  try {
    await api('/api/requests', { method: 'POST', body: JSON.stringify({
      video_url: $('rUrl').value, title: $('rTitle').value, artist: $('rArtist').value,
      difficulty: $('rDiff').value, language: $('rLang').value, notes: $('rNotes').value,
    }) });
    $('reqMsg').textContent = 'Solicitud enviada ✅ El admin la verá y generará el vídeo después.';
    ['rUrl', 'rTitle', 'rArtist', 'rLang', 'rNotes'].forEach((id) => { $(id).value = ''; });
    await loadRequests();
  } catch (e) { $('reqMsg').textContent = e.message; }
});

// Admin: vídeo local
$('addLocalBtn').addEventListener('click', async () => {
  try {
    await api('/api/songs', { method: 'POST', body: JSON.stringify({
      title: $('lTitle').value, artist: $('lArtist').value, platform: 'local',
      local_path: $('lPath').value, genre: $('lGenre').value, language: $('lLang').value,
      difficulty: $('lDiff').value, status: 'want', rating: 0, lyrics_text: $('lLyrics').value,
    }) });
    $('localMsg').textContent = 'Vídeo local registrado ✅ (enlazado, no copiado).';
    ['lPath', 'lTitle', 'lArtist', 'lGenre', 'lLang', 'lLyrics'].forEach((id) => { $(id).value = ''; });
    await loadSongs();
  } catch (e) { $('localMsg').textContent = e.message; }
});

// PWA: service worker (solo si el navegador lo soporta) e instalación.
// El service worker solo cachea la interfaz; la API y el vídeo nunca se cachean
// (ver public/sw.js). La PWA no sirve el backend: el servidor debe estar encendido.
let deferredInstallPrompt = null;
if ('serviceWorker' in navigator) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').catch(() => { /* sin PWA offline, la app sigue funcionando online */ });
  });
}
window.addEventListener('beforeinstallprompt', (e) => {
  e.preventDefault();
  deferredInstallPrompt = e;
  $('installBtn').classList.remove('hidden');
  $('installMsg').textContent = 'Puedes instalar KaraokeMuse como app en este dispositivo.';
});
$('installBtn').addEventListener('click', async () => {
  if (!deferredInstallPrompt) return;
  deferredInstallPrompt.prompt();
  const choice = await deferredInstallPrompt.userChoice.catch(() => null);
  if (choice && choice.outcome === 'accepted') $('installMsg').textContent = 'Instalando KaraokeMuse como app…';
  deferredInstallPrompt = null;
  $('installBtn').classList.add('hidden');
});
if (window.matchMedia && window.matchMedia('(display-mode: standalone)').matches) {
  $('installMsg').textContent = 'Abierta como app instalada 📲 (sigue necesitando el servidor encendido).';
}

// Arranque: ?room=XXXX prellena el código; sesión guardada si existe
(function init() {
  const params = new URLSearchParams(location.search);
  if (params.get('room')) $('joinCode').value = params.get('room').toUpperCase();
  if (!token) return;
  api('/api/me').then(async (d) => {
    me = d.user;
    room = await api('/api/room');
    showApp();
  }).catch(() => { token = ''; localStorage.removeItem('km_token'); });
})();
