# HANDOFF — Port de KaraokeMuse a Rust (checkpoint 2026-11-20, lift pasado)

## Estado

- Rama: `rust-port` (local; sin push a GitHub — lo hace el agente
  principal por navegador).
- **LIFT PASADO 2026-11-20 (ADR-017, ADR-018): la app es Rust pura.**
  `rust/app/rust-anything.json` marca `"lifted": true`;
  `src/original.rs` borrado por el lift; `reqwest` fuera de
  `rust/app/Cargo.toml`; `mod.rs` en forma levantada (sin kill
  switches); `main.rs` puro (sin llamadas al original).
- Front door: `rust/app`, binario `karaokemuse` release **8.2 MB**
  (debug 77 MB), replay **295/295**, fallback **0.0% (0/295)** —
  todo nativo en Rust, sin Node.
- Unidades `server/db`, `server/auth` y `server/routes`:
  **terminadas**. Total tests: **44** (crypto 5, db 17, auth 7,
  routes 15, app 0), fmt y clippy limpios en los 5 crates.
- CI Rust creada localmente: `.github/workflows/rust.yml` (sin
  subir; la sube el agente principal).

## Lo terminado en este checkpoint

### El lift — pasado 2026-11-20 (ADR-017, ADR-018)

- El lift se negó primero con los dos bloqueos de ADR-016 y luego
  con dos problemas del stage; los cuatro se resolvieron:
  1. **Gate de `karaoke-routes` (ADR-017):** el check `ported` del
     lift lee `src/port.rs`, que un crate lib no tiene. Se añadió
     `rust/routes/src/port.rs` real (fachada pública que re-exporta
     `AppState`, `dispatch`, `serve` y los módulos del port); el
     check pasa en verdad porque el crate no tiene stubs. No se
     fabricó ningún `rust-anything.json`.
  2. **Binario puro sin original (ADR-018):** `main.rs` ya no
     arranca el servidor Node. El reseteo de la BD de pruebas vive
     en Rust: `karaoke_db::reset_record_data`
     (`rust/db/src/record.rs`, espejo de `seed-record-db.mjs`),
     que corre solo con `KARAOKE_RESET_RECORD_DB` fijada y se niega
     a borrar un directorio que no se llame `record-data` (la BD
     real `data/` nunca se toca). `main.rs` mantiene el toque al
     archivo de fallback (vacío = 0 fallbacks reportados).
  3. **Estáticos del stage:** el binario del stage se compila con
     `ROOT` temporal y sin `public/` (7 casos 404). El glue acepta
     `KARAOKE_PUBLIC_DIR` (sin ella, `<ROOT>/public` como siempre).
  4. **Casos de replay fuera del crate:** los tests de replay de
     db/auth leían `cases.jsonl` fuera de su crate, que el stage no
     copia. Ahora hay copias dentro de los crates
     (`rust/db/tests/cases.jsonl`, 140; `rust/auth/tests/auth-cases.jsonl`,
     9) con assert de conteo; los originales siguen siendo los
     artefactos de la grabación.
- **El lift del release 1.5.1 NO sirve:** no copia al stage las
  dependencias por ruta (faltaron `db`/`auth`/`crypto`). El lift se
  corrió con el build desde fuente de `main`:
  `~/workspace/tools/rust-anything/from-source/bin/rust-anything`.
- Invocación del lift:
  `DATA_DIR=$PWD/rust/record-data KARAOKE_RESET_RECORD_DB=1 KARAOKE_PUBLIC_DIR=$PWD/public rust-anything app lift .`
- Resultado del lift: todos los checks en verde (fmt, clippy,
  cargo test, unsafe en routes/auth/crypto/db/app) y replay
  **295/295, fallback 0%** contra el binario levantado. "Removed 1
  generated files; the original is untouched."
