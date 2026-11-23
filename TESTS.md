# Pruebas de KaraokeMuse v1

Verificación en vivo contra el servidor real en esta VM (Node 24.22.1),
no solo revisión de código.

## Cómo se corrió

```bash
cd ~/workspace/KaraokeMuse
PORT=8081 KARAOKE_ADMIN_USER=admin KARAOKE_ADMIN_PASSWORD='AdminTest123!' node server/index.js
# Sala creada: código CYMRFJ (aleatorio en cada base nueva)
python3 /tmp/km-test.py   # script de pruebas (35 checks, /tmp, no forma parte del repo)
```

El script cubre: salud, frontend estático, QR vendored, 401 sin token,
login admin bueno/malo, unirse con código bueno/malo, alta de enlaces
YouTube y Spotify como invitado, edición como invitado → 403, registro
de vídeo local como invitado → 403, borrar canción ajena → 403, borrar
propia → 200, registro de vídeo local como admin (existe → 201,
inexistente → 400), streaming por rangos (206) y completo (200),
cola (agregar, quitar ajena → 403, siguiente como invitado → 403,
siguiente como admin → 200 y Player actualizado), aleatorias
(`/api/songs/random` devuelve 3 distintas), solicitudes (crear,
invitado cambia estado → 403, admin cambia a `in_progress`, invitado
solo ve las suyas, admin ve todas) y filtro `minRating=5`.

## Resultado

**35/35 pruebas pasaron** en base de datos limpia (segunda corrida,
tras corregir dos expectativas del propio script: el archivo de vídeo
de prueba mide 42 bytes, no 41, y los bytes 0–9 son `fake-mp4-c`).

## Extras verificados aparte

- **QR vendored:** `public/vendor/qrcode.min.js` (qrcode-generator
  1.4.4) genera un QR real en Node: 29×29 módulos, SVG de 11.778
  caracteres para `http://localhost:8081/?room=CYMRFJ`.
- **oEmbed real:** `GET /api/metadata?url=https://www.youtube.com/
  watch?v=dQw4w9WgXcQ` devolvió título real ("Rick Astley - Never
  Gonna Give You Up (Official Video) (4K Remaster)"), artista
  "Rick Astley" y miniatura `i.ytimg.com`. El fallback manual
  (escribir título/artista a mano) existe para cuando no hay red.

## PWA y Android (2026-11-20)

Verificación en vivo contra el servidor real en esta VM (Node 24.22.1),
no solo revisión de código.

```bash
cd ~/workspace/KaraokeMuse
node --check public/app.js && node --check public/sw.js && node --check server/index.js   # syntax OK
node -e "require('node:sqlite')"   # OK en Node v24.22.1 x64 (VM; ARM64/Termux NO probado)
python3 -c "import json; json.load(open('public/manifest.webmanifest'))"  # JSON válido
PORT=8082 DATA_DIR=/tmp/km-pwa-data KARAOKE_ADMIN_PASSWORD='PwaTest123!' node server/index.js
# Sala creada: código WB2W54 (aleatorio en cada base nueva)
```

Resultados (curl, servidor en 8082):

| Comprobación | Resultado |
|---|---|
| `GET /manifest.webmanifest` | **200, `application/manifest+json`**; JSON válido: nombre KaraokeMuse, `display: standalone`, iconos 192×192 y 512×512 |
| `GET /sw.js` | **200, `text/javascript; charset=utf-8`** |
| `GET /icons/icon-192.png` y `/icons/icon-512.png` | **200, `image/png`**; PNG reales generados con PIL (192×192 y 512×512, micrófono dorado sobre fondo oscuro) |
| `index.html` | referencia `manifest.webmanifest`, `theme-color`, `apple-mobile-web-app-capable` y el botón `#installBtn` |
| `sw.js` no cachea la API | el handler `fetch` hace `return` temprano si el path empieza por `/api/` (línea 38 del archivo servido); la interfaz cacheada es solo estática |
| `GET /api/health` | `{"ok":true,"app":"KaraokeMuse","version":"1.0.0"}` — la API sigue viva tras los cambios |
| Registro del SW y `beforeinstallprompt` | presentes en `app.js` (registro solo si `serviceWorker` existe; botón oculto hasta que el navegador dispare el evento) |

El servidor de pruebas se paró al terminar; la base de pruebas quedó
en `/tmp/km-pwa-data` (no forma parte del repo).

## No probado en vivo (pendiente)

- Interacción en el navegador (clics en la UI, escaneo real del QR
  con el móvil, reproducción de un MP4 real en el Player): el
  servidor y la API están verificados; la UI es HTML/JS estático
  servido por ese mismo servidor y su sintaxis fue validada con
  `node --check`, pero no se abrió en un navegador en esta VM.
- **PWA en un teléfono real: NO probado.** Instalación en Chrome
  Android (botón 📲 Instalar app / Agregar a pantalla principal),
  apertura en modo `standalone`, funcionamiento offline de la
  interfaz y escaneo del QR desde la PWA instalada. El service
  worker solo se verificó por código servido y sintaxis, no con un
  navegador que lo registre.
- **Termux en el teléfono: NO probado.** `node:sqlite` en
  Termux/ARM64 con Bionic, batería durante una sesión, que Android
  no mate el proceso (phantom process killer) y exposición del
  puerto en la Wi-Fi del teléfono. Ver README, Opción 2.
- **Wrapper nativo (Capacitor/Tauri/WebView): NO construido.**
  Solo evaluación y plan en el README; los builds Android van por
  GitHub Actions porque Gradle no corre en esta VM.
- Spotify oEmbed en vivo (solo se probó YouTube; el código usa el
  mismo camino con `open.spotify.com/oembed`).
- Arranque en Windows ni en Termux/Android: solo se probó en
  esta VM Linux con Node 24.22.1.
- Vídeo real: el streaming se probó con un archivo de prueba de
  42 bytes, no con un MP4 de karaoke real.
