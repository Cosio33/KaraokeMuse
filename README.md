# 🎤 KaraokeMuse

Port del servidor de KaraokeMuse (v1, Node 24) a **Rust puro**, con
paridad total: mismas rutas, mismos status codes, mismas variables
de entorno, mismo archivo SQLite y mismo formato JSON. El frontend
(`public/`) no se porta: sigue en JavaScript y se sirve como
archivos estáticos.

> **Esta es la rama principal desde 2026-11-23 (ADR-021).** El
> servidor Rust es la versión principal; el servidor Node v1.0.0 queda
> en la rama secundaria `node-v1`, y `server/` en este repo queda como
> referencia.

Repo: https://github.com/Cosio33/KaraokeMuse

## Versión Node (secundaria)

La versión original, en Node 24 sin dependencias (v1.0.0), vive en
la rama [`node-v1`](https://github.com/Cosio33/KaraokeMuse/tree/node-v1)
y su documentación está en [README-node.md](README-node.md). La guía
de uso de ese documento (roles, QR, Android, PWA) vale igual para la
versión Rust: mismo frontend, misma API, misma base SQLite.

## Estado

- **Lift pasado el 2026-11-20**: `rust/app/rust-anything.json`
  marca `"lifted": true`. La app es Rust pura: sin código Node en
  el binario, sin proxy ni kill switches.
- **Replay 295/295, fallback 0%** contra el binario release.
- **50 tests** en los 5 crates, `cargo fmt --check` y
  `clippy --all-targets -- -D warnings` limpios.
- **CI en verde** en `main`: fmt, clippy, tests y build release
  en Linux y Windows. Artefactos: `karaokemuse-linux-x86_64`
  (3.60 MB) y `karaokemuse-windows-x86_64` (3.04 MB).
- Binario release `karaokemuse`: ~3.6 MB en Linux, ~3.0 MB en
  Windows (artefactos de la CI).

## Qué se portó

| Crate | Origen | Qué hace |
|---|---|---|
| `rust/crypto` (`karaoke-crypto`) | primitivas de `db`/`auth` | scrypt + sha256, verificados byte a byte contra vectores de Node |
| `rust/db` (`karaoke-db`) | `server/db.js` (126 líneas) | SQLite: abrir, sembrar, sesiones; replay 140/140 |
| `rust/auth` (`karaoke-auth`) | `server/auth.js` (50 líneas) | Login con scrypt, tokens opacos, sesiones; replay 9/9 + 6 de sesiones |
| `rust/routes` (`karaoke-routes`) | `server/index.js` (388 líneas) | El núcleo: ~25 endpoints, estáticos, rangos (206), oEmbed YouTube/Spotify |
| `rust/app` (front door axum) | — | Binario `karaokemuse`: axum + tokio, 18 rutas + catch-all |

Cada crate es independiente (sin workspace raíz): cada uno tiene
su propio `Cargo.toml` y `Cargo.lock`. El frontend, la PWA, los
iconos y el service worker no se tocaron.

## Requisitos

- **Rust estable** (cargo + rustfmt + clippy). Nada más: el
  binario final **no necesita Node** ni el código Node presente.
- Para compilar en GitHub no hace falta nada local: la CI lo hace
  en Linux y Windows (ver `.github/workflows/rust.yml`).

### Instalar Rust (solo si vas a compilar en tu máquina)

La forma oficial es **rustup** (https://rustup.rs). Verificado
con Rust 1.100.0; la CI usa el stable más reciente.

**Windows (la PC de Cosio33):**

1. Descarga y ejecuta el instalador de https://rustup.rs
   (`rustup-init.exe`).
2. Acepta la instalación por defecto (tecla Enter).
3. Cierra y vuelve a abrir la terminal (o reinicia la PC si el
   comando no aparece).
4. Comprueba:

   ```bat
   cargo --version
   rustc --version
   ```

**Linux:**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
cargo --version
```

**Componentes que usa el proyecto:**

```bash
rustup component add rustfmt clippy
```

(Suelen venir con la instalación por defecto; si ya están, el
comando no hace nada.)

La primera compilación descarga las dependencias de crates.io
(axum, tokio, rusqlite, scrypt, etc.); después compila sin red.
No hay `npm install` ni nada de Node en este paso.

## Compilar

Desde la raíz del repo:

```bash
cargo build --release --manifest-path rust/app/Cargo.toml
```

El binario queda en `rust/app/target/release/karaokemuse`
(`karaokemuse.exe` en Windows).

## Arrancar

Igual que la v1: un solo comando, mismas variables de entorno,
misma base de datos.

```bash
./rust/app/target/release/karaokemuse
```

El servidor arranca en `http://localhost:8080` y guarda los datos
en `./data/karaokemuse.sqlite`. **Abre la base existente sin
migración**: puedes arrancar el binario Rust sobre los datos que
ya creó el servidor Node.

El binario publicado es suelto: basta con ponerlo junto a la carpeta
`public/` del repo (y deja que cree `data/` a su lado) y ejecutarlo; no
hacen falta variables de entorno. Al arrancar imprime en la terminal
`[karaokemuse] Servidor en http://localhost:<puerto>`.

### Probarlo con el teléfono

Abre `http://localhost:8080` en la PC y escanea el QR: la app ya usa
sola la IP LAN de la PC para el QR (ADR-020), así que no hace falta abrir
la IP a mano. Si el teléfono no conecta: comprueba que PC y teléfono
estén en el mismo Wi-Fi y acepta el permiso de red de Windows cuando lo
pida. Si el puerto 8080 está ocupado, arranca con `PORT=8081` (en CMD:
`set PORT=8081` antes de ejecutar el `.exe` en la misma ventana).

### Novedades tras la primera prueba (ADR-020)

- **Player:** botones Anterior, Pausa y Siguiente (admin). La pausa
  funciona en vídeos locales y YouTube; Spotify se pausa con su propio
  control dentro del reproductor embebido.
- **Spotify y vídeos locales:** los enlaces de Spotify con prefijo de
  idioma (`/intl-es/`) ya embeben. Las rutas locales pegadas desde el
  Explorador de Windows (con comillas) se corrigen solas al registrarse,
  y en la pestaña Admin hay un explorador de carpetas del servidor para
  elegir el vídeo sin teclear la ruta.
- **Biblioteca:** el formulario va primero y el repertorio después, con
  botón Editar en cada canción y casillas para agregar varias a la cola
  o borrarlas a la vez.
- **Admin:** desde la pestaña Admin se cambia el nombre y la contraseña
  de la sala, se elimina la sala (se crea una nueva con código nuevo) y
  se cambia la contraseña del admin.

### Novedades (ADR-024)

- **Enlaces cortos de YouTube:** los enlaces que comparte la app de
  YouTube (`https://youtu.be/…`) ya se aceptan igual que los completos.
  Antes, Detectar los etiquetaba como Spotify y agregarlos fallaba.
- **Letra en Spotify:** cuando suena una canción de Spotify, el Player
  muestra su letra (buscada en LRCLIB, gratis y sin clave) debajo del
  reproductor, sin sincronizar: el embed de Spotify no comparte el
  tiempo de reproducción. Si no se encuentra la letra, se avisa que la
  reproducción de Spotify es solo para escuchar y que el karaoke con
  letra sincronizada es para vídeos locales.

### Novedades (ADR-025)

- **Avance automático:** en pantalla completa, al terminar una canción
  pasa sola a la siguiente (solo en la pantalla del admin). Usa el
  botón "⛶ Pantalla completa" del Player: si se pone en pantalla
  completa el vídeo con su propio botón, al cambiar de canción se
  saldría de pantalla completa.
- **Listas de YouTube (solo admin):** pega el enlace de una lista
  (`youtube.com/playlist?list=…`) en Biblioteca y usa "📋 Importar
  lista completa" para agregar todos sus vídeos de una vez (hasta 100,
  sin repetir los que ya están). Las mezclas automáticas (`list=RD…`)
  no se pueden importar. Si un invitado pega una lista, se le pide que
  la solicite en la pestaña Pedir video, y el admin la importa desde
  esa petición.

| Variable | Por defecto | Qué hace |
|---|---|---|
| `PORT` | `8080` | Puerto del servidor |
| `HOST` | `0.0.0.0` | Interfaz de escucha (en LAN, `0.0.0.0` para que otros equipos de la red lo vean) |
| `DATA_DIR` | `./data` | Carpeta de la base SQLite |
| `KARAOKE_ADMIN_USER` | `admin` | Usuario administrador |
| `KARAOKE_ADMIN_PASSWORD` | `admin123` | Contraseña del admin — **cámbiala antes de abrir el servidor a nadie** |
| `KARAOKE_ROOM_NAME` | `Sala principal` | Nombre de la sala |
| `KARAOKE_ROOM_PASSWORD` | *(vacía)* | Contraseña opcional de la sala (los invitados la escriben a mano) |

Variables solo para pruebas/replay (no las uses en producción):

| Variable | Qué hace |
|---|---|
| `KARAOKE_RESET_RECORD_DB=1` | Resiembra la BD temporal de pruebas al arrancar (se niega a borrar un directorio que no se llame `record-data`; la BD real nunca se toca) |
| `KARAOKE_PUBLIC_DIR` | Sobrescribe el directorio de estáticos (por defecto `<ROOT>/public`) |

## Permisos (igual que v1)

- **Invitados**: ver todo, agregar canciones por enlace de
  YouTube o Spotify a la cola, y borrar solo las canciones que
  ellos mismos agregaron. No pueden subir vídeos locales ni editar
  nada.
- **Admin**: todo lo demás (editar, vídeos locales, avanzar la
  cola, gestionar solicitudes de generación de vídeo).
- Los vídeos locales se **enlazan desde archivos del servidor,
  nunca se suben ni se copian**. No hay generador de vídeo
  integrado: la generación se hace aparte y se agrega a mano.

## Verificar (tests + replay)

```bash
source ~/workspace/tools/rust-anything/env/rust-env.sh  # solo en la VM de Muse
cd rust/crypto && cargo test   # 5/5
cd ../db && cargo test         # 17 (10 unit + 3 seed + 3 record + replay 140/140)
cd ../auth && cargo test       # 7 (replay 9/9 + 6 sesiones)
cd ../routes && cargo test     # 21 (8 unit + 13 integración)
cd ../app && cargo build --release && cargo clippy --all-targets -- -D warnings
```

Replay completo contra el binario release (295 casos grabados del
servidor Node original):

```bash
cd ~/workspace/KaraokeMuse
DATA_DIR=$PWD/rust/record-data KARAOKE_RESET_RECORD_DB=1 KARAOKE_PUBLIC_DIR=$PWD/public \
  rust-anything app replay . --build "rust/app/target/release/karaokemuse"
# 295/295, fallback 0%
```

Sin `DATA_DIR` el binario abriría `<ROOT>/data` (la BD real); sin
`KARAOKE_RESET_RECORD_DB=1` la BD de pruebas no se resiembra.

## CI

`.github/workflows/rust.yml`: en cada push a `main`
(además de PR y tags `v*`) corre `fmt --check`, `clippy -D
warnings`, `cargo test` y `build --release` en `ubuntu-latest` y
`windows-latest`, y sube el binario como artefacto. Sin replay en
CI (necesita Node).

## Cómo se hizo

Con **rust-anything** (record → bridge → diff → gate → lift)
sobre el plan en
`~/workspace/custom-skills/rust-anything/references/karaokemuse-plan*.md`
de la VM de Muse. Decisiones en `rust/DECISIONS.md` (ADR-001 a
ADR-021), mapa en `rust/MAP.md`. Dos detalles que costaron:

- **ADR-015**: Node pasa la sal scrypt hex **como cadena**
  (32 bytes UTF-8), no decodificada (16 bytes). El port lo hace
  igual; el bug se encontró porque el login daba 401 tras el
  primer cableado.
- **ADR-017/018**: el lift pedía `src/port.rs` real en
  `karaoke-routes` y un binario puro sin arrancar el original.
  El lift se corrió con el build de rust-anything **desde fuente**,
  porque el binario del release 1.5.1 no copia al stage las
  dependencias por ruta.

## Límites honestos

- El caso `songs-random` del replay es débil (el orden aleatorio
  se cubre con wildcards); se verificó a mano: 6 llamadas al
  binario release, 3 canciones distintas en cada una.
- `ORDER BY created_at DESC` con timestamps sembrados iguales no
  garantiza orden en SQLite; el replay lo tolera con wildcards.
- El estado se inicializa en la primera request, no en el
  arranque: si la BD no abre, esa request da 500 en vez de tumbar
  el arranque (desviación respecto a Node).
- oEmbed real solo tiene cobertura del camino de error; la UI en
  navegador real y el escaneo del QR ya se probaron en la PC del
  usuario (ADR-020); la reproducción de un MP4 real en el navegador
  sigue pendiente de su prueba completa.
- Sin target Termux/Android (aarch64) por ahora; se evalúa aparte.