- Verificación final (post-lift): release build 8.2 MB; replay
  **295/295, fallback 0.0%** contra `rust/app/target/release/karaokemuse`;
  `cargo test` 44/44 en los 5 crates; fmt y clippy limpios.
  `/api/songs/random` verificado a mano con el binario release
  levantado: 6 llamadas, 3 canciones distintas en cada una; login
  admin OK; `GET /` 200; **ningún proceso Node corriendo**.

### `server/routes` — terminado 2026-11-20 (ADR-014)

- Crate `karaoke-routes` (`rust/routes/`): `AppState` (Db + Room +
  dir público + cliente reqwest), `init` (abre la BD, siembra con
  las `KARAOKE_*`) y `dispatch` (cadena if/else fiel a `handle` de
  `server/index.js`: auth, sala/join, metadata oEmbed, canciones
  CRUD + validación + filtros + random, cola, player, solicitudes,
  media por rangos nativo con `tokio::fs` + stream — nunca en
  memoria — y estáticos con guarda anti-traversal).
- Módulos: `json.rs` (filas SQLite → JSON en orden de columnas,
  `Number::toString` de JS a mano, `Number(str)`, `Math.min/max`,
  truthiness), `http_util.rs` (cuerpo 1 MiB, query estilo WHATWG,
  `encodeURIComponent`, normalización de pathname, corte UTF-16),
  `validate.rs` (validación de canciones, mismo orden de errores),
  `metadata.rs` (oEmbed YouTube/Spotify, mismo agrupamiento de
  errores del `catch` original), `handlers.rs` (el dispatch),
  `lib.rs` (`AppState` + `dispatch` + binario de pruebas `serve`),
  `port.rs` (ADR-017).
- Tests: 8 unitarios + 7 de integración (`tests/routes.rs`, BD y
  `public/` temporales, nunca `data/`): health/me/login/join,
  CRUD + validación, cola/player, solicitudes, media 200/206 +
  rango imposible (500) + rango fuera de archivo (206 vacío),
  estáticos, metadata sin red, cuerpo >1 MiB y JSON inválido.
- `cargo fmt --check` y `clippy --all-targets -- -D warnings`
  limpios; release compila (`rust/routes/target/release/`).

### Wire — 18 rutas, fallback 0%

- `app wire . routes --skip-gate` en tres tandas (exactas,
  `{*rest}`, catch-all `*`), siempre con la lista COMPLETA (ADR-016:
  `--route` reemplaza la lista; omitirlo la conserva).
- Rutas sin método (`any()`): Node da 404, no 405, con método
  incorrecto. Glue en `rust/app/src/units/karaoke_routes.rs`
  (init perezoso del estado en la primera request; `DATA_DIR` o
  `<ROOT>/data`, `<ROOT>/public` como `index.js`, salvo
  `KARAOKE_PUBLIC_DIR`).
- Replay tras cada tanda: exactas 295/295 fallback 81.7% →
  +`{*rest}` 295/295 fallback 71.2% → +`*` **295/295 fallback
  0.0%**. Los kill switches ya no existen (el lift los quitó).

### Corrección scrypt — ADR-015 (bug real, encontrado por el replay)

- `login-good` falló con 401 tras el primer wire: `verify_password`
  en `karaoke-crypto` hacía `hex::decode` de la sal (16 bytes),
  pero Node pasa la sal hex COMO CADENA a `scryptSync` (32 bytes
  UTF-8). El bug pasó desapercibido porque `auth-record.mjs`
  generaba su vector con `Buffer.from(saltHex, 'hex')`.
- Corregido: `hash_password`/`verify_password` usan
  `salt_hex.as_bytes()`; `auth-record.mjs` regenera el vector con
  la cadena; `auth-cases.jsonl` regenerado; nuevo test
  `verify_password_matches_node_string_salt` (vector Node
  `0c5f3622…`). Crypto 5/5, auth replay 9/9 + sesiones 6/6.
- Verificado: el hash del admin sembrado es scrypt con sal-cadena.

### CI — `.github/workflows/rust.yml` (local, sin subir)

