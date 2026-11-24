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
    // enablejsapi=1: el botón Pausa del Player controla el vídeo por postMessage.
    return id ? `https://www.youtube.com/embed/${id}?autoplay=1&rel=0&enablejsapi=1` : null;
  }
  if (song.platform === 'spotify') {
    // Acepta el prefijo de idioma que Spotify añade al compartir
    // (ej. open.spotify.com/intl-es/track/…), que antes no embebía.
    const m = String(song.url || '').match(/open\.spotify\.com\/(?:(?:intl-[a-z]{2}|[a-z]{2}(?:-[a-z]{2})?)\/)?(track|album|playlist|episode)\/([\w]+)/);
    return m ? `https://open.spotify.com/embed/${m[1]}/${m[2]}?utm_source=generator` : null;
  }
  return null;
}
function platformLabel(p) { return p === 'youtube' ? '▶️ YouTube' : p === 'spotify' ? '🎧 Spotify' : '💾 Vídeo local'; }

// URL de una lista de YouTube (/playlist?list=…) → su id, o null.
// Un watch?v=…&list=… es un solo vídeo con lista, no una lista.
function playlistIdFromUrl(url) {
  try {
    const u = new URL(String(url));
    if (!/(?:www\.|music\.)?youtube\.com$/i.test(u.hostname)) return null;
    if (u.pathname !== '/playlist') return null;
    return u.searchParams.get('list') || null;
  } catch (e) { return null; }
}
function spotifyUri(song) {
  const m = String(song.url || '').match(/open\.spotify\.com\/(?:(?:intl-[a-z]{2}|[a-z]{2}(?:-[a-z]{2})?)\/)?(track|album|playlist|episode)\/([\w]+)/);
  return m ? `spotify:${m[1]}:${m[2]}` : null;
}

