# MAP — Port de KaraokeMuse a Rust

## Objetivo

Reescribir el servidor de KaraokeMuse (Node 24, sin dependencias) como
una app Rust pura (binario `karaokemuse`), con paridad total con la v1:
mismas rutas, status codes, variables de entorno, mismo archivo SQLite
y mismo formato JSON. El frontend (`public/`) se queda en JavaScript y
se sirve como archivos estáticos. El original Node se conserva como
referencia hasta que Cosio33 diga lo contrario.

- Repo: https://github.com/Cosio33/KaraokeMuse
- Rama de trabajo: `rust-port` (local; en GitHub existe desde main
  1e0c856; el push y el workflow los hace el agente principal por
  navegador, NO este port)
- Análisis: `~/workspace/custom-skills/rust-anything/references/karaokemuse-plan*.md`
- Clon de análisis: `~/workspace/KaraokeMuse-analisis/`
- Flujo: `~/workspace/custom-skills/rust-anything/SKILL.md` y
  `~/workspace/tools/rust-anything/repo/skills/rust-anything/whole-app.md`

## Índice de ADRs (fuente de verdad: rust/DECISIONS.md)

- ADR-001 Forma: servidor HTTP con front door axum
- ADR-002 Paridad total con v1 (rutas, status, JSON, env)
- ADR-003 Misma base SQLite, sin migración
- ADR-004 Frontend en JavaScript, servido como estático
- ADR-005 Binario `karaokemuse`; targets Linux x86_64 y Windows x86_64
- ADR-006 Barra de equivalencia: JSON byte a byte; ruido normalizado
- ADR-007 Rama `rust-port`, merge solo tras el lift; release después
- ADR-008 El Node original se queda como referencia
- ADR-009 Suite de caracterización: 35 casos de TESTS.md, BD temporal
- ADR-010 `server/db` como crate lib Rust plano, no puente napi
- ADR-011 Paridad de mensajes de error y formato numérico de Node
- ADR-012 Primitivas scrypt/sha256 en crate hoja `karaoke-crypto`
- ADR-013 `server/auth` también como crate lib plano, sin puente napi
- ADR-014 `server/routes` como crate lib `karaoke-routes`, wire con rutas sin método
- ADR-015 scrypt usa la sal hex COMO CADENA (corrección de ADR-012)
- ADR-016 wire reemplaza la lista de rutas; replay/wire con `DATA_DIR`; lift bloqueado (bloqueos resueltos 2026-11-20, ver ADR-017/ADR-018)
- ADR-017 Gate del lift para crates lib — `src/port.rs` real en `karaoke-routes` (y el lift debe correrse con el build desde fuente, no el release 1.5.1)
- ADR-018 Binario puro — reseteo de BD de pruebas en Rust, `main.rs` sin original, y el stage del lift (`KARAOKE_PUBLIC_DIR`, copias de casos en los crates)

## Unidades

| Unidad | Archivos | Líneas | Estado | Gate / replay |
| --- | --- | --- | --- | --- |
| `rust/app` (front door axum) | `rust/app/` | — | **levantado 2026-11-20: app Rust pura** (`rust-anything.json` `"lifted": true`; `src/original.rs` borrado por el lift; `reqwest` fuera de las deps; `main.rs` puro sin original) | binario `karaokemuse` (release 8.2 MB, debug 77 MB); replay **295/295, fallback 0%** contra el binario release. `main.rs` lee `HOST` (def. 0.0.0.0) y `PORT` (def. 8080); `KARAOKE_RESET_RECORD_DB=1` resiembra la BD de pruebas al arrancar; `KARAOKE_PUBLIC_DIR` sobrescribe el dir de estáticos (solo replay/lift). Sin proxy, sin kill switches. |
| `server/db` | `server/db.js` | 126 | done (2026-11-20) | crate lib `karaoke-db` (`rust/db/`) + hoja `karaoke-crypto` (`rust/crypto/`). Replay 140/140 (randomCode 27/27, newId 12/12, openDatabase 12/12, seed 89/89) + 10 unit tests + 3 tests seed con BD válida + **3 tests de `reset_record_data`** (ADR-018). `cargo fmt --check` y `clippy -D warnings` limpios. Los casos viven también en `rust/db/tests/cases.jsonl` (copia del artefacto `db-rs/cases.jsonl`, con assert de 140). Enlazado a mano a `rust/app` (wire CLI exige `--route`; db no responde rutas) — fallback sigue 100% |
| `server/auth` | `server/auth.js` | 50 | done (2026-11-20) | crate lib `karaoke-auth` (`rust/auth/`). Replay 9/9 casos puros (`auth-cases.jsonl`, regenerados 2026-11-20 con la convención scrypt correcta, ADR-015) + 6 tests de sesiones (crear/consultar/destruir/expiración con borrado/fecha inválida/usuario fantasma). fmt y clippy limpios. Los casos viven también en `rust/auth/tests/auth-cases.jsonl` (copia, con assert de 9). Enlazado a mano a `rust/app` — fallback sigue 100% |
| `server/routes` | `server/index.js` | 388 | done (2026-11-20) | crate lib `karaoke-routes` (`rust/routes/`): `AppState` + `dispatch` (handlers, estáticos, rangos nativos, oEmbed) + **`src/port.rs`** (ADR-017: fachada pública del port para el check `ported` del lift). 8 tests unitarios + 7 de integración; `/api/songs/random` verificado a mano (3 distintas por llamada, 6 llamadas, binario release levantado). Cableado con `app wire` (18 rutas sin método + catch-all `*`; glue en `rust/app/src/units/karaoke_routes.rs`, init perezoso). Replay app **295/295, fallback 0.0%**. fmt y clippy limpios. |
| `public/` | `public/app.js`, HTML/CSS, sw.js, manifest | 348 | keep (no se porta) | servido como estático |
| `public/vendor/` | `public/vendor/qrcode.min.js` | 8 | keep (no se porta) | corre en el navegador |

