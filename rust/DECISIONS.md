# DECISIONS — Port de KaraokeMuse a Rust

Decisiones aprobadas por Cosio33 (paridad total con v1). Una sección por
ADR, numeradas en orden y sin renumerar jamás. Un cambio de decisión
crea un ADR nuevo que deja obsoleto al anterior.

## ADR-001: Forma — servidor HTTP con front door axum
Status: accepted (2026-11-20)
Context: `rust-anything analyze --whole-app` detecta forma `server`,
punto de entrada `server/index.js` (`createServer(` en la línea 383),
cero dependencias externas. El flujo whole-app pide una puerta de
entrada Rust que sea dueña del entry point desde el principio.
Decision: el port es un servidor HTTP. Se crea `rust/app` con
`rust-anything app front-door` (axum, proxya al original lo no portado
todavía) y las unidades se conectan con `app wire` por ruta.
Consequences: el binario Rust escucha en el mismo puerto y sirve las
mismas rutas; el Node original sigue corriendo detrás hasta el lift.

## ADR-002: Paridad total con la v1
Status: accepted (2026-11-20)
Context: KaraokeMuse v1 funciona y Cosio33 la usa; el port no debe
cambiar el comportamiento visible.
Decision: mismas rutas y métodos, mismos status codes, mismos mensajes
de error en español, mismas variables de entorno (`PORT`, `HOST`,
`KARAOKE_ADMIN_USER`, `KARAOKE_ADMIN_PASSWORD`, `KARAOKE_ROOM_NAME`,
`KARAOKE_ROOM_PASSWORD`, `DATA_DIR`), mismo formato JSON.
Consequences: cualquier diferencia en un caso grabado es un bug del
port, nunca del caso (salvo aprobación expresa de Cosio33, registrada
aquí).

## ADR-003: Misma base SQLite, sin migración
Status: accepted (2026-11-20)
Context: el servidor guarda todo en `karaokemuse.sqlite` (7 tablas:
users, rooms, songs, queue, requests, sessions, player_state), con
`PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;`.
Decision: el binario Rust abre el mismo archivo con rusqlite (feature
`bundled`); no hay migración ni cambio de esquema; los datos existentes
siguen válidos.
Consequences: las contraseñas scrypt existentes deben seguir siendo
verificables (mismos parámetros N/r/p y salida de 64 bytes); las
sesiones existentes siguen válidas (sha256 del token).

## ADR-004: Frontend en JavaScript, servido como estático
Status: accepted (2026-11-20)
Context: `public/` (app.js, index.html, style.css, sw.js, manifest,
iconos, qrcode-generator vendored) es la PWA que ya funciona.
Decision: el frontend NO se porta a Rust. El servidor Rust lo sirve
como archivos estáticos con los mismos MIME types y la misma guarda
anti-traversal (`..` fuera de `public/` → 403).
Consequences: `analyze` listó `public/` y `public/vendor/` como
unidades `ready`, pero se marcan `keep` por decisión, no por bloqueo.

## ADR-005: Binario `karaokemuse`; targets Linux x86_64 y Windows x86_64
Status: accepted (2026-11-20)
Context: Cosio33 corre el servidor en su PC (Windows) y en la VM
(Linux x86_64); los builds de Windows van por GitHub Actions.
Decision: el artefacto es un binario llamado `karaokemuse`. Linux
x86_64 se compila en esta VM; Windows x86_64 se compila en GitHub
Actions (workflow `.github/workflows/rust.yml`), no en esta VM.
Consequences: el workflow de Actions es parte del entregable; el
binario Windows no se verifica en local.

## ADR-006: Barra de equivalencia — JSON byte a byte; ruido normalizado
Status: accepted (2026-11-20)
Context: el port debe demostrar que se comporta igual que el original;
pero hay valores que cambian en cada corrida (timestamps, ids y tokens
aleatorios, orden de `ORDER BY RANDOM()`).
Decision: los cuerpos JSON, status codes y headers que importan
(content-type, content-range, accept-ranges, content-length) deben
coincidir byte a byte. El ruido se normaliza en los casos con
`--normalize` (timestamps, uuids, regex propias para tokens e ids),
nunca en el port. El orden de `/api/songs/random` se compara sin orden
(la aleatoriedad es el contrato de esa ruta).
Consequences: un caso con wildcard `lines` (`...`) no se acepta en
silencio: se revisa y se justifica aquí.

