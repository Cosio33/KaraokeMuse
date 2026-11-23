# 🎤 KaraokeMuse

Karaoke compartido autoalojado, hecho desde cero por Cosio33.

Un servidor HTML sirve la app: los invitados entran escaneando un **QR**,
agregan canciones por **enlace de YouTube o Spotify** a la **cola de la
playlist activa**, y el anfitrión reproduce en el **Player** a pantalla
completa. Los vídeos de karaoke generados (por ejemplo en el Generador de
Karaoke de Muse) se **enlazan desde archivos locales del servidor, nunca
se suben ni se copian**, para no saturar la app. Los invitados también
pueden **pedir que se genere un vídeo** con un formulario sencillo; el
admin lo genera después y lo agrega manualmente.

> **Inspiración:** el diseño está inspirado en KaraokeEternal
> (https://github.com/bhj/KaraokeEternal, RadRoot LLC, licencia ISC),
> analizado con REA antes de escribir este código. KaraokeMuse **no es
> un fork y no copia su código**: es un proyecto nuevo desde cero, más
> simple y ajustado. Ver `~/workspace/
> karaoke-eternal-analysis/INFORME.md` en la VM de Muse para el
> análisis completo.

## Requisitos

- **Node.js 24 o superior** (usa `node:sqlite`, incluido en Node 24).
  En Windows, descárgalo de https://nodejs.org (versión LTS 24).
- Sin dependencias externas: no hay `npm install`. El servidor usa
  solo el módulo `http` de Node y SQLite.
- PC de prueba: Windows — sobra para
  esto. No hace falta GPU: el servidor solo sirve archivos y la base
  de datos; el vídeo lo reproduce el navegador.

## Arranque (un solo comando)

```bat
cd KaraokeMuse
npm start
```

O directamente:

```bat
node server/index.js
```

El servidor arranca en `http://localhost:8080` (puerto configurable
con la variable `PORT`). Al arrancar muestra en la consola el código
de la sala y la URL para unirse. Los datos se guardan en `./data/
karaokemuse.sqlite` (creado solo).

## Variables de entorno

| Variable | Por defecto | Qué hace |
|---|---|---|
| `PORT` | `8080` | Puerto del servidor |
| `HOST` | `0.0.0.0` | Interfaz de escucha (en LAN, `0.0.0.0` para que otros equipos de la red lo vean) |
| `DATA_DIR` | `./data` | Carpeta de la base SQLite |
| `KARAOKE_ADMIN_USER` | `admin` | Usuario administrador |
| `KARAOKE_ADMIN_PASSWORD` | `admin123` | Contraseña del admin — **cámbiala antes de abrir el servidor a nadie** |
| `KARAOKE_ROOM_NAME` | `Sala principal` | Nombre de la sala |
| `KARAOKE_ROOM_PASSWORD` | *(vacía)* | Contraseña opcional de la sala (los invitados la escriben a mano) |

Ejemplo en Windows (PowerShell):

```powershell
$env:KARAOKE_ADMIN_PASSWORD="una-clave-larga-y-secreta"
$env:PORT="8080"
npm start
```

En el primer arranque se crean el usuario admin y la sala con un
código aleatorio de 6 caracteres (se muestra en consola y en la
pestaña del anfitrión).

## Cómo se usa

1. **Anfitrión (admin):** entra en `http://localhost:8080`, inicia
   sesión como admin y abre la pestaña **Player** en la pantalla o
   tele donde se va a cantar. Ahí se ve el QR.
2. **Invitados:** escanean el QR con el móvil (o abren la URL con
   `?room=CÓDIGO`). Entran con su nombre, sin contraseña, salvo que
   la sala tenga una — en ese caso la escriben a mano.
3. **Agregar canciones:** en **Biblioteca**, pega el enlace de YouTube
   o Spotify, pulsa *Detectar* (rellena título, artista y portada vía
   oEmbed) y guarda. La canción entra al repertorio.
4. **Cola:** desde cualquier canción, *+ Cola*. Los enlaces de
   YouTube y Spotify **sí entran en la cola activa**. El admin pulsa
   *Siguiente canción* y el Player reproduce: YouTube y Spotify con
   su reproductor oficial embebido; los vídeos locales con la etiqueta
   `<video>` desde el stream del servidor.
5. **A cantar:** sortea 3 canciones al azar del repertorio; el botón
   *Cambiar canciones* sortea otras 3.
6. **Pedir un vídeo:** en **Pedir vídeo**, el invitado deja enlace del
   vídeo original, nombre, artista, dificultad, idioma y notas. El
   admin lo ve en la pestaña Admin, lo marca pendiente / en proceso /
   hecho y deja una nota. El vídeo se genera fuera (en la VM de Muse)
   y el admin lo agrega manualmente después.

## Vídeos locales (solo admin, enlazados)

En la pestaña **Admin → Registrar vídeo local**, el admin escribe la
**ruta del archivo en el disco del servidor** (ej. en Windows:
`C:\Karaoke\videos\mi-cancion.mp4`; en Linux:
`/home/usuario/karaoke/mi-cancion.mp4`), título y artista. El servidor
comprueba que el archivo exista y lo sirve con **streaming por
rangos** (`/api/media/local/:id`). El archivo **nunca se copia ni se
sube**: si se borra del disco, deja de reproducirse. Opcionalmente se
puede pegar el contenido del `.srt`; en v1 la letra se muestra en el
Player como texto (sin resaltado sincronizado).

## Roles y permisos (se aplican en el servidor, no solo en la UI)

| Acción | Admin | Invitado |
|---|---|---|
| Ver todo (biblioteca, cola, player) | ✅ | ✅ |
| Agregar enlaces YouTube / Spotify | ✅ | ✅ |
| Agregar a la cola | ✅ | ✅ |
| Borrar canciones **propias** | ✅ | ✅ |
| Borrar canciones **de otros** | ✅ | ❌ (403) |
| Quitar de la cola canciones de otros | ✅ | ❌ (403) |
| **Editar / modificar** cualquier canción | ✅ | ❌ (403) |
| Registrar vídeos locales | ✅ | ❌ (403) |
| Pedir un vídeo karaoke (formulario) | ✅ | ✅ |
| Ver / gestionar solicitudes | ✅ (todas) | ✅ (solo las suyas) |
| Avanzar la cola (*Siguiente*) | ✅ | ❌ (403) |

Las contraseñas se guardan con `scrypt` y las sesiones con tokens
opacos (solo su hash SHA-256 vive en la base). El único privilegio de
edición de un invitado es borrar canciones que él mismo subió.

## El QR

El Player muestra un QR generado **en el navegador** con la librería
vendida `public/vendor/qrcode.min.js` (qrcode-generator 1.4.4, MIT,
sin servicios externos). El QR codifica solo la URL de unión y el
código de sala (`http://<servidor>:8080/?room=CÓDIGO`).

**Decisión de seguridad (distinta de KaraokeEternal):** en
KaraokeEternal la contraseña de sala viaja en el QR en base64, que
cualquiera puede leer — base64 no es cifrado. Aquí la contraseña de
sala, si existe, **no va en el QR**: el invitado la escribe a mano
después de escanear.

## Límites honestos

- **Derechos de autor:** compartir vídeos de karaoke con música
  comercial puede infringir derechos de autor, sobre todo si el
  servidor es público o lo usa gente fuera de tu círculo. Usa el
  servidor solo con invitados de confianza y sube/enlaza solo vídeos
  tuyos o con licencia. Esto no es asesoría legal.
- **YouTube / Spotify solo como enlaces:** sus términos prohíben
  descargar y re-servir su contenido. KaraokeMuse nunca descarga
  nada de ellos: solo lee título/artista/portada vía oEmbed y los
  reproduce con su reproductor oficial embebido.
- **LAN por defecto:** el QR funciona en tu red local (o en la red
  donde esté el PC). Para usarlo fuera de casa harían falta HTTPS +
  reverse proxy (Caddy/NGINX) y un dominio o túnel; no expongas el
  puerto directamente a internet.
- **Un solo servidor, una sola sala en v1:** no hay salas múltiples
  ni cuentas con contraseña para invitados (entran por código de
  sala). La letra `.srt` se muestra sin sincronía en v1.
- **El generador de vídeos no está integrado**:
  se genera en la VM de Muse y se agrega manualmente al servidor.

## Estructura

```
KaraokeMuse/
├── server/
│   ├── index.js   # servidor HTTP, API REST y streaming por rangos
│   ├── db.js      # SQLite (node:sqlite): usuarios, sala, canciones, cola, solicitudes
│   └── auth.js    # scrypt + sesiones con token
├── public/
│   ├── index.html # app (Biblioteca, Cola, Player, A cantar, Pedir vídeo, Admin)
│   ├── app.js     # lógica del frontend, sin compilación (+ registro PWA)
│   ├── style.css
│   ├── sw.js      # service worker PWA: solo cachea la interfaz, nunca la API ni el vídeo
│   ├── manifest.webmanifest  # PWA instalable (KaraokeMuse, standalone)
│   ├── icons/icon-192.png, icon-512.png, icon.svg  # iconos PWA (micrófono dorado, generados)
│   └── vendor/qrcode.min.js   # qrcode-generator 1.4.4 (MIT)
├── data/          # base SQLite (gitignored, se crea al arrancar)
├── LICENSE        # MIT (Cosio33, 2026)
├── package.json   # sin dependencias; "start": node server/index.js
└── TESTS.md       # pruebas hechas con el servidor real
```

## Android — 3 opciones

Uso desde  KaraokeMuse desde su teléfono Android, además de en su PC. Hay 3 vías, en orden de
rapidez:

### Opción 1 — PWA instalable (la vía rápida, ya hecha ✅)

La app ya es una **PWA instalable**: `public/manifest.webmanifest`
(nombre KaraokeMuse, `display: standalone`, colores `#171033`),
iconos propios 192×192 y 512×512 (`public/icons/`, micrófono dorado
sobre fondo oscuro, generados sin imágenes con copyright; el SVG
fuente es `icon.svg`) y service worker `public/sw.js` que cachea
**solo la interfaz** (HTML/CSS/JS/vendor/iconos). La API
(`/api/...`) y los streams de vídeo **nunca se cachean**: la
biblioteca, la cola y el Player siempre necesitan el servidor
encendido.

**Cómo instalarla en el teléfono (Chrome):**

1. En el teléfono, abre en Chrome la dirección del servidor, por
   ejemplo `http://192.168.x.x:8080` (la IP de la PC en tu red, no
   `localhost`: `localhost` en el teléfono es el propio teléfono).
2. Cuando aparezca el botón **📲 Instalar app** (arriba, bajo el
   título), tócalo y confirma. Si no aparece, usa el menú de Chrome
   (⋮) → **Agregar a pantalla principal** / **Instalar app**.
3. La app queda en la pantalla principal con su icono y se abre a
   pantalla completa, sin la barra del navegador. Para entrar a la
   sala puedes escanear el QR del anfitrión desde la PWA instalada,
   igual que desde el navegador.

**Qué hace y qué NO hace la PWA:**

- ✅ Sí: icono en pantalla principal, apertura a pantalla completa,
  interfaz disponible sin red tras la primera carga.
- ❌ No: **no sirve el backend**. Si la PC (o el teléfono, opción 2)
  con el servidor está apagada, la PWA abre la interfaz pero no
  carga canciones, cola ni Player. Tampoco reproduce vídeos locales
  si el servidor no está corriendo.
- ❌ No: no es una app de Play Store ni funciona sin el servidor.

### Opción 2 — Servidor en el teléfono con Termux (documentado, NO validado en un teléfono real)

Con Termux el teléfono puede **correr el mismo servidor** (Node)
y servir KaraokeMuse en su propia red Wi-Fi, sin PC encendida.
Esto **no se probó en un teléfono real** — los pasos son los
documentados y hay que validarlos en el teléfono.

**Fuentes (consultadas 2026-11-20):** Node.js 24 es LTS hasta abril
de 2028 y publica binarios precompilados para Linux ARM64
(nodejs.org, migración v22→v24); Termux instala Node con
`pkg install nodejs` (línea Current, Node 26 en julio de 2026) o
`pkg install nodejs-lts` (línea LTS, Node 24.17.0 en esa fecha; los
dos paquetes **conflictan**, hay que elegir uno). Para KaraokeMuse
hay que usar **`nodejs-lts`** (Node 24), no `nodejs` (Current 26).

**Pasos exactos en el teléfono:**

1. Instala **Termux desde F-Droid** (https://f-droid.org), **no
   desde Google Play**: la versión de Play está desactualizada
   desde 2020/2023 y los paquetes fallan.
2. En Termux:
   ```bash
   pkg update && pkg upgrade -y
   pkg install -y nodejs-lts git
   node -v   # debe dar v24.x
   ```
3. Clona el repo (cuando esté publicado) y arranca el servidor
   **dentro de `~/` de Termux, no en `/sdcard`** (el almacenamiento
   compartido es `noexec` y no admite enlaces simbólicos; rompe
   git y herramientas):
   ```bash
   git clone https://github.com/Cosio33/KaraokeMuse.git
   cd KaraokeMuse
   node server/index.js
   ```
4. Abre en Chrome del teléfono `http://localhost:8080` y, para
   que otros móviles entren, la IP del teléfono en la Wi-Fi
   (`http://192.168.x.x:8080`); el QR del Player usa esa URL.
   Pon una contraseña de sala y cambia la contraseña del admin
   (`KARAOKE_ADMIN_PASSWORD`) antes de abrirlo a nadie.
5. **Batería y proceso en segundo plano:** el servidor debe seguir
   encendido mientras se cante. Android 12+ tiene un *phantom
   process killer* que puede matar procesos largos en segundo
   plano con señal 9; Termux puede morir si se cierra la app o el
   sistema lo mata por batería. Consejos: mantén Termux en primer
   plano o con un wake lock (`termux-wake-lock`, paquete
   `termux-api`), desactiva la optimización de batería para Termux
   y ten el teléfono cargando durante la fiesta. El consumo exacto
   de batería **no está medido**: es un servidor ligero (Node +
   SQLite, sin GPU), pero el streaming de vídeo y los embeds de
   YouTube/Spotify los hace el navegador, no el servidor.

**NO validado en un teléfono real (pendiente):**

- Que `node:sqlite` funcione en Termux/ARM64 con Bionic: es parte
  de Node 24 (no un addon nativo con prebuild que falte), así que
  en teoría debería funcionar, pero la build de Termux es propia
  (Bionic, no glibc) y no es idéntica al binario oficial de
  nodejs.org para Linux ARM64. Hay que probarlo en el teléfono
  con `node -e "require('node:sqlite')"`.
- Batería real durante una sesión de karaoke, muerte del proceso
  por el *phantom process killer* y exposición del puerto 8080 en
  la red Wi-Fi del teléfono (algunos routers/móviles bloquean
  conexiones entrantes al teléfono).
- Los vídeos locales en el teléfono: las rutas que registra el
  admin deben existir **dentro del almacenamiento de Termux** (o
  con `termux-setup-storage` y rutas `/sdcard/...`); el acceso a
  archivos fuera de Termux está limitado por el almacenamiento
  con ámbito de Android 11+.

### Opción 3 — App nativa / wrapper (solo plan, NO construida)

Envolver la PWA en una app Android instalable (APK) para no
depender de Chrome ni de Termux. **No se creó ningún proyecto
nativo en este commit**; es solo evaluación y plan.

| Enfoque | Qué es | Qué necesita | Veredicto |
|---|---|---|---|
| **Capacitor (recomendado)** | WebView (Chrome) + puente nativo, de Ionic. El estándar para envolver una web en Android/iOS. | Node + `@capacitor/core`/`cli`, Android Studio + JDK 17+ + Android SDK (API 34), `npx cap sync` tras cada cambio web. App ~4–6 MB. | ✅ **Recomendado**: el más maduro para móvil, plugins listos (archivos, compartir, notificaciones), la PWA no cambia. |
| **Tauri Mobile** | WebView del sistema + backend en Rust (Tauri 2, móvil desde 2024). | Rust + Android SDK + **NDK** + 4 targets Rust para Android (`aarch64-linux-android`, etc.). App ~2–4 MB. | Interesante a futuro: encaja con el flujo de Cosio33 (REA + GitHub + **rust-anything**), pero móvil aún en beta y ecosistema de plugins más fino que Capacitor. |
| **WebView nativa (Kotlin)** | Una Activity con un WebView que carga la app, escrita a mano. | Android Studio + Kotlin + Gradle; hay que escribir a mano todo lo que Capacitor ya trae (navegación, permisos, archivos). | Solo si se quiere el APK más mínimo y sin dependencias; más trabajo manual por el mismo resultado. |
| **TWA (Trusted Web Activity)** | El APK es básicamente Chrome a pantalla completa apuntando a la PWA. | Dominio con HTTPS + `assetlinks.json` (prueba de propiedad). | ❌ No sirve aquí: KaraokeMuse es un servidor LAN sin dominio ni HTTPS, así que no se puede verificar la propiedad. |

**Pasos cuando se solicite (con Capacitor, el recomendado):**

1. En la PC (o en GitHub Actions): `npm i @capacitor/core @capacitor/cli`,
   `npx cap init KaraokeMuse mx.cosio33.karaokemuse --web-dir=public`,
   `npx cap add android`, `npx cap sync`.
2. Abrir el proyecto en **Android Studio** en la PC y compilar
   el APK (debug primero; release con keystore propio y firmado).
3. **Los builds Android NO se hacen en la VM de Muse:** Gradle no
   corre en esta VM porque el sandbox bloquea TCP en localhost
   (probado con navegacion-care). Van por **GitHub Actions**
   (workflow `android.yml`: JDK 17 + Android SDK + `./gradlew
   assembleDebug`), como ya se hace con otros proyectos.
4. Decisión pendiente para entonces: ¿la app nativa solo es el
   **cliente** (se conecta al servidor de la PC, como la PWA) o
   también **lleva el servidor dentro** (mucho más complejo: Node
   embebido en Android no es trivial; el camino realista para
   "servidor en el teléfono" sigue siendo Termux, opción 2)?

## Licencia

MIT — ver `LICENSE`. Autor: Cosio33 (2026). Inspirado en
KaraokeEternal (RadRoot LLC, ISC); sin código copiado de ese proyecto.