Notas:
- `analyze` marcó las 3 unidades del plan como `ready`, 0 bloqueadas.
- `server/routes` es el núcleo enredado (router + ~25 handlers + estáticos
  + rangos + oEmbed); si una unidad es demasiado enredada, se toma como
  un solo bloque grande (decisión del coordinador, paso 5 de la tarea).
- `server/db` y `server/auth` no dependen de `server/routes`; el orden
  de port es db → auth → routes (las rutas usan db y auth).
- Fallback rate inicial (tras front door): 100%. Solo baja con cada `wire`.

## Casos de caracterización

- Fuente: `TESTS.md` (35 checks de v1) + extras (room, listados, estáticos).
- Grabación: `rust-anything app record --http` con el servidor ORIGINAL
  en puerto de prueba, BD temporal sembrada de forma determinista
  (`rust/record-data/`, nunca `data/`, que es la BD real).
- Script de arranque: `rust/record-server.sh`; sembrador: `rust/seed-record-db.mjs`.
- Casos en `rust/app-cases/http/`; replay contra el original: 100%.

## Preguntas abiertas

- Q1: ¿El caso oEmbed real de YouTube entra en la suite? Es el único
  caso con red externa; si es inestable se deja solo el camino de error
  (URL no soportada) y se documenta.
- Q2: ¿Binario Windows se verifica solo en Actions, o también se
  cross-compila aquí? Decisión del coordinador: solo Actions (sin
  toolchain Windows en la VM).
- Q3: ¿Binario para Termux/Android (aarch64) entra en este port?
  No: fuera de alcance; se evalúa aparte tras el lift.

## Fuera de alcance

- Portar el frontend a Rust o tocar `public/` (salvo servirlo).
- Cambios de PWA, iconos o service worker.
- Empaquetado Android/Termux, instaladores, DNS, despliegue.
- Spotify oEmbed en vivo (pendiente en TESTS.md; mismo camino que YouTube).
- Reproducir un MP4 real en el Player (TESTS.md lo deja pendiente).

## Estado actual

- 2026-11-20: rama `rust-port` creada; MAP/DECISIONS/GLOSSARY escritos.
- Investigación: `rust/research/storage-auth.md`, `http-stack.md` y
  `json-network.md` listos (los tres subagentes terminaron
  2026-11-20; versiones: rusqlite 0.43.0, scrypt 0.12.0, sha2 0.11.1,
  rand 0.10.4, hex 0.4.3, subtle 2.6.1, axum 0.8.9, tokio 1.54.1,
  serde 1.0.229, serde_json 1.0.154, reqwest 0.13.5).
- `server/db`: puente napi `rust/db-rs/` grabó 140 casos
  (`randomCode` 27, `newId` 12, `openDatabase` 12, `seed` 89) pero
  110 son solo error por el `DatabaseSync` vivo; decisión ADR-010:
  port como crate lib `karaoke-db`. Sondeos en vivo contra Node 24
  fijaron las reglas de formato de error (ADR-011). El caso ok de
  `openDatabase` había creado un directorio `./x/` en la raíz;
  eliminado (era artefacto de la grabación, no la BD real).