## ADR-007: Rama `rust-port`, merge solo tras el lift; release después
Status: accepted (2026-11-20)
Context: el trabajo no debe romper `main` (v1 en Node, funcionando) y
el release es una etiqueta con binarios, no un lugar para el código.
Decision: todo el port se hace en la rama local `rust-port`, con
commits de checkpoint. Merge a `main` solo cuando `app lift` pase con
fallback 0 y todos los gates en verde. Release (p. ej. `v2.0.0`) con
binarios después del merge. La subida a GitHub y el workflow por
navegador los hace el agente principal, no este port.
Consequences: la v1 Node queda en el historial y en su tag; si la
versión Rust falla, se puede volver atrás.

## ADR-008: El Node original se queda como referencia
Status: accepted (2026-11-20)
Context: el port se verifica contra el original en cada paso.
Decision: no se borra ni se modifica el servidor Node hasta el lift, y
después del lift queda en el repo como referencia hasta que Cosio33 diga
lo contrario. `app lift` trabaja sobre una copia primero.
Consequences: `server/` y `public/` siguen en el repo durante todo el
port.

## ADR-009: Suite de caracterización — 35 casos de TESTS.md, BD temporal
Status: accepted (2026-11-20)
Context: el repo no tiene tests automatizados (0 archivos de test); los
35 checks de `TESTS.md` son la suite de caracterización. El servidor
genera estado aleatorio (código de sala, ids, tokens) que los casos
estáticos de `app record --http` no pueden referenciar.
Decision: se recrean los 35 casos (más extras: room, listados,
estáticos) en un JSONL de inputs y se graban con
`rust-anything app record --http` corriendo el servidor ORIGINAL con
un sembrador determinista (`rust/seed-record-db.mjs`) que crea admin,
invitado, sala de código fijo, canciones/cola/solicitudes de ids fijos
y sesiones de tokens conocidos, en un directorio de datos temporal
(`rust/record-data/`). Nunca se toca `data/` (BD real). El replay
contra el original debe dar 100% antes de mover ninguna unidad.
Consequences: el sembrador y el script de arranque son parte del
entregable del port y quedan en `rust/`.

## ADR-010: `server/db` se porta como crate lib Rust plano, no como puente napi
Status: accepted (2026-11-20)
Context: `rust-anything bridge server/db.js` creó el andamio napi
`rust/db-rs`, pero `record` grabó 140 casos de los que 110 son solo
error: `openDatabase`/`seed` toman y devuelven un `DatabaseSync` vivo
(estado mutable, no JSON), así que el puente napi no puede grabarlos
como funciones puras. `seed` no tiene ni un solo caso válido grabado
(89/89 errores) y `openDatabase` solo 1 (arg `"x"`, que además creó un
directorio de pruebas `./x/` en la raíz, ya eliminado).
Decision: el port de `server/db` es un crate de librería Rust plano
`karaoke-db` en `rust/db/` (sin napi, sin bindings JS): `random_code`,
`new_id`, `open_database`, `seed` y el handle `Db` sobre rusqlite.
El andamio `rust/db-rs/` se conserva como artefacto de la grabación;
su `cases.jsonl` (140 casos) sigue siendo la fuente de verdad y el
replay es un test de integración del crate (`rust/db/tests/replay.rs`)
que lee ese archivo, con normalización solo para la aleatoriedad
(contenido de `random_code`/`new_id`: se compara longitud + charset +
prefijo, nunca el valor aleatorio). El gate de esta unidad es ese
replay 140/140 + `cargo test` + `cargo fmt --check` +
`cargo clippy -- -D warnings`, en lugar de `rust-anything diff/gate`
(que solo conocen crates puente).
Consequences: `app wire` de esta unidad se intentará con el crate
lib; si `wire` lo rechaza por no ser puente, se conecta a mano como
dependencia de `rust/app` y se registra aquí. Los casos de `seed` con
BD válida no existen en `cases.jsonl`: se cubren con tests de
integración propios (`rust/db/tests/seed.rs`, siembra real dos veces,
idempotencia, filas verificadas).

## ADR-011: Paridad de mensajes de error y formato numérico de Node
Status: accepted (2026-11-20)
Context: los casos grabados incluyen mensajes de error exactos de
Node 24 (`ERR_OUT_OF_RANGE`, `ERR_INVALID_ARG_TYPE`) y coerciones
`String()` de JS. `serde_json` no reproduce el formato numérico de JS
(zmij usa notación científica desde 1e16; JS desde 1e21 — ver
`rust/research/json-network.md` §3.2), así que el port no puede
delegar ese formato en `serde_json`.
Decision: el port implementa en `karaoke-db` (`src/js.rs`) la
semántica JS necesaria, verificada con sondeos en vivo contra Node 24
el 2026-11-20:
- `Number::toString` ECMAScript (vía dígitos más cortos de Rust
  `{:e}` + reglas de umbral JS: plano para 1e-6 ≤ |x| < 1e21).