function showApp() {
  $('authView').classList.add('hidden');
  $('appView').classList.remove('hidden');
  $('sessionBar').classList.remove('hidden');
  $('sessionInfo').textContent = `${me.username} (${me.role === 'admin' ? 'admin' : 'invitado'}) · Sala ${room ? room.name : ''}`;
  $('adminTab').style.display = me.role === 'admin' ? '' : 'none';
  $('prevBtn').style.display = me.role === 'admin' ? '' : 'none';
  $('playerNextBtn').style.display = me.role === 'admin' ? '' : 'none';
  if (me.role !== 'admin') switchTab('library');
  drawQR();
  refreshAll();
}
function showAuth() {
  $('authView').classList.remove('hidden');
  $('appView').classList.add('hidden');
  $('sessionBar').classList.add('hidden');
}
async function drawQR() {
  if (!room) return;
  $('roomName').textContent = room.name;
  $('roomCode').textContent = room.code;
  // Si la página se abrió como localhost, el QR llevaría localhost y
  // el móvil se buscaría a sí mismo. En ese caso se usa la IP LAN del
  // servidor (viene de /api/server-info) para que el QR funcione.
  let origin = location.origin;
  let lanNote = '';
  const host = location.hostname;
  if (host === 'localhost' || host === '127.0.0.1') {
    try {
      const info = await api('/api/server-info');
      if (info.lan_url) {
        origin = info.lan_url;
        lanNote = `QR con la IP local de la PC (${info.lan_ip}). El teléfono y la PC deben estar en el mismo Wi-Fi.`;
      }
    } catch (e) { /* sin IP LAN, el QR usa localhost como antes */ }
  }
  const joinUrl = `${origin}/?room=${room.code}`;
  $('qrLanNote').textContent = lanNote;
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
  if (name === 'admin') { loadAdminRoom(); loadAdminRequests(); }
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
  $('songList').innerHTML = songs.length ? songs.map(songCard).join('') : '<p>No hay canciones todavía. Agrega la primera con el formulario de arriba.</p>';
  updateBulkBar();
}
let selected = new Set();
function songCard(s) {
  const thumb = s.thumbnail_url ? `<img src="${esc(s.thumbnail_url)}" alt="">` : '<img alt="">';
  const canEdit = me && me.role === 'admin';
  return `<div class="song"><input type="checkbox" class="sel" ${selected.has(s.id) ? 'checked' : ''} onchange="toggleSelect('${s.id}', this.checked)">${thumb}<div class="info"><b>${esc(s.title)}</b>
    <div class="meta">${esc(s.artist || 'Sin artista')} · ${platformLabel(s.platform)} · ${esc(s.genre || '')} ${esc(s.language || '')} · ${esc(s.difficulty)} · ${stars(s.rating)}${s.is_favorite ? ' ⭐' : ''}</div>
    <div class="meta">${esc(s.status === 'want' ? 'Quiero aprenderla' : s.status === 'practicing' ? 'En práctica' : 'La domino')}</div></div>
    <div class="actions">
      <button class="btn small" onclick="addToQueue('${s.id}')">+ Cola</button>
      ${canEdit ? `<button class="btn small" onclick="editSong('${s.id}')">✏️ Editar</button>` : ''}
      ${s.url ? `<a class="btn small" href="${esc(s.url)}" target="_blank" rel="noopener">Abrir</a>` : ''}
      <button class="btn small" onclick="deleteSong('${s.id}')">🗑 Borrar</button>
    </div></div>`;
}
window.toggleSelect = (id, on) => {
  if (on) selected.add(id); else selected.delete(id);
  updateBulkBar();
};
function updateBulkBar() {
  const n = selected.size;
  $('bulkBar').classList.toggle('hidden', n === 0);
  $('bulkCount').textContent = n ? `${n} seleccionada${n > 1 ? 's' : ''}` : '';
  const all = $('selAll');
  if (all) all.checked = songs.length > 0 && songs.every((s) => selected.has(s.id));
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

let currentSong = null;
let ytPaused = false;
let ytPlayer = null;
let spotifyController = null;
let spotifyEndedFired = false;
let playerSeq = 0;

function isPlayerFullscreen() {
  const fs = document.fullscreenElement;
  return !!(fs && $('tab-player').contains(fs));
}
// Avance automático (ADR-025): solo en pantalla completa y solo en
// la pantalla del admin (la cola solo la avanza el admin).
function maybeAutoAdvance() {
  if (me && me.role === 'admin' && isPlayerFullscreen()) nextSong();
}
function destroyPlayers() {
  if (ytPlayer) { try { ytPlayer.destroy(); } catch (e) {} ytPlayer = null; }
  if (spotifyController) { try { spotifyController.destroy(); } catch (e) {} spotifyController = null; }
  spotifyEndedFired = false;
  ytPaused = false;
}
// Carga perezosa de la API de YouTube (para el evento de fin de vídeo).
let ytApiPromise = null;
function ytApiReady() {
  if (window.YT && window.YT.Player) return Promise.resolve(true);
  if (!ytApiPromise) {
    ytApiPromise = new Promise((resolve) => {
      const timer = setTimeout(() => resolve(false), 8000);
      const prev = window.onYouTubeIframeAPIReady;
      window.onYouTubeIframeAPIReady = () => { if (prev) prev(); clearTimeout(timer); resolve(true); };
      const tag = document.createElement('script');
      tag.src = 'https://www.youtube.com/iframe_api';
      tag.onerror = () => { clearTimeout(timer); resolve(false); };
      document.head.appendChild(tag);
    });
  }
  return ytApiPromise;
}
// Carga perezosa de la API de Spotify (para saber cuándo termina la
// canción: el embed normal no avisa). Solo se escucha: nunca se llama
// a play() del controlador, porque eso fuerza la muestra de 30 s
// incluso con Premium (ver ADR-023).
let spotifyApiPromise = null;
let spotifyApi = null;
function spotifyApiReady() {
  if (spotifyApi) return Promise.resolve(true);
  if (!spotifyApiPromise) {
    spotifyApiPromise = new Promise((resolve) => {
      const timer = setTimeout(() => resolve(false), 8000);
      window.onSpotifyIframeApiReady = (IFrameAPI) => { spotifyApi = IFrameAPI; clearTimeout(timer); resolve(true); };
      const tag = document.createElement('script');
      tag.src = 'https://open.spotify.com/embed/iframe-api/v1';
      tag.onerror = () => { clearTimeout(timer); resolve(false); };
      document.head.appendChild(tag);
    });
  }
  return spotifyApiPromise;
}
async function loadPlayer() {
  const d = await api('/api/player');
  const box = $('playerBox');
  currentSong = d.song;
  const mySeq = ++playerSeq;
  destroyPlayers();
  const pauseBtn = $('pauseBtn');
  if (pauseBtn) pauseBtn.textContent = '⏸ Pausa';
  if (!d.song) { box.innerHTML = '<p>Aún no hay canción sonando. Pide la siguiente desde la Cola.</p>'; $('lyricsBox').classList.add('hidden'); return; }
  const s = d.song;
  box.innerHTML = `<h3>${esc(s.title)} — ${esc(s.artist || '')}</h3>`;
  if (s.platform === 'local') {
    box.innerHTML += `<video src="/api/media/local/${s.id}?token=${encodeURIComponent(token)}" controls autoplay></video>`;
    const v = box.querySelector('video');
    if (v) v.addEventListener('ended', maybeAutoAdvance);
  } else if (s.platform === 'youtube') {
    const id = youtubeId(s.url);
    if (!id) {
      box.innerHTML += `<p><a href="${esc(s.url)}" target="_blank" rel="noopener">Abrir en YouTube</a> (no se pudo embeber este enlace).</p>`;
    } else if (await ytApiReady()) {
      if (mySeq !== playerSeq) return;
      box.innerHTML += '<div id="ytPlayer"></div>';
      ytPlayer = new YT.Player('ytPlayer', {
        videoId: id,
        width: '100%',
        height: '100%',
        playerVars: { autoplay: 1, rel: 0, playsinline: 1 },
        events: { onStateChange: (e) => { if (e.data === YT.PlayerState.ENDED) maybeAutoAdvance(); } },
      });
    } else {
      // Sin la API de YouTube (sin red para el script): embed simple
      // como antes; en este caso no hay avance automático.
      if (mySeq !== playerSeq) return;
      box.innerHTML += `<iframe data-platform="youtube" src="${esc(embedUrl(s))}" allow="autoplay; clipboard-write; encrypted-media; fullscreen; picture-in-picture" allowfullscreen></iframe>`;
    }
  } else if (s.platform === 'spotify') {
    const uri = spotifyUri(s);
    if (!uri) {
      box.innerHTML += `<p><a href="${esc(s.url)}" target="_blank" rel="noopener">Abrir en Spotify</a> (no se pudo embeber este enlace).</p>`;
    } else if (await spotifyApiReady()) {
      if (mySeq !== playerSeq) return;
      box.innerHTML += '<div id="spPlayer"></div>';
      spotifyApi.createController(document.getElementById('spPlayer'), { uri, width: '100%', height: 352 }, (controller) => {
        if (mySeq !== playerSeq) { try { controller.destroy(); } catch (e) {} return; }
        spotifyController = controller;
        controller.addListener('playback_update', (e) => {
          const p = e && e.data;
          if (!p || !p.duration || p.duration <= 0) return;
          if (!spotifyEndedFired && p.position >= p.duration - 1500) {
            spotifyEndedFired = true;
            maybeAutoAdvance();
          }
        });
      });
    } else {
      if (mySeq !== playerSeq) return;
      box.innerHTML += `<iframe data-platform="spotify" src="${esc(embedUrl(s))}" allow="autoplay; clipboard-write; encrypted-media; fullscreen; picture-in-picture" allowfullscreen></iframe>`;
    }
  }
  if (s.lyrics_text) {
    $('lyricsBox').classList.remove('hidden');
    $('lyricsText').textContent = s.lyrics_text;
    $('lyricsNote').textContent = '';
  } else if (s.platform === 'spotify') {
    // ADR-024: el embed de Spotify no trae letra y no se puede
    // modificar (iframe de otro origen). Se busca la letra en LRCLIB
    // (gratis, sin clave) y se muestra sin sincronizar: el embed no
    // comparte el tiempo de reproducción, así que no se puede
    // resaltar la línea actual.
    $('lyricsBox').classList.remove('hidden');
    $('lyricsText').textContent = 'Buscando letra…';
    $('lyricsNote').textContent = '';
    showSpotifyLyrics(s);
  } else $('lyricsBox').classList.add('hidden');
}

// Letra de Spotify vía LRCLIB, con caché por canción. Solo se pinta
// si la canción que suena sigue siendo la misma (la búsqueda es
// asíncrona y la cola puede avanzar mientras tanto).
const spotifyLyricsCache = {};
async function fetchSpotifyLyrics(song) {
  const title = (song.title || '').trim();
  const artist = (song.artist || '').trim();
  if (!title) return { found: false };
  try {
    let data = null;
    if (artist) {
      const r = await fetch(`https://lrclib.net/api/get?artist_name=${encodeURIComponent(artist)}&track_name=${encodeURIComponent(title)}`);
      if (r.ok) data = await r.json();
    }
    if (!data || !data.plainLyrics) {
      const r = await fetch(`https://lrclib.net/api/search?track_name=${encodeURIComponent(title)}${artist ? `&artist_name=${encodeURIComponent(artist)}` : ''}`);
      if (r.ok) {
        const list = await r.json();
        data = (list || []).find((x) => x && x.plainLyrics) || null;
      }
    }
    return data && data.plainLyrics ? { found: true, text: data.plainLyrics } : { found: false };
  } catch (e) { return { found: false }; }
}
async function showSpotifyLyrics(song) {
  const songId = song.id;
  let result = spotifyLyricsCache[songId];
  if (!result) {
    result = await fetchSpotifyLyrics(song);
    spotifyLyricsCache[songId] = result;
  }
  if (!currentSong || currentSong.id !== songId) return;
  if (result.found) {
    $('lyricsText').textContent = result.text;
    $('lyricsNote').textContent = 'Letra sin sincronizar (fuente: LRCLIB). El reproductor de Spotify no comparte el tiempo de reproducción.';
  } else {
    $('lyricsText').textContent = 'No se encontró la letra de esta canción.';
    $('lyricsNote').textContent = 'La reproducción de Spotify es solo para escuchar la canción; el karaoke con letra sincronizada es para vídeos locales.';
  }
}

// Controles del Player: anterior / pausa / siguiente (admin).
// Pausa actúa sobre el vídeo o el iframe de YouTube de ESTE
// dispositivo; Spotify no expone control por JS, se pausa con su
// propio botón dentro del reproductor embebido.
async function nextSong() {
  try {
    const d = await api('/api/queue/next', { method: 'POST' });
    await loadQueue(); await loadPlayer();
    $('playerMsg').textContent = d.song ? '' : 'La cola está vacía.';
  } catch (e) { $('playerMsg').textContent = e.message; }
}
$('prevBtn').addEventListener('click', async () => {
  try {
    const d = await api('/api/queue/previous', { method: 'POST' });
    await loadQueue(); await loadPlayer();
    $('playerMsg').textContent = d.song ? '' : 'No hay canción anterior.';
  } catch (e) { $('playerMsg').textContent = e.message; }
});
$('pauseBtn').addEventListener('click', () => {
  const btn = $('pauseBtn');
  if (ytPlayer) {
    if (ytPaused) { ytPlayer.playVideo(); ytPaused = false; btn.textContent = '⏸ Pausa'; }
    else { ytPlayer.pauseVideo(); ytPaused = true; btn.textContent = '▶️ Seguir'; }
    return;
  }
  const video = document.querySelector('#playerBox video');
  const frame = document.querySelector('#playerBox iframe');
  if (video) {
    if (video.paused) video.play(); else video.pause();
    btn.textContent = video.paused ? '▶️ Seguir' : '⏸ Pausa';
    return;
  }
  if (frame && currentSong && currentSong.platform === 'youtube') {
    frame.contentWindow.postMessage(JSON.stringify({ event: 'command', func: ytPaused ? 'play' : 'pause', args: '' }), '*');
    ytPaused = !ytPaused;
    btn.textContent = ytPaused ? '▶️ Seguir' : '⏸ Pausa';
    return;
  }
  if (frame) {
    $('playerMsg').textContent = 'Spotify se pausa con su propio control dentro del reproductor.';
    return;
  }
  $('playerMsg').textContent = 'No hay nada sonando.';
});
$('playerNextBtn').addEventListener('click', nextSong);

// Pantalla completa del Player (ADR-025): a pantalla completa va
// toda la pestaña, así el avance automático sigue de canción en
// canción sin salirse de pantalla completa.
$('fsBtn').addEventListener('click', async () => {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await $('tab-player').requestFullscreen();
  } catch (e) { $('playerMsg').textContent = 'No se pudo entrar a pantalla completa en este navegador.'; }
});
document.addEventListener('fullscreenchange', () => {
  const b = $('fsBtn');
  if (b) b.textContent = document.fullscreenElement ? '⛶ Salir de pantalla completa' : '⛶ Pantalla completa';
});

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
      ${playlistIdFromUrl(r.video_url) ? `<button class="btn small" onclick="importRequestList('${r.id}', '${esc(r.video_url)}')">📋 Importar lista</button>` : ''}
      </div></div>`).join('')
      : '<p>No hay solicitudes.</p>';
  } catch (e) { $('adminReqList').innerHTML = `<p>${esc(e.message)}</p>`; }
}
window.setReqStatus = async (id, status) => { try { await api(`/api/requests/${id}`, { method: 'PATCH', body: JSON.stringify({ status }) }); } catch (e) { alert(e.message); } };
window.setReqNote = async (id, note) => { try { await api(`/api/requests/${id}`, { method: 'PATCH', body: JSON.stringify({ admin_note: note }) }); } catch (e) { alert(e.message); } };
// Importar la lista que pidió un invitado (ADR-025).
window.importRequestList = async (id, url) => {
  try {
    const d = await api('/api/playlists/import', { method: 'POST', body: JSON.stringify({ url }) });
    alert(`Lista «${d.playlist_title || 'YouTube'}»: ${d.added} agregadas${d.skipped ? ` (${d.skipped} ya estaban)` : ''}.`);
    await loadAdminRequests(); await loadSongs();
  } catch (e) { alert(e.message); }
};

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

// Biblioteca: filtros, alta y edición
['fSearch', 'fPlatform', 'fRating'].forEach((id) => $(id).addEventListener('input', () => loadSongs().catch(() => {})));
$('fClear').addEventListener('click', () => { $('fSearch').value = ''; $('fPlatform').value = ''; $('fRating').value = '0'; loadSongs(); });

// Edición de canciones (admin): carga la canción en el formulario y
// el botón Agregar pasa a guardar los cambios (PATCH).
let editingId = null;
let editingPlatform = null;
let editingLocalPath = '';
window.editSong = (id) => {
  const s = songs.find((x) => x.id === id);
  if (!s) return;
  editingId = id;
  editingPlatform = s.platform;
  editingLocalPath = s.local_path || '';
  $('sUrl').value = s.platform === 'local' ? (s.local_path || '') : (s.url || '');
  $('sTitle').value = s.title || '';
  $('sArtist').value = s.artist || '';
  $('sGenre').value = s.genre || '';
  $('sLang').value = s.language || '';
  $('sDiff').value = s.difficulty || 'media';
  $('sStatus').value = s.status || 'want';
  $('sRating').value = s.rating || 0;
  $('sFav').checked = !!s.is_favorite;
  $('addSongBtn').dataset.platform = s.platform;
  $('addSongBtn').dataset.thumb = s.thumbnail_url || '';
  $('addSongBtn').textContent = '💾 Guardar cambios';
  $('cancelEditBtn').classList.remove('hidden');
  $('addMsg').textContent = `Editando "${s.title}".`;
  window.scrollTo({ top: 0, behavior: 'smooth' });
};
function resetEditMode() {
  editingId = null; editingPlatform = null; editingLocalPath = '';
  $('addSongBtn').textContent = 'Agregar canción';
  $('cancelEditBtn').classList.add('hidden');
  delete $('addSongBtn').dataset.thumb; delete $('addSongBtn').dataset.platform;
}
$('cancelEditBtn').addEventListener('click', () => {
  resetEditMode();
  ['sUrl', 'sTitle', 'sArtist', 'sGenre', 'sLang'].forEach((id) => { $(id).value = ''; });
  $('sRating').value = '0'; $('sFav').checked = false;
  $('addMsg').textContent = '';
});

// Selección masiva del repertorio: agregar a la cola o borrar varias.
$('selAll').addEventListener('change', (e) => {
  songs.forEach((s) => { if (e.target.checked) selected.add(s.id); else selected.delete(s.id); });
  loadSongs();
});
$('bulkQueueBtn').addEventListener('click', async () => {
  let ok = 0; let fail = 0;
  for (const id of [...selected]) {
    try { await api('/api/queue', { method: 'POST', body: JSON.stringify({ songId: id }) }); ok++; }
    catch (e) { fail++; }
  }
  selected.clear(); updateBulkBar();
  await loadQueue(); await loadSongs();
  alert(`${ok} en la cola 🎶${fail ? ` (${fail} fallaron)` : ''}`);
});
$('bulkDeleteBtn').addEventListener('click', async () => {
  if (!selected.size || !confirm(`¿Borrar ${selected.size} canciones del repertorio?`)) return;
  let ok = 0; let fail = 0;
  for (const id of [...selected]) {
    try { await api(`/api/songs/${id}`, { method: 'DELETE' }); ok++; }
    catch (e) { fail++; }
  }
  selected.clear(); updateBulkBar();
  await loadSongs(); await loadQueue();
  if (fail) alert(`${ok} borradas, ${fail} no se pudieron borrar (solo el admin borra canciones de otros).`);
});
$('detectBtn').addEventListener('click', async () => {
  const url = $('sUrl').value.trim();
  if (!url) { $('detectMsg').textContent = 'Pega primero el enlace.'; return; }
  // ADR-025: una lista no es una canción; no se detecta por oEmbed.
  if (playlistIdFromUrl(url)) {
    $('detectMsg').textContent = me && me.role === 'admin'
      ? 'Es una lista de YouTube: usa «Importar lista completa» para agregar todos sus vídeos.'
      : 'Solicita la lista en la pestaña Pedir video';
    updateImportBtn();
    return;
  }
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
  // ADR-025: una lista no se agrega como canción.
  if (playlistIdFromUrl($('sUrl').value)) {
    $('addMsg').textContent = me && me.role === 'admin'
      ? 'Esa URL es una lista: usa «Importar lista completa».'
      : 'Solicita la lista en la pestaña Pedir video';
    return;
  }
  const platform = editingPlatform || $('addSongBtn').dataset.platform || (/spotify/.test($('sUrl').value) ? 'spotify' : 'youtube');
  const body = {
    title: $('sTitle').value, artist: $('sArtist').value, platform,
    url: platform === 'local' ? '' : $('sUrl').value,
    local_path: platform === 'local' ? ($('sUrl').value || editingLocalPath) : '',
    thumbnail_url: $('addSongBtn').dataset.thumb || '',
    genre: $('sGenre').value, language: $('sLang').value, difficulty: $('sDiff').value,
    status: $('sStatus').value, rating: Number($('sRating').value || 0),
    is_favorite: $('sFav').checked,
  };
  try {
    if (editingId) {
      await api(`/api/songs/${editingId}`, { method: 'PATCH', body: JSON.stringify(body) });
      $('addMsg').textContent = 'Cambios guardados ✅';
      resetEditMode();
    } else {
      await api('/api/songs', { method: 'POST', body: JSON.stringify(body) });
      $('addMsg').textContent = 'Canción agregada ✅';
    }
    ['sUrl', 'sTitle', 'sArtist', 'sGenre', 'sLang'].forEach((id) => { $(id).value = ''; });
    $('sRating').value = '0'; $('sFav').checked = false;
    delete $('addSongBtn').dataset.thumb; delete $('addSongBtn').dataset.platform;
    await loadSongs();
  } catch (e) { $('addMsg').textContent = e.message; }
});

// Importar una lista completa de YouTube (ADR-025, solo admin).
function updateImportBtn() {
  const show = !!(me && me.role === 'admin' && playlistIdFromUrl($('sUrl').value.trim()) && !editingId);
  $('importListBtn').classList.toggle('hidden', !show);
  if (!show) $('importMsg').textContent = '';
}
$('sUrl').addEventListener('input', updateImportBtn);
$('importListBtn').addEventListener('click', async () => {
  const url = $('sUrl').value.trim();
  $('importMsg').textContent = 'Importando lista…';
  try {
    const d = await api('/api/playlists/import', { method: 'POST', body: JSON.stringify({ url }) });
    $('importMsg').textContent = `Lista «${d.playlist_title || 'YouTube'}»: ${d.added} agregadas${d.skipped ? ` (${d.skipped} ya estaban en el repertorio)` : ''}.`;
    $('sUrl').value = '';
    updateImportBtn();
    await loadSongs();
  } catch (e) { $('importMsg').textContent = e.message; }
});

// Cola: siguiente (admin)
$('nextBtn').addEventListener('click', nextSong);
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

// Admin: sala
function loadAdminRoom() {
  if (room) $('roomNameInput').value = room.name || '';
}
$('saveRoomBtn').addEventListener('click', async () => {
  try {
    const body = { name: $('roomNameInput').value };
    if ($('roomPassClear').checked) body.password = '';
    else if ($('roomPassInput').value) body.password = $('roomPassInput').value;
    const d = await api('/api/room', { method: 'PATCH', body: JSON.stringify(body) });
    room = d;
    $('roomMsg').textContent = 'Sala guardada ✅';
    $('roomPassInput').value = ''; $('roomPassClear').checked = false;
    drawQR();
  } catch (e) { $('roomMsg').textContent = e.message; }
});
$('deleteRoomBtn').addEventListener('click', async () => {
  if (!confirm('¿Eliminar la sala? Se crea una nueva con código nuevo, la cola se vacía y el repertorio se conserva. Todos tendrán que entrar con el código nuevo.')) return;
  try {
    const d = await api('/api/room', { method: 'DELETE' });
    room = d;
    $('roomMsg').textContent = `Sala nueva creada. Código: ${d.code}`;
    drawQR();
  } catch (e) { $('roomMsg').textContent = e.message; }
});
$('changePassBtn').addEventListener('click', async () => {
  try {
    await api('/api/admin/password', { method: 'PATCH', body: JSON.stringify({ current_password: $('adminCurPass').value, new_password: $('adminNewPass').value }) });
    $('passMsg').textContent = 'Contraseña cambiada ✅';
    $('adminCurPass').value = ''; $('adminNewPass').value = '';
  } catch (e) { $('passMsg').textContent = e.message; }
});

// Admin: explorar carpetas del servidor para elegir el vídeo local.
// El navegador no puede dar la ruta completa de un archivo (seguridad),
// así que el servidor lista la carpeta y al hacer clic se llena la ruta.
$('browseBtn').addEventListener('click', async () => {
  const dir = $('lBrowseDir').value.trim();
  if (!dir) { $('localMsg').textContent = 'Escribe primero la carpeta en el servidor.'; return; }
  try {
    const d = await api(`/api/admin/browse?dir=${encodeURIComponent(dir)}`);
    $('browseList').innerHTML = d.entries.length ? d.entries.map((e) => `
      <button class="btn small browse-item" data-path="${esc(e.path)}" data-dir="${e.is_dir ? '1' : ''}">${e.is_dir ? '📁' : '🎬'} ${esc(e.name)}</button>`).join('') : '<p class="small">Carpeta vacía o sin vídeos.</p>';
    document.querySelectorAll('.browse-item').forEach((b) => b.addEventListener('click', () => {
      if (b.dataset.dir === '1') {
        $('lBrowseDir').value = b.dataset.path;
        $('browseBtn').click();
      } else {
        $('lPath').value = b.dataset.path;
        if (!$('lTitle').value) $('lTitle').value = b.dataset.path.split(/[\\/]/).pop().replace(/\.[^.]+$/, '').replace(/[_-]+/g, ' ');
        $('localMsg').textContent = 'Ruta elegida ✅ Revisa el título y registra el vídeo.';
      }
    }));
  } catch (e) { $('localMsg').textContent = e.message; }
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
