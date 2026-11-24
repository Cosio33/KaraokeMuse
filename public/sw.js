// KaraokeMuse — service worker de la PWA.
// Solo cachea los archivos estáticos de la interfaz (HTML/CSS/JS/vendor/iconos).
// NUNCA cachea la API (/api/...) ni los streams de vídeo (/api/media/...):
// la biblioteca, la cola y el Player siempre necesitan el servidor encendido.
//
// IMPORTANTE: cada vez que cambie un archivo de public/, hay que subir
// la versión de CACHE_NAME (v2, v3…). Si no, el navegador sigue sirviendo
// los archivos viejos de la caché (caché primero) aunque en el disco ya
// estén los nuevos: eso dejó la app rota en 2026-11-24 (index.html nuevo
// sin el campo Notas + app.js viejo que lo leía → "Cannot read properties
// of null (reading 'value')" al agregar enlaces, y sin ?token= ni intl-es).
const CACHE_NAME = 'karaokemuse-v3';
const PRECACHE = [
  '/',
  '/index.html',
  '/style.css',
  '/app.js',
  '/manifest.webmanifest',
  '/vendor/qrcode.min.js',
  '/icons/icon-192.png',
  '/icons/icon-512.png',
  '/icons/icon.svg',
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(PRECACHE)).then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return; // solo GET
  const url = new URL(req.url);
  if (url.origin !== self.location.origin) return; // solo mismo origen
  // API y vídeo: siempre a la red, nunca al caché.
  if (url.pathname.startsWith('/api/')) return;

  if (req.mode === 'navigate') {
    // Navegación: red primero, y si no hay red, la app cacheada.
    event.respondWith(
      fetch(req).catch(() => caches.match('/index.html')),
    );
    return;
  }

  // Archivos estáticos: caché primero, red como respaldo.
  event.respondWith(
    caches.match(req).then((cached) => {
      if (cached) return cached;
      return fetch(req).then((res) => {
        const type = res.headers.get('content-type') || '';
        if (res.ok && !type.startsWith('video/') && !type.startsWith('audio/')) {
          const copy = res.clone();
          caches.open(CACHE_NAME).then((cache) => cache.put(req, copy));
        }
        return res;
      });
    }),
  );
});