- `ERR_OUT_OF_RANGE` de `crypto.randomBytes(size)`: el "Received"
  formatea el número con `_` insertado antes de cada posición a
  múltiplo de 3 caracteres desde el final de la cadena cuando la
  notación es exponencial (`1e+21` → `1e_+21`,
  `1.7976931348623157e+308` →
  `1._797_693_134_862_315_7e+_308`); para enteros decimales solo
  cuando |v| > 2^32 (`9007199254740992` → `9_007_199_254_740_992`,
  pero `2147483648` y `4294967296` quedan planos); decimales no
  enteros y `NaN`/`Infinity` quedan planos.
- `ERR_INVALID_ARG_TYPE` para `size` y `path` con el sufijo
  `Received` de Node (`type number (0)`, `type string ('abc')`,
  `type boolean (true)`, `null`, `undefined`, `an instance of
  Array`, `an instance of Object`).
- Coerción `String()` de JS para el prefijo de `new_id`
  (`undefined` → `"undefined"`, `{}` → `"[object Object]"`,
  `[3,1,2]` → `"3,1,2"`, `[]` → `""`, números con
  `Number::toString`).
Consequences: estas reglas viven en `karaoke-db::js` y las reusan
las unidades siguientes (auth, routes) para sus propios mensajes.

## ADR-012: Primitivas scrypt/sha256 en un crate hoja `karaoke-crypto`
Status: accepted (2026-11-20)
Context: `db.js` importa `hashPassword` de `auth.js`, y `auth.js`
usa la BD para las sesiones. En Rust eso sería un ciclo
db ↔ auth: `seed` (db) necesita scrypt y las sesiones (auth)
necesitan la BD.
Decision: las primitivas puras (`hash_password`, `verify_password`,
`token_hash`) viven en un crate hoja `karaoke-crypto` (`rust/crypto/`,
sin dependencia de BD). `karaoke-db` depende de él (para `seed`) y
`karaoke-auth` dependerá de él + `karaoke-db` (sesiones). Parámetros
fijos: scrypt `Params::new(14, 8, 1)`, salida 64 bytes, sal de 16
bytes guardada en hex; `token_hash` = sha256 de la cadena hex de 64
caracteres del token, no de los 32 bytes crudos; comparación con
`subtle::ct_eq` (ver `rust/research/storage-auth.md`).
Consequences: `Params::RECOMMENDED` (log_n=17, salida 32) está
prohibido: invalidaría todas las contraseñas existentes.

## ADR-014: `server/routes` como crate lib `karaoke-routes`, wire con rutas sin método
Status: accepted (2026-11-20)
Context: `server/index.js` no exporta ninguna función (los handlers
cierran sobre `db` y `room` y toman `(req, res)`), así que
`rust-anything bridge . server/index.js` falla: "unit
`server/index.js` exports no functions; bridge a unit with an
exported function API". Es el mismo callejón sin salida de ADR-010
(db) y ADR-013 (auth): el puente napi no puede representar esta
unidad. Además, el dispatch de Node no es un router por método: una
ruta conocida con método incorrecto da 404 (`GET /api/songs/<id>`
→ `No encontrado` vía estáticos; `PATCH /api/songs/` con invitado →
403 antes de mirar el id), mientras que axum devolvería 405; y los
ids son segmentos crudos sin decodificar (`p.split('/')[3]`), no
parámetros decodificados. Los casos grabados lo exigen: trampas
`GET /api/songs/<unicode>` → 404 `No encontrado`,
`PATCH /api/songs/` (invitado) → 403, `DELETE /api/queue/` → 404
`Elemento de cola no encontrado.`
Decision: `server/routes` se porta como crate lib plano
`karaoke-routes` (`rust/routes/`), sin andamio napi. El crate expone
`AppState` (Db + Room + dir público + cliente reqwest), `init` (abre
la BD con `DATA_DIR` y siembra con las `KARAOKE_*`, igual que
`index.js`) y `dispatch` (cadena if/else sobre método + pathname
crudo normalizado estilo WHATWG URL, igual que `handle` de Node).
Se conecta con `rust-anything app wire` con las rutas SIN método
(`--route '/api/songs'`, `--route '/api/songs/{*rest}'`, `--route
'*'` para el catch-all): wire genera handlers `any()` y el glue
`rust/app/src/units/karaoke_routes.rs` delega en el crate; el estado
vive en un `OnceLock` global inicializado perezosamente por el glue
en la primera request (corrección 2026-11-20: no en `main.rs`, para
no tocar código que `wire` no regenera; la primera request llega
después de arrancar el original, que ya sembró la BD de pruebas).
El gate de la unidad es el replay de la app
295/295 con fallback 0 + `cargo test` + fmt + clippy, como en
ADR-010/013. El streaming por rangos se implementa de forma nativa
con `tokio::fs` + stream (nunca en memoria); oEmbed con reqwest
0.12 + `rustls-tls` (para no mezclar reqwest 0.13 con el 0.12 del
front door) y `no_proxy()` (Node `fetch` no usa proxy del entorno).
Consequences: `app wire` se usa con `--skip-gate` (el gate de
`rust-anything` no conoce crates lib). El lift puede no reconocer
este gate; si se niega, se reportan sus bloqueos sin forzarlo.