- `server/db` TERMINADO 2026-11-20: `karaoke-crypto` (scrypt
  verificado byte a byte contra 2 vectores de Node + sha256 de
  token) y `karaoke-db` (replay 140/140, 10 unit tests, 3 tests de
  `seed` con BD válida; fmt y clippy limpios). Enlazado a mano a
  `rust/app` (`app wire` exige `--route` y db no responde rutas;
  ver ADR-010). Replay del front door re-verificado: 295/295,
  fallback 100% (db no baja el fallback: no es una ruta).
- `server/auth` TERMINADO 2026-11-20 (ADR-013, mismo patrón que
  db: crate lib `karaoke-auth`, sin puente napi): primitivas
  re-exportadas de `karaoke-crypto`; sesiones sobre `karaoke-db`.
  Replay 9/9 casos puros generados contra el Node original
  (`rust/auth-record.mjs` → `rust/auth-cases.jsonl`) + 6 tests de
  sesiones (crear/consultar/destruir, expiración que borra la
  sesión, fecha inválida que NO expira, sesión de usuario
  inexistente → null). Enlazado a mano a `rust/app`; replay del
  front door re-verificado: 295/295, fallback 100%.
- `server/routes` terminado 2026-11-20 (ADR-014): crate
  `karaoke-routes`, 8 tests unitarios + 7 de integración, wire de
  18 rutas sin método + catch-all, replay **295/295, fallback
  0.0%**. Corrección scrypt ADR-015 (sal hex como cadena).
  Workflow CI `.github/workflows/rust.yml` creado (local).
  Lift bloqueado (ADR-016): gate de routes no reconocido + main.rs
  todavía llama al original (que re-siembra la BD de pruebas).
- **LIFT PASADO 2026-11-20 (ADR-017, ADR-018): `rust-anything app
  lift .` con el build desde fuente de `main` (el release 1.5.1 no
  copia las dependencias por ruta al stage) y
  `DATA_DIR=$PWD/rust/record-data KARAOKE_RESET_RECORD_DB=1 KARAOKE_PUBLIC_DIR=$PWD/public`.
  Todos los checks en verde (fmt, clippy, cargo test, unsafe en
  routes/auth/crypto/db/app) y replay **295/295, fallback 0%**
  contra el binario levantado. El lift sincronizó: `original.rs`
  borrado, `mod.rs` reescrito en forma levantada (sin kill
  switches), `reqwest` fuera de `rust/app/Cargo.toml`,
  `"lifted": true` en `rust/app/rust-anything.json`. Binario release
  `karaokemuse` 8.2 MB, verificado a mano: `/api/songs/random` 6
  llamadas (3 distintas cada una), login admin, `GET /` 200, y
  ningún proceso Node corriendo. El binario NO necesita Node ni el
  código Node presente. Total tests: 44 (crypto 5, db 17, auth 7,
  routes 15, app 0), fmt y clippy limpios en los 5 crates.
  Queda pendiente: merge de `rust-port` a main + release v2.0.0
  con binarios Linux/Windows (lo hace el agente principal con
  Cosio33, no este port).
- Casos: 295 grabados con `app record --http` (43 inputs + 252 traps);
  replay contra el original **295/295**. Ruido: solo `songs-random`
  (body) — el cuerpo es una sola línea JSON y el orden aleatorio se
  fusionó en wildcards `[..]`; el caso quedó débil (verifica status
  200 + content-type + forma aproximada). Verificación manual del
  random hecha al portar routes (binario Rust puro, 5 llamadas:
  3 canciones distintas en cada una).
  Un input (`me-admin-after-logout-401`) se descartó por duplicado
  exacto con `me-admin`; se sustituyó por
  `songs-admin-after-logout-401` (GET /api/songs con token admin
  tras logout → 401).
- Front door: creado con `app front-door`; replay contra el binario
  `karaokemuse` (renombrado desde `record-server`) **295/295**,
  fallback **0.0%** tras cablear routes. El streaming por rangos ya
  es nativo (no pasa por memoria). El front door arranca el original
  vía `rust/record-server.sh` (BD de pruebas sembrada); nunca sirve
  la BD real. El catch-all `*` es nativo; el proxy solo se usa si
  `RUST_ANYTHING_UNIT_KARAOKE_ROUTES=original`.