- fmt --check, clippy -D warnings, cargo test y build --release
  en ubuntu-latest y windows-latest (matriz), por crate (sin
  workspace raíz), artifact `karaokemuse-linux-x86_64` /
  `karaokemuse-windows-x86_64`. Disparadores: push a main y
  rust-port, PR, tags `v*`. Sin replay en CI (necesita Node).

### Decisiones nuevas (en `rust/DECISIONS.md`)

- **ADR-014**: `server/routes` como crate lib `karaoke-routes`,
  wire con rutas sin método. (Corrección: init perezoso en el
  glue, no en `main.rs`.)
- **ADR-015**: scrypt usa la sal hex COMO CADENA (corrección de
  ADR-012).
- **ADR-016**: wire reemplaza la lista de rutas; replay/wire con
  `DATA_DIR`; lift bloqueado (bloqueos resueltos 2026-11-20).
- **ADR-017**: gate del lift para crates lib — `src/port.rs` real
  en `karaoke-routes`; el lift corre con el build desde fuente.
- **ADR-018**: binario puro — reseteo de BD de pruebas en Rust,
  `main.rs` sin original, `KARAOKE_PUBLIC_DIR`, copias de casos
  en los crates.

## Cómo verificar

```bash
source ~/workspace/tools/rust-anything/env/rust-env.sh
cd ~/workspace/KaraokeMuse
cd rust/crypto && cargo test                 # 5/5
cd ../db && cargo test                        # 10 + replay 140/140 + seed 3/3 + record 3/3
cd ../auth && cargo test                      # replay 9/9 + sesiones 6/6
cd ../routes && cargo test                    # 8 + 7 integración
cd ../app && cargo build --release && cargo clippy --all-targets -- -D warnings
cd ../.. && DATA_DIR=$PWD/rust/record-data KARAOKE_RESET_RECORD_DB=1 KARAOKE_PUBLIC_DIR=$PWD/public rust-anything app replay . --build "rust/app/target/release/karaokemuse"  # 295/295, fallback 0%
```

Sin `DATA_DIR` el glue abriría `<ROOT>/data` (BD real); sin
`KARAOKE_RESET_RECORD_DB=1` la BD de pruebas no se resiembra y el
replay ve el estado mutado de la corrida anterior; sin
`KARAOKE_PUBLIC_DIR` un binario compilado fuera del repo no
encuentra los estáticos.

## Pendiente (en orden)

1. **Merge de `rust-port` a main y release v2.0.0** con binarios
   Linux + Windows (la CI ya los sube como artifacts). Lo hace el
   agente principal con Cosio33, no este port. El servidor Node
   (`server/`) queda en el repo como referencia hasta que Cosio33
   decida el cutover; no se borra.
2. El push a GitHub (rama + workflow) lo hace el agente principal
   por navegador.
3. `record-server.sh` y `seed-record-db.mjs` quedan como artefactos
   históricos de la grabación; el reseteo vivo para replay es el
   de Rust (`KARAOKE_RESET_RECORD_DB=1`).

## Riesgos / notas

- El caso `songs-random` del replay app sigue débil (wildcard
  `[..]` en el body por el orden aleatorio); verificación manual
  hecha tras el lift (6 llamadas, 3 distintas cada una).
- `ORDER BY created_at DESC` con timestamps sembrados iguales
  tiene orden no garantizado por SQLite; el replay lo tolera con
  wildcards/normalización.
- El glue inicializa el estado en la primera request, no en el
  arranque: si la BD no abre, esa request da 500 en vez de tumbar
  el arranque (desviación respecto a Node).
- No tocar `data/` (BD real). Todo build en `~/workspace`, nunca
  en /tmp. No push a GitHub desde esta sesión.
- Cada crate Rust tiene su `.gitignore` con `/target`; no añadir
  `target/` al repo (ya ocurrió una vez con routes y se deshizo).
- El lift del release 1.5.1 no copia dependencias por ruta al
  stage; para un futuro lift usar el build desde fuente de `main`.