## ADR-015: scrypt usa la sal hex COMO CADENA (no los bytes crudos)
Status: accepted (2026-11-20, corrección de ADR-012)
Context: `hashPassword`/`verifyPassword` de `server/auth.js` pasan
la sal hex como cadena a `crypto.scryptSync(password, salt, 64)`:
Node usa los bytes UTF-8 de la cadena (32 bytes), no los 16 bytes
que resultan de decodificar el hex. El port inicial de
`karaoke-crypto` hacía `hex::decode` (16 bytes) y el `login-good`
del replay del front door falló con 401 (`Usuario o contraseña
incorrectos.`) aunque el original aceptaba la misma contraseña. El
bug pasó desapercibido porque los vectores de `auth-record.mjs` se
generaron con `Buffer.from(saltHex, 'hex')` (sal cruda), no con la
cadena que `verifyPassword` realmente usa — el replay 9/9 de auth
verificaba la convención equivocada. Verificado: el hash del admin
sembrado (`AdminTest123!`, sal `26cd762c…`) es el scrypt con la sal
como cadena, no con la sal cruda.
Decision: `hash_password` y `verify_password` en `karaoke-crypto`
usan `salt_hex.as_bytes()` como sal del KDF. La sal ya no se valida
como hex (Node acepta cualquier cadena no vacía); el hash sí debe
decodificar a 64 bytes (o `timingSafeEqual` lanzaría en Node).
`auth-record.mjs` regenera su vector con la cadena y
`auth-cases.jsonl` se regeneró (hash `0c5f3622…` para
`Secreta123!`/sal `000102…0f`). Nuevo test
`verify_password_matches_node_string_salt` en crypto.
Consequences: las contraseñas creadas por Node verifican en Rust y
al revés; los admins sembrados por el `seed` de Rust ahora usan la
misma convención que los de Node. Los dos vectores KDF en crudo de
crypto siguen válidos (verifican el KDF, no la convención).

## ADR-016: wire reemplaza la lista de rutas; replay/wire con `DATA_DIR`; lift bloqueado
Status: accepted (2026-11-20)
Context: al cablear `server/routes` en tres tandas (exactas,
`{*rest}`, catch-all `*`), la segunda tanda dejó solo 4 rutas: `app
wire --route` REEMPLAZA la lista completa; solo se conserva la
anterior si se omite `--route` por completo. Además, el replay
interno de `wire` y `app replay --build <front door>` corren el
front door sin `DATA_DIR`, y el glue usaría `<ROOT>/data` (la BD
real) mientras el original usa `record-data` — los casos que
necesitan el estado sembrado fallan (401 en vez de 404). El lift se
intentó con fallback 0 y se negó con dos bloqueos.
Decision: cada `wire` pasa la lista COMPLETA de rutas (18: 13
exactas + 4 `{*rest}` + `*`). Todo replay/wire del front door se
invoca con `DATA_DIR=$PWD/rust/record-data`. El lift queda
BLOQUEADO (no forzado) por: (1) `karaoke-routes: gate fails` — no
existe `rust/routes/rust-anything.json` porque el crate no es
puenteable (su gate equivalente es el replay 295/295 + 15 tests
cargo); (2) `src/main.rs` todavía llama al original — quitarlo a
mano quitaría también la única re-siembra de `record-data` para el
replay (la hace `record-server.sh` como hijo del original), así que
el binario puro necesitaría su propia historia de reseteo de BD
antes de que el lift pueda replayar en verde. El front door sigue
arrancando el original (que siembra) y el kill switch
`RUST_ANYTHING_UNIT_KARAOKE_ROUTES=original` sigue disponible.
Consequences: el merge a main espera al lift; el release v2.0.0
también. La CI (`.github/workflows/rust.yml`) no incluye replay
(solo fmt/clippy/test/build release en Linux+Windows).

Actualización 2026-11-20: los dos bloqueos se resolvieron
(ADR-017 y ADR-018) y el lift pasó con el tool. Esta ADR queda
como registro histórico del bloqueo; sus decisiones de wire y
`DATA_DIR` siguen vigentes.

## ADR-013: `server/auth` también como crate lib plano, sin puente napi
Status: accepted (2026-11-20)
Context: igual que ADR-010 para `server/db`: `createSession`,
`getUserFromToken` y `destroySession` toman un `DatabaseSync` vivo
como primer argumento, que el puente napi no puede representar en
JSON. Solo `hashPassword`, `verifyPassword` y `publicUser` son
puras. Intentar el puente repetiría el callejón sin salida de db
(la mayoría de casos solo error).
Decision: `server/auth` se porta directamente como crate lib
`karaoke-auth` (`rust/auth/`), sin andamio napi. Las primitivas
puras se re-exportan desde `karaoke-crypto` (ADR-012); las sesiones
se implementan sobre `karaoke-db::Db`. La caracterización es un
script Node (`rust/auth-record.mjs`) que ejerce `server/auth.js`
contra una BD temporal y vuelca casos a `rust/auth-cases.jsonl`
(valores deterministas tal cual; aleatorios como forma), más un
replay en Rust (`rust/auth/tests/replay.rs`) y tests de integración
de expiración/limpieza.
Consequences: no hay `rust/auth-rs/` ni `cases.jsonl` de
`rust-anything record` para esta unidad; la fuente de verdad son
`auth-cases.jsonl` + los tests.

## ADR-017: Gate del lift para crates lib — `src/port.rs` real en `karaoke-routes`

Context: el lift se negó con `karaoke-routes: gate fails (ported)`.
Leyendo `lift.rs`/`gate.rs` en `~/workspace/tools/rust-anything/repo/src/`:
el check por unidad es — si existe `rust-anything.json`, corre
`gate(dir)`; si no, corre `gate::rust_checks(dir)` (fmt, clippy,
cargo test, unsafe) **más `gate::ported_check(dir)`**, que lee
`src/port.rs` y falla si no existe o si contiene stubs
(`unimplemented!`/`todo!`/panic-only). Un crate lib plano no tiene
`src/port.rs` porque el modelo del tool asume puentes napi. Fabricar
un `rust-anything.json` falso está prohibido: no hay forma honesta
de hacer puenteable un crate cuya API es `dispatch` sobre HTTP.
Decision: añadir `rust/routes/src/port.rs` como módulo REAL, declarado
en `lib.rs`: fachada pública del port que re-exporta la API que usa
el front door (`AppState`, `dispatch`, `serve`) y los módulos del
port. El check `ported` pasa en verdad porque ningún módulo del
crate tiene stubs (lo verifican `cargo test` — 15 tests — y el replay
295/295 de la app). No es un andamio vacío ni un manifiesto
fabricado: es código compilado y su docstring explica por qué existe.
Además, el lift del binario release 1.5.1 NO copia al stage las
dependencias por ruta (`rust/db`, `rust/auth`, `rust/crypto` faltaron
en el stage y el checklist falló); el lift debe correrse con el build
desde fuente de `main` (`~/workspace/tools/rust-anything/from-source/bin/rust-anything`),
que sí las copia (`path_dependencies`).
Consequences: el lift reconoce el gate del crate con su propio
mecanismo (`rust_checks` + `ported`), sin fabricar nada. El gate
equivalente del crate sigue siendo replay 295/295 + 15 tests cargo,
ahora refrendado por el check del tool.

## ADR-018: Binario puro — reseteo de BD de pruebas en Rust, `main.rs` sin original, y el stage del lift

Context: `main.rs` arrancaba el servidor Node porque la re-siembra
de `record-data` para el replay la hacía `record-server.sh` como
hijo del original. Quitar el original sin más dejaba al binario puro
sin historia de reseteo. Además, el lift reveló dos problemas más
del stage (copia temporal en `/tmp/rust-anything-lift-*`): (1) el
binario del stage se compila con `ROOT` temporal y sin `public/`, así
que los 7 casos de estáticos daban 404 en el replay final; (2) los
tests de replay de `karaoke-db` y `karaoke-auth` leían sus
`cases.jsonl` fuera del crate (`rust/db-rs/cases.jsonl`,
`rust/auth-cases.jsonl`), que el stage no copia.
Decision:
- `karaoke_db::reset_record_data` (`rust/db/src/record.rs`, 3 tests
  nuevos): espejo en Rust de `rust/seed-record-db.mjs` — borra el
  directorio y lo vuelve a sembrar con los valores fijos (admin
  `admin`/`AdminTest123!`, sala `room_seed001`/`KMTEST`, invitado
  `u_guest001`, sesiones de tokens conocidos, 3 canciones, 2 colas,
  2 solicitudes, vídeo de 42 bytes). Guarda: se niega a borrar un
  directorio que no se llame exactamente `record-data`, así que la BD
  real (`data/`) nunca puede tocarse por error.
- `main.rs` puro: sin `mod original`, sin llamadas al original,
  `units::router()` sin estado; el router es la forma levantada
  (`mod.rs` reescrito por el lift con su versión canónica, sin kill
  switches). Mantiene el toque al archivo de fallback
  (`RUST_ANYTHING_FALLBACK_LOG`): crearlo vacío reporta "0
  fallbacks" al replay, que es verdad (no hay a dónde caer). El
  reseteo solo corre con `KARAOKE_RESET_RECORD_DB` fijada; en
  producción el binario abre `DATA_DIR` (o `data/`) sin tocarla.
- `KARAOKE_PUBLIC_DIR` en el glue (`public_dir()`): si está fijada,
  sirve estáticos desde ahí; si no, `<ROOT>/public` como siempre
  (comportamiento de producción sin cambios). Solo existe para que el
  binario del stage sirva el `public/` real.
- Copias de los casos dentro de los crates
  (`rust/db/tests/cases.jsonl`, 140 casos;
  `rust/auth/tests/auth-cases.jsonl`, 9 casos), con assert de conteo
  para que una copia desactualizada falle en voz alta. Los originales
  (`db-rs/cases.jsonl`, `auth-cases.jsonl`) siguen siendo los
  artefactos de la grabación; si se regraban hay que refrescar las
  copias.
- Invocación del lift (binario desde fuente):
  `DATA_DIR=$PWD/rust/record-data KARAOKE_RESET_RECORD_DB=1 KARAOKE_PUBLIC_DIR=$PWD/public rust-anything app lift .`
Consequences: el lift pasó con el tool (295/295, fallback 0%).
`rust/app/src/original.rs` lo borró el lift al sincronizar;
`rust/app/rust-anything.json` marca `"lifted": true`; `reqwest` salió
de `rust/app/Cargo.toml`. El binario release es `karaokemuse` 8.2 MB
y NO necesita Node ni el código Node presente. El servidor Node
(`server/`) queda en el repo solo como referencia hasta que Cosio33
decida el cutover (no se borra). `record-server.sh` y
`seed-record-db.mjs` quedan como artefactos históricos de la
grabación; el reseteo vivo es el de Rust.

## ADR-019: `ROOT` en tiempo de ejecución + mensaje al arrancar
Status: accepted (2026-11-23)
Context: el binario publicado no arrancaba bien fuera del repo: `ROOT` era
una constante de compilación (`CARGO_MANIFEST_DIR/../..`) que apuntaba a
la ruta del servidor de CI, inexistente en la máquina del usuario, así que
`public/` no se encontraba y `GET /` daba 404 aunque el servidor estaba
vivo. Además el binario no imprimía nada al arrancar y la terminal quedaba
en negro, igual que si no hubiera arrancado.
Decision: `ROOT` se resuelve en tiempo de ejecución (`runtime_root()` en
`main.rs` y en `units/karaoke_routes.rs`): primera carpeta con `public/`
entre la carpeta del ejecutable, la carpeta actual y, por último, la raíz
de compilación. `KARAOKE_PUBLIC_DIR` y `DATA_DIR` siguen teniendo prioridad
cuando están fijadas. Al arrancar, el binario imprime en stderr
`[karaokemuse] Servidor en http://localhost:<puerto>`, como la v1 Node.
Consequences: el binario suelto funciona con `public/` a su lado, sin
variables; el replay y los tests no cambian (usan las variables).
