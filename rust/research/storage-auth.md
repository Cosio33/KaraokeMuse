# KaraokeMuse — Rust storage + auth crates: research notes

Scope: replacing `node:sqlite` (Node 24, `DatabaseSync`) + `node:crypto`
(`scryptSync`, `timingSafeEqual`, `createHash('sha256')`, `randomBytes`) with
`rusqlite` + `scrypt` + `sha2` (+ `rand`, `hex`, `subtle`) in a standalone
Rust binary. Read-only research; no code was changed.

**Verified:** 2026-11-20, against docs.rs crate pages and the official
rusqlite / RustCrypto / rust-random GitHub repos (via docs.rs source views),
plus nodejs.org and sqlite.org for the Node/SQLite side. Every claim below
carries its source URL.

---

## 1. Versions, MSRV, features

| Crate | Current stable (2026-11-20) | MSRV | Repo |
|---|---|---|---|
| `rusqlite` | **0.43.0** | **1.89.0** (edition 2024) | https://github.com/rusqlite/rusqlite |
| `scrypt` | **0.12.0** (2026-04-22) | **1.85** (edition 2024) | https://github.com/RustCrypto/password-hashes/tree/master/scrypt |
| `sha2` | **0.11.1** | **1.88** (edition 2024) | https://github.com/RustCrypto/hashes |
| `rand` | **0.10.4** (0.10 released Feb 2026; 0.9.4 is the previous minor line) | **1.85** (edition 2024) | https://github.com/rust-random/rand |
| `hex` (support) | **0.4.3** | no `rust-version` declared; edition 2018 | https://github.com/KokaKiwi/rust-hex |
| `subtle` (support) | **2.6.1** | **1.41** | https://github.com/dalek-cryptography/subtle |

Sources:

- rusqlite 0.43.0 and MSRV 1.89.0: https://docs.rs/crate/rusqlite/latest/source/Cargo.toml.orig
  (`version = "0.43.0"`, `edition = "2024"`, `rust-version = "1.89.0"`).
- scrypt 0.12.0 (2026-04-22), MSRV 1.85: https://docs.rs/crate/scrypt/latest
  and https://docs.rs/crate/scrypt/latest/source/Cargo.toml.orig
  (`version = "0.12.0"`, `rust-version = "1.85"`).
- sha2 0.11.1, MSRV 1.88: https://docs.rs/crate/sha2/latest
  and https://docs.rs/crate/sha2/latest/source/Cargo.toml.orig
  (`version = "0.11.1"`, `rust-version = "1.88"`). Note: the docs.rs badge
  says "rustc 1.85+", but the crate's own `Cargo.toml` declares `1.88` —
  treat **1.88** as the floor.
- rand 0.10.4, MSRV 1.85, "Version 0.10 was released in February 2026":
  https://docs.rs/crate/rand/latest
  and https://docs.rs/crate/rand/latest/source/Cargo.toml.orig
  (`version = "0.10.4"`, `rust-version = "1.85"`).
- hex 0.4.3: https://docs.rs/crate/hex/latest
  and https://docs.rs/crate/hex/latest/source/Cargo.toml.orig
  (`version = "0.4.3"`, no `rust-version` field).
- subtle 2.6.1, MSRV 1.41: https://docs.rs/crate/subtle/latest
  ("Minimum Supported Rust Version: Rust **1.41** or higher").

**Effective toolchain floor for the port is Rust 1.89**, set by rusqlite
0.43.0. The installed toolchain in this VM is rustc/cargo 1.100.0, so this
is not a constraint in practice, but pin it in CI (`rust-version = "1.89"`
or a `rust-toolchain.toml`) so a future rusqlite bump cannot silently raise
the floor.

### Exact Cargo features needed

```toml
rusqlite = { version = "0.43", features = ["bundled"] }
scrypt   = { version = "0.12", default-features = false }
sha2     = "0.11"
rand     = "0.10"
hex      = "0.4"
subtle   = "2.6"
```

- `rusqlite` — only `bundled` is required. Default features are `cache`
  and `ffi-sqlite-wasm-rs` (the latter is a no-op on native targets).
  No `chrono`/`time`/`serde_json` feature is needed: KaraokeMuse stores
  dates as ISO-8601 TEXT and does its own JSON, so keep them as plain
  `String`s. Source: https://docs.rs/crate/rusqlite/latest/source/Cargo.toml.orig
  (`default = ["cache", "ffi-sqlite-wasm-rs"]`, `bundled = ["libsqlite3-sys?/bundled", "modern_sqlite"]`).
- `scrypt` — `default-features = false` is the documented recommendation
  when only the low-level `scrypt()` function is used (no PHC/MCF string
  formats). The `parallel` feature (rayon) is optional; it only parallelises
  the ROMix over `p` lanes, and with `p = 1` it buys nothing. Do **not**
  enable `phc`/`mcf`: `Scrypt::hash_password` produces `$scrypt$…` PHC
  strings, a different format from Node's raw 64-byte output.
  Source: https://docs.rs/crate/scrypt/latest/source/src/lib.rs
  ("If you are only using the low-level `scrypt` function … it's recommended
  to disable default features") and https://docs.rs/crate/scrypt/latest/source/Cargo.toml.orig
  (`phc = ["password-hash/phc"]`, `parallel = ["dep:rayon"]`).
- `sha2` — no features needed; defaults are `alloc` + `oid`.
  Source: https://docs.rs/crate/sha2/latest/source/Cargo.toml.orig
  (`default = ["alloc", "oid"]`).
- `rand` — defaults (`std`, `std_rng`, `sys_rng`, `thread_rng`) cover
  everything below. No extra feature is needed for `rand::rng()` or
  `SysRng`. Source: https://docs.rs/crate/rand/latest/source/Cargo.toml.orig
  (`default = ["std", "std_rng", "sys_rng", "thread_rng"]`).
- `hex` — defaults. `hex::encode` / `hex::decode` are all that is needed.
  Source: https://docs.rs/crate/hex/latest.
- `subtle` — defaults, no features needed for `ConstantTimeEq`.
  Source: https://docs.rs/crate/subtle/latest.

**Dependency coherence bonus:** `scrypt` 0.12 depends on `sha2` 0.11
internally, so pinning `sha2 = "0.11"` directly does not introduce a
second major version of the crate.
Source: https://docs.rs/crate/scrypt/latest/source/Cargo.toml.orig
(`sha2 = { version = "0.11", default-features = false }`).

### Does `bundled` vendor SQLite? Which version?

Yes. With `bundled`, `libsqlite3-sys` uses the `cc` crate to compile
SQLite from source embedded in the `libsqlite3-sys` crate and statically
link it into the binary — no system `libsqlite3` is needed at build or
run time. As of rusqlite 0.43.0 / libsqlite3-sys 0.40.0 the bundled
SQLite is **3.54.1**. Without `bundled`, `libsqlite3-sys` links the
system SQLite via pkg-config/vcpkg and rusqlite requires SQLite ≥ 3.45.3.
Source: https://docs.rs/crate/rusqlite/latest
("`libsqlite3-sys` will use the `cc` crate to compile SQLite … from
source … and is currently SQLite 3.54.1 (as of `rusqlite` 0.43.0 /
`libsqlite3-sys` 0.40.0)" and "The base `rusqlite` package supports
SQLite version 3.45.3 or newer").

Build-time consequence: `bundled` needs a working C compiler on the build
machine (that is what `cc` invokes). On a Windows PC that means a
C toolchain (MSVC Build Tools or MinGW) must be present for `cargo build`;
on GitHub Actions the standard runners already have one. This is the
only new build prerequisite the port adds.

### Does a `bundled` database file open identically in `node:sqlite` and vice versa?

**Yes.** The on-disk format is a property of SQLite itself, not of the
wrapper, and it has been fully backwards-compatible since SQLite 3.0.0
(2004): "newer versions of SQLite can always read and write database
files created by older versions of SQLite", and older versions can read
files created by newer ones as long as no newer feature (e.g. WAL, added
in 3.7.0) is in use. Source: https://www2.sqlite.org/matrix/formatchng.html
and https://www.sqlite.org/fileformat2.html ("This document describes …
the on-disk database file format used by all releases of SQLite since
version 3.0.0").

The two engines involved are close in version anyway: the official Node 24
binary statically bundles SQLite **3.53.4** (Node 24.18.0 / 24.20.0; later
24.22.x patch releases moved to 3.54.1), while rusqlite `bundled` ships
3.54.1. Sources: https://sqldocs.org/sqlite-nodejs/
("Tested with Node.js 24.18.0 (node:sqlite, SQLite 3.53.4)") and
https://docs.rs/crate/rusqlite/latest (bundled 3.54.1, above). Both are
far above the WAL threshold (3.7.0), so a WAL-mode `karaokemuse.sqlite`
created by either side opens in the other with no migration, no dump /
reload, and no schema change. The 7 tables, indexes, triggers and
`sqlite_sequence` (if any) are byte-identical in meaning.

Two operational caveats (same in both directions):

1. **WAL is a persistent property of the database file** (header bytes 18–19
   flip from 1 to 2 in WAL mode), so once Node has run
   `PRAGMA journal_mode = WAL`, the Rust side inherits WAL mode on open
   and does not need to re-issue the pragma — but re-issuing it is harmless.
   Source: https://www.sqlite.org/fileformat2.html (header offsets 18–19:
   "File format write version. 1 for legacy; 2 for WAL").
2. **The `-wal` and `-shm` sidecar files are part of the database state.**
   Copying/moving only `karaokemuse.sqlite` while a writer holds
   un-checkpointed WAL frames loses committed data. Always close the
   connection cleanly (or checkpoint) before copying the file, and move
   all three files together. Source: https://www.sqlite.org/fileformat2.html
   ("During a transaction, SQLite stores additional information in a
   second file called the 'rollback journal', or if SQLite is in WAL mode,
   a write-ahead log file").

---

## 2. rusqlite synchronous API mapping (Node `DatabaseSync` → rusqlite)

rusqlite is synchronous by design — `Connection` methods block, exactly
like `node:sqlite`'s `DatabaseSync`. There is no async API to avoid.
Source: https://docs.rs/crate/rusqlite/latest ("Rusqlite is an ergonomic
wrapper for using SQLite from Rust") and the `Connection` API page
https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html.

| Node (`node:sqlite`) | Rust (`rusqlite` 0.43) | Notes |
|---|---|---|
| `new DatabaseSync("karaokemuse.sqlite")` | `Connection::open("karaokemuse.sqlite")?` | Creates the file if missing; returns `Result<Connection>`. `Connection::open_in_memory()` is the `:memory:` equivalent. Source: Connection docs, `open` / `open_in_memory`. |
| `db.exec("PRAGMA …; CREATE TABLE IF NOT EXISTS …")` | `conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; CREATE TABLE IF NOT EXISTS …;")?` | `execute_batch` runs **multiple statements, no parameters allowed**. The docs example shows exactly this shape (`BEGIN; CREATE TABLE …; COMMIT;`). Source: Connection docs, `execute_batch`. |
| `db.prepare(sql)` | `conn.prepare(sql)?` → `Statement` | Reusable; `prepare_cached` (default `cache` feature) returns a cached statement. Source: Connection docs, `prepare` / `prepare_cached`. |
| `stmt.get(?…)` (one row) | `conn.query_row(sql, params, \|row\| row.get(0))?` | Returns `Err(Error::QueryReturnedNoRows)` when empty; extra rows are ignored. Append `.optional()?` (via `OptionalExtension`) to get `Result<Option<T>>`. `query_one` is the strict variant (errors on >1 row). Source: Connection docs, `query_row` / `query_one`. |
| `stmt.all(?…)` (all rows) | `let mut stmt = conn.prepare(sql)?; let rows = stmt.query_map(params, \|row\| …)?;` then collect the iterator | `query_map` returns a lazy `MappedRows` iterator; each item is `Result<T>`. There is no single `query_all` convenience — collect it yourself. Source: crate docs example (`stmt.query_map([], …)`) at https://docs.rs/crate/rusqlite/latest. |
| `stmt.run(?…)` (INSERT/UPDATE/DELETE) | `conn.execute(sql, params)?` → `usize` | Returns rows changed/inserted/deleted (`sqlite3_changes`). For a prepared statement, `stmt.execute(params)?`. Source: Connection docs, `execute`. |
| `?` placeholders | `?`, `?1`, `?2`, … and named `:name` / `@name` / `$name` | Positional `?` works exactly as in Node. Pass params as a tuple, array, slice, or the `params![…]` macro for mixed types. Source: Connection docs, `execute` examples (`?1`, `params![1i32, 1.5f64]`, `:name`). |
| Reading columns | `row.get::<_, T>(idx_or_name)?` | Index (`row.get(0)`) or name (`row.get("id")`). Types via `FromSql` (see §2.1). Source: crate docs example; https://docs.rs/rusqlite/latest/rusqlite/types/index.html. |
| `db.exec("BEGIN")` / manual transactions | `let tx = conn.transaction()?; … tx.commit()?;` | `transaction()` takes `&mut self` (compile-time guard against nesting) and defaults to DEFERRED; **dropping `tx` without `commit()` rolls back**. `Transaction` derefs to `Connection`, so `tx.execute(…)` / `tx.query_row(…)` work inside it. `set_transaction_behavior(TransactionBehavior::Immediate)` changes the default. Source: Connection docs, `transaction` ("The transaction defaults to rolling back when it is dropped. If you want the transaction to commit, you must call `commit`"). |
| `PRAGMA journal_mode = WAL` (and reading it back) | `conn.pragma_update(None, "journal_mode", "WAL")?;` / `conn.pragma_query_value(None, "journal_mode", \|r\| r.get::<_, String>(0))?` | `pragma_update` sets, `pragma_query_value` reads a single value. `execute_batch` with the PRAGMA text also works and matches the Node code most directly. Source: Connection docs, `pragma_update` / `pragma_query_value`. |
| Errors | `rusqlite::Error` (enum, `#[non_exhaustive]`) | Key variants: `SqliteFailure(ffi::Error, Option<String>)` (constraint violations etc. carry the SQLite extended code — use this to detect UNIQUE violations instead of string-matching), `QueryReturnedNoRows`, `QueryReturnedMoreThanOneRow`, `InvalidColumnType`, `IntegralValueOutOfRange`, `FromSqlConversionFailure`, `InvalidParameterCount`, `ExecuteReturnedResults`. Because the enum is `#[non_exhaustive]`, `match` arms need a wildcard. Source: https://docs.rs/rusqlite/latest/rusqlite/enum.Error.html. |
| Busy/locking | `conn.busy_timeout(Duration::from_millis(5000))?` | **New connections default to a 5000 ms busy timeout** in rusqlite. Node's `DatabaseSync` has a `timeout` option (added v24.0.0); make sure the port sets the same value Node used, or explicitly keeps 5000 ms. Source: Connection docs, `busy_timeout` ("Newly created connections currently have a default busy timeout of 5000ms"). |
| Closing | `conn.close()` or just drop | `close()` returns the connection back on error so closing can be retried; dropping is the normal path. Source: Connection docs, `close`. |

### 2.1 Reading TEXT / INTEGER columns

- **TEXT → `String` / `&str`**: `row.get::<_, String>(i)?`. Invalid UTF-8
  in a TEXT column yields `Error::Utf8Error`. Node would return the same
  bytes as a JS string (lossy); rusqlite is strict — a corrupt TEXT value
  fails the row instead of silently mangling it.
  Source: https://docs.rs/rusqlite/latest/rusqlite/types/index.html
  and https://docs.rs/rusqlite/latest/rusqlite/enum.Error.html (`Utf8Error`).
- **INTEGER → `i64` (preferred), `i32`, `u32`, `bool`, …**: `FromSql` is
  implemented for all primitive integer types. `INTEGER` → integer errors
  with `IntegralValueOutOfRange` if the value does not fit the target
  Rust type; `REAL` → integer always errors `InvalidColumnType`;
  `INTEGER` → float casts with `as` and never fails.
  Source: https://docs.rs/rusqlite/latest/rusqlite/types/index.html.
- **NULL → `Option<T>`**: `FromSql`/`ToSql` are implemented for
  `Option<T>`; NULL maps to `None`. A non-nullable `row.get::<_, T>` on a
  NULL column fails — mirror Node's `null` handling explicitly.
  Source: https://docs.rs/rusqlite/latest/rusqlite/types/index.html.
- **BLOB → `Vec<u8>` / `&[u8]`**.
- **`u64` / `usize` are NOT implemented** for `FromSql`/`ToSql` by design
  (SQLite integers are signed 64-bit). Either fetch as `i64` and convert,
  or enable the `fallible_uint` feature (stores only values ≤ `i64::MAX`).
  KaraokeMuse ids/counts fit in `i64`, so plain `i64` is enough.
  Source: https://docs.rs/rusqlite/latest/rusqlite/types/index.html
  ("`FromSql` and `ToSql` are defined for most integral types, but not
  `u64` or `usize`") and https://docs.rs/crate/rusqlite/latest
  (`fallible_uint` feature description).
- **`bool` ↔ INTEGER 0/1**: `FromSql for bool` maps `0 → false` and any
  other integer → `true`; `ToSql for bool` stores `1`/`0`. This is exactly
  how Node stores JS booleans through `node:sqlite`, so `is_favorite`
  round-trips unchanged. Trap: any nonzero integer reads as `true`, so a
  stray `2` in that column (possible under SQLite's loose typing, see §7)
  would silently become `true` — same as Node's truthiness, but worth a
  `CHECK (is_favorite IN (0, 1))` if the schema is ever regenerated.
  Source: https://docs.rs/rusqlite/0.15.0/src/rusqlite/types/from_sql.rs.html
  (`impl FromSql for bool { … 0 => false, _ => true }`) and
  https://docs.rs/rusqlite/latest/rusqlite/types/trait.FromSql.html
  (`impl FromSql for bool` listed).

### 2.2 Concurrency shape

`rusqlite::Connection` is `Send` but **not `Sync`** — one connection
cannot be shared across threads without a `Mutex`. The Node original is
also single-connection and synchronous, so the faithful port is
`Mutex<Connection>` (or one connection per thread). Do not open a second
connection to the same WAL file for writes from another thread without
understanding SQLite locking; the 5000 ms busy timeout is what saves the
single-writer case.
Source: rusqlite docs / `Connection` API (single-threaded design is
implicit in the `&mut self` transaction API and the `SqliteSingleThreadedMode`
error variant at https://docs.rs/rusqlite/latest/rusqlite/enum.Error.html).

---

## 3. scrypt — reproducing `crypto.scryptSync(password, salt, 64)` exactly

### 3.1 Node's parameters (the ground truth)

`crypto.scryptSync(password, salt, keylen[, options])` with no `options`
uses **cost `N` = 16384, block size `r` = 8, parallelization `p` = 1**,
and `maxmem` = 32 MiB (irrelevant to the output, only a guard: it errors
if `128 * N * r > maxmem`). Source: https://nodejs.org/docs/v24.21.0/api/crypto.html
(`crypto.scryptSync` — "CPU/memory cost parameter. Must be a power of two
greater than one. **Default:** `16384`", "Block size parameter.
**Default:** `8`", "Parallelization parameter. **Default:** `1`",
"Memory upper bound. … **Default:** `32 * 1024 * 1024`").

### 3.2 The Rust equivalent

```rust
use scrypt::{Params, scrypt};

let params = Params::new(14, 8, 1).expect("valid scrypt params"); // log_n=14 → N = 2^14 = 16384
let mut out = [0u8; 64];
scrypt(password.as_bytes(), &salt_bytes, &params, &mut out)?;
let hash_hex = hex::encode(out);
```

- `Params::new(log_n, r, p)` takes **log₂(N)**, not N: `log_n = 14`
  gives `N = 16384`. `Params::n()` returns `1 << log_n`.
  Source: https://docs.rs/crate/scrypt/latest/source/src/params.rs
  (`pub fn new(log_n: u8, r: u32, p: u32)`, `pub const fn n(&self) -> u64 { 1 << self.log_n }`).
- **Output length is determined by the size of the `output` buffer**, not
  by `Params`. A 64-byte buffer reproduces `keylen = 64`. The `len` field
  inside `Params` (set via `new_with_output_len`) is ignored by the
  low-level `scrypt()` function.
  Source: https://docs.rs/crate/scrypt/latest/source/src/lib.rs
  ("The output size is determined entirely by size of the `output`
  parameter. If the length of the `Params` have been customized using
  `Params::new_with_output_len` … that length is ignored").
- **Salt**: the stored salt is 16 random bytes **hex-encoded** (32 hex
  chars). Hex-**decode** it back to the 16 raw bytes before calling
  `scrypt()` — do not pass the 32-char hex string as the salt, or every
  stored hash becomes unverifiable. `hex::decode(stored_salt_hex)?`.
  Source: https://docs.rs/crate/hex/latest (`hex::decode`).
- **Password**: Node passes a JS string, which `scryptSync` encodes as
  UTF-8 bytes. In Rust that is `password.as_bytes()` on the `&str` — the
  same bytes, provided the password was not normalized. Node does **not**
  normalize Unicode before key derivation, so do not normalize in Rust
  either (see §7).
  Source: https://nodejs.org/docs/v24.21.0/api/crypto.html ("Node.js does
  not normalize character representations" before passing user input to
  key derivation functions such as scrypt).
- Errors: `scrypt()` returns `Result<(), errors::InvalidOutputLen>`, which
  only fires for an empty output buffer or one longer than
  `(2^32 − 1) * 32` bytes — a 64-byte buffer cannot fail.
  Source: https://docs.rs/crate/scrypt/latest/source/src/lib.rs.

### 3.3 ⚠️ The big trap: do NOT use `Params::RECOMMENDED` / `Scrypt::default()`

The scrypt crate's recommended parameters are **`log_n = 17` (N = 131072),
`r = 8`, `p = 1`, output length 32** — eight times the cost of Node's
defaults, and a different output length.
Source: https://docs.rs/crate/scrypt/latest/source/src/params.rs
(`RECOMMENDED_LOG_N: u8 = 17`, `RECOMMENDED_R: u32 = 8`,
`RECOMMENDED_P: u32 = 1`, `RECOMMENDED_LEN: usize = 32`).
Using `Params::RECOMMENDED` (or `Scrypt::default()`, which uses it) would
produce a completely different 32-byte hash for the same password+salt,
and **every existing password in `karaokemuse.sqlite` would stop
verifying** — with no error, just `false` on every login. The port must
hard-code `Params::new(14, 8, 1)` and a 64-byte output, and the
characterization tests must assert byte-equality against a hash produced
by the Node original before the port is trusted.

### 3.4 Constant-time comparison (replacing `timingSafeEqual`)

Node: `crypto.timingSafeEqual(a, b)` compares bytes in constant time and
**throws if the two buffers have different lengths**.
Source: https://nodejs.org/docs/v24.21.0/api/crypto.html
("This function compares the underlying bytes … using a constant-time
algorithm", "`a` and `b` must … have the same byte length. An error is
thrown if `a` and `b` have different byte lengths").

Rust equivalent, with the `subtle` crate (2.6.1):

```rust
use subtle::ConstantTimeEq;

let ok: bool = candidate_hash.ct_eq(stored_hash).into(); // both &[u8; 64]
```

- `ct_eq` on slices returns a `Choice` (1 = equal, 0 = not); convert with
  `bool::from(...)` / `.into()`. The crate's own docs warn it is a
  best-effort construction and is intended for release builds.
  Source: https://docs.rs/crate/subtle/latest ("Pure-Rust traits and
  utilities for constant-time cryptographic implementations", "This crate
  is intended to be used in release mode").
- **Length behaviour differs**: `subtle`'s slice `ct_eq` **short-circuits
  (returns false immediately) when the lengths differ**, leaking only the
  length — not the contents. That is safe here because both sides are
  always exactly 64 bytes, but do not rely on it as a length check:
  compare decoded byte arrays (`[u8; 64]`), not hex strings, and reject
  a stored hash that does not hex-decode to exactly 64 bytes *before*
  comparing. The scrypt crate's own `scrypt()` docs carry the same
  warning: "WARNING: Make sure to compare this value in constant time!"
  Source: https://docs.rs/crate/scrypt/latest/source/src/lib.rs and
  https://docs.rs/subtle/2.4.1/source/src/lib.rs (slice `ct_eq`:
  "This function short-circuits if the lengths of the input slices are
  different").
- **Do not use `==` on the hashes or on the hex strings.** `==` on
  `[u8; 64]` short-circuits on the first differing byte and leaks the
  position of the first mismatch through timing — exactly what
  `timingSafeEqual` was preventing.

---

## 4. sha2 — SHA-256 hex digest of a session token

```rust
use sha2::{Digest, Sha256};

let token_hash_hex = hex::encode(Sha256::digest(token.as_bytes()));
// store token_hash_hex in sessions.token_hash; compare on lookup
```

- `Sha256::digest(data)` is the one-shot API; the incremental API is
  `Sha256::new()` + `.update(…)` + `.finalize()`. Both return a 32-byte
  `GenericArray<u8, U32>`, which `hex::encode` turns into the 64-char
  lowercase hex string Node produces with `.digest('hex')`.
  Source: https://docs.rs/crate/sha2/latest (one-shot and incremental
  examples, including the known SHA-256 test vector for `"hello world"`).
- **Hash the token string, not the raw bytes.** Node stores the session
  token as a 64-char hex string and hashes that string
  (`createHash('sha256').update(token).digest('hex')`), i.e. the SHA-256
  input is the 64 UTF-8 bytes of the hex text, not the 32 raw random
  bytes. Hashing the raw 32 bytes in Rust would produce a different
  digest and every existing session would stop matching. This is the
  single most likely silent breakage in the session code — see §7.
- `sha2` 0.11 has hardware backends (x86 SHA-NI, aarch64 SHA2) selected
  automatically at runtime with a portable software fallback, so
  performance is a non-issue for a session lookup.
  Source: https://docs.rs/crate/sha2/latest ("By default the following
  backends are used: … `x86_64` … use `x86-sha` … when the required target
  features are detected at runtime; otherwise fall back to `soft`").

---

## 5. rand — random bytes, IDs, and room codes

### 5.1 API (rand 0.10)

- `rand::rng()` returns the thread-local generator (`ThreadRng`);
  it replaces `thread_rng()` (renamed in 0.9/0.10).
- The user-level methods live on the **`RngExt` trait** in 0.10
  (in 0.8/0.9 they were on `Rng`): `random()`, `random_range(..)`,
  `fill(&mut …)`. Import it: `use rand::RngExt;`.
  Source: https://docs.rs/rand/latest/rand/trait.RngExt.html
  ("`pub trait RngExt: Rng`", "This trait must usually be brought into
  scope via `use rand::RngExt;`", examples using `rand::rng()`).
- `rng.fill(&mut buf)` fills any `[u8]`-like buffer — the direct
  equivalent of `crypto.randomBytes(n)`.
  Source: https://docs.rs/rand/latest/rand/trait.RngExt.html
  (`fn fill`, example `rand::rng().fill(&mut arr[..])`).
- `rng.random_range(0..n)` samples uniformly from a range (rejection
  sampling — unbiased). The old names `gen` / `gen_range` were renamed
  to `random` / `random_range` in 0.9 to avoid clashing with the Rust 2024
  `gen` keyword.
  Source: https://docs.rs/rand/latest/rand/trait.RngExt.html
  (`random_range`, "Generate a random value in the given range").

### 5.2 What to use for secrets (tokens, salts, IDs)

`rand::rng()` is described by the crate as "an asymptotically-fast,
automatically-seeded and reasonably strong generator available on all
`std` targets", but the same page warns rand "is not … Primarily a
cryptographic library" and users must judge whether it meets their
security requirements. Source: https://docs.rs/crate/rand/latest.

For the security-sensitive values (session token, password salt) prefer
the OS source directly — `rand::rngs::SysRng`, "a `TryRng` interface over
the system's preferred random number source" (backed by `getrandom`),
enabled by the default `sys_rng` feature:

```rust
use getrandom::{SysRng, rand_core::TryRng}; // SysRng is re-exported via rand::rngs::SysRng
let mut bytes = [0u8; 32];
SysRng.try_fill_bytes(&mut bytes).unwrap();
```

Source: https://docs.rs/rand/latest/rand/rngs/struct.SysRng.html
("A `TryRng` interface over the system's preferred random number source",
usage example with `try_fill_bytes`). `TryRng` (fallible) is the honest
API here: OS entropy can fail, and `unwrap()`/proper error propagation
is better than `rand::rng()` silently reseeding. For non-secret values
(room codes, IDs) `rand::rng()` is fine.

### 5.3 IDs and room codes

```rust
// song_<16 hex> — 8 random bytes, hex-encoded, prefixed
let mut id_bytes = [0u8; 8];
rng.fill(&mut id_bytes);
let id = format!("song_{}", hex::encode(id_bytes));

// 6-char room code from 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'
const ALPHABET: &[u8; 32] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
let code: String = (0..6)
    .map(|_| ALPHABET[rng.random_range(0..32)] as char)
    .collect();
```

- **Modulo bias — not a problem here, but understand why.** Node does
  `randomBytes(6)[i] % 32` over a 32-char alphabet. Modulo bias occurs
  when the byte range (256) is not divisible by the alphabet size; here
  **256 % 32 == 0**, so every alphabet character is hit by exactly 8 byte
  values and `byte % 32` is exactly uniform. If the alphabet ever changes
  to a length that does not divide 256 (e.g. 31 or 36), the naive `%`
  becomes biased and the Rust port should use `random_range` (rejection
  sampling, unbiased by construction) instead of copying the `%`.
  `random_range` is the safer idiom regardless — it stays correct if the
  alphabet changes.
- Note the alphabet deliberately omits `I`, `L`, `O`, `0`, `1` (confusable
  characters) — copy it verbatim; do not "simplify" to `A-Z0-9`.
- `random_range` panics on an empty range; `0..32` on a 32-byte alphabet
  cannot be empty, but keep the bound derived from `ALPHABET.len()`
  rather than a literal `32`.

---

## 6. How the Node schema maps (checklist for the port)

`execute_batch` can carry the exact Node schema text unchanged
(`PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; CREATE TABLE IF
NOT EXISTS …`). The only per-connection statements that must be repeated
on **every** new connection are the PRAGMAs — `foreign_keys` especially
(see §7). Suggested Rust startup sequence:

```rust
let conn = Connection::open("karaokemuse.sqlite")?;
conn.execute_batch(
    "PRAGMA journal_mode = WAL;
     PRAGMA foreign_keys = ON;
     CREATE TABLE IF NOT EXISTS users (…);
     … -- the other 6 tables, verbatim from the Node schema
     ",
)?;
```

---

## 7. Semantic traps — where Rust behaviour differs from Node

1. **`Params::RECOMMENDED` ≠ Node defaults (§3.3).** The scrypt crate's
   recommended `log_n` is 17 (N = 131072), Node's is 14 (N = 16384), and
   the recommended output length is 32, not 64. Using the crate default
   silently invalidates every stored password hash. Hard-code
   `Params::new(14, 8, 1)` + a 64-byte buffer.
2. **SHA-256 input: hex string vs raw bytes (§4).** Node hashes the
   64-char hex token *text*. Hashing the 32 raw bytes gives a different
   digest and silently invalidates every live session.
3. **Salt encoding (§3.2).** The stored salt is hex text; scrypt needs
   the 16 decoded bytes. Passing the hex string as the salt also
   silently invalidates every password.
4. **Password bytes (§3.2).** Node does not normalize Unicode before
   scrypt; `password.as_bytes()` (UTF-8) matches, but adding
   `.normalize()` (NFC/NFKC) in Rust would change the derived key for
   passwords containing accented characters — common in Spanish
   passwords.
5. **`foreign_keys` is per-connection, not persisted.** In SQLite,
   `PRAGMA foreign_keys` defaults to OFF and must be set on every
   connection; only `journal_mode` persists in the file. Node's
   `node:sqlite` enables foreign keys **by default**, so the Node app
   never had to think about it — the Rust port must run
   `PRAGMA foreign_keys = ON` after every `Connection::open`, or FK
   violations will silently insert orphans. (The `PRAGMA foreign_keys`
   docs are the authority; the Node-side default is documented in the
   Node 24 `node:sqlite` docs at
   https://nodejs.org/docs/v24.21.0/api/sqlite.html.)
6. **WAL sidecars (§1).** The `.sqlite-wal` / `.sqlite-shm` files are
   part of the database. Never copy `karaokemuse.sqlite` alone while the
   server is running.
7. **SQLite type affinity is loose.** SQLite does not enforce column
   types (unless `STRICT` tables): storing an `i64` in a `REAL` column
   keeps it an INTEGER, and any integer can land in a TEXT column.
   Source: https://docs.rs/rusqlite/latest/rusqlite/types/index.html
   ("SQLite ignores column types, so if you store an `i64` in a column
   with type `REAL` it will be stored as an `INTEGER`, not a `REAL`
   (unless the column is part of a `STRICT` table)"). Practical effect:
   `row.get::<_, String>` on a column that accidentally holds an
   INTEGER fails with `InvalidColumnType` in Rust, where Node would
   return the number. The Node schema is not STRICT, so keep the
   existing column types and do not add STRICT in the port.
8. **`bool` is lenient on read, strict on write.** `FromSql for bool`
   treats any nonzero integer as `true` (§2.1). Node's JS code presumably
   does the same truthiness check, so behaviour matches — but a `CHECK
   (col IN (0,1))` is the only way to guarantee it.
9. **TEXT dates order lexicographically — in both engines.** ISO-8601
   (`2026-11-20T…Z`) stored as TEXT sorts correctly as strings in
   SQLite, identically under Node and rusqlite. Do **not** enable
   rusqlite's `chrono`/`time` features and re-store dates in another
   format; keep TEXT. (If those features were enabled, `time` stores
   RFC 3339 strings — compatible in practice — but there is no reason
   to add the dependency.)
10. **Numbers: JS `number` is f64; SQLite INTEGER is i64.** Counts and
    queue positions fit in `i64` easily. Never `row.get::<_, f64>` an
    INTEGER id and compare it to an integer — use `i64` throughout.
    `u64`/`usize` have no `FromSql`/`ToSql` impls (§2.1).
11. **`query_row` silently ignores extra rows.** Node's `stmt.get()` does
    the same, so this matches — but if the port ever needs "exactly one
    row" semantics (e.g. room lookup by code), use `query_one`, which
    errors `QueryReturnedMoreThanOneRow`. A duplicate room code would
    otherwise be silently masked.
12. **Errors are values, not exceptions.** Every rusqlite call returns
    `Result<_, rusqlite::Error>`; the `?` operator is the port's
    equivalent of Node's throws. Map `Error::SqliteFailure` with
    `ErrorCode::ConstraintViolation` (UNIQUE) to the same HTTP 409-style
    responses Node produced — do not string-match the error message,
    which differs between SQLite versions.
13. **Transactions roll back on drop.** In Node, an uncommitted
    transaction is rolled back when the connection closes; in rusqlite
    it rolls back when the `Transaction` value is dropped (§2). Both
    are safe, but the Rust version makes it easier to accidentally drop
    `tx` early (e.g. by returning from a closure) — keep `tx.commit()?`
    explicit and last.
14. **`Connection` is not `Sync` (§2.2).** Share it behind a `Mutex` or
    use one connection per thread. Node's single-threaded event loop
    never faced this; Axum handlers will.
15. **Bundled vs system SQLite differences.** With `bundled`, the binary
    always uses SQLite 3.54.1 regardless of the host OS — this is a
    feature for a Windows PC (no system SQLite needed), and it
    removes the class of bug where the Node side and the Rust side
    disagree on SQLite behaviour. The only cost is the C compiler at
    build time (§1). Without `bundled`, the Rust binary would link
    whatever SQLite the target machine has (possibly 3.46-era on older
    systems), which is still ≥ 3.45.3 and still reads the file — but
    `bundled` is the deterministic choice for a standalone binary.
16. **`timingSafeEqual` throws on length mismatch; `subtle::ct_eq`
    short-circuits (§3.4).** Both are safe when inputs are fixed at 64
    bytes, but validate the stored hash length (hex-decode to exactly 64
    bytes) before comparing; never compare hex *strings* with `==`.

---

## Recommended versions / features

```toml
[dependencies]
# Storage — rusqlite 0.43.0, MSRV 1.89.0, bundled SQLite 3.54.1 (static,
# no system libsqlite3 needed; needs a C compiler at build time).
# DB file is byte-compatible with node:sqlite (SQLite 3.53.4 in Node 24).
rusqlite = { version = "0.43", features = ["bundled"] }

# Passwords — scrypt 0.12.0, MSRV 1.85. Low-level fn only; DO NOT use
# Params::RECOMMENDED (log_n=17) or Scrypt::hash_password (PHC strings).
# Node compat: Params::new(14, 8, 1), 64-byte output, hex-decoded 16-byte salt.
scrypt = { version = "0.12", default-features = false }

# Sessions — sha2 0.11.1, MSRV 1.88. Hash the 64-char hex token STRING.
sha2 = "0.11"

# Randomness — rand 0.10.4, MSRV 1.85. rand::rng() + RngExt for room
# codes/IDs; SysRng (getrandom) for tokens and salts.
rand = "0.10"

# Hex encoding for salts, hashes, tokens, IDs.
hex = "0.4"

# Constant-time hash comparison (replaces crypto.timingSafeEqual).
subtle = "2.6"
```

**Toolchain floor: Rust ≥ 1.89** (set by rusqlite 0.43.0; all other crates
need ≤ 1.88).

**Pre-port verification checklist** (do before trusting the Rust binary
with the real `karaokemuse.sqlite`):

1. Generate a scrypt hash in Node (`scryptSync('test-password', salt, 64)`
   with defaults) and assert the Rust `scrypt()` output is byte-identical.
2. Generate a session token hash in Node and assert `Sha256::digest`
   of the hex string matches.
3. Open a copy of the real `karaokemuse.sqlite` with rusqlite `bundled`
   and run the full read path (all 7 tables) — then open the same copy
   again with Node to confirm no mutation.
4. Insert a row with the Rust binary (e.g. a test song) and read it back
   from Node, checking `is_favorite` round-trips as 0/1.

---

## Sources

- rusqlite crate docs (0.43.0): https://docs.rs/crate/rusqlite/latest
- rusqlite `Cargo.toml` (version, MSRV, features):
  https://docs.rs/crate/rusqlite/latest/source/Cargo.toml.orig
- rusqlite `Connection` API (`open`, `execute_batch`, `execute`,
  `query_row`, `query_one`, `prepare`, `transaction`, `pragma_update`,
  `busy_timeout`): https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html
- rusqlite types (`FromSql`/`ToSql`, INTEGER/REAL rules, `Option<T>`,
  `u64` exclusion): https://docs.rs/rusqlite/latest/rusqlite/types/index.html
- rusqlite `Error` enum: https://docs.rs/rusqlite/latest/rusqlite/enum.Error.html
- rusqlite `FromSql for bool` source:
  https://docs.rs/rusqlite/0.15.0/src/rusqlite/types/from_sql.rs.html
- scrypt crate docs (0.12.0): https://docs.rs/crate/scrypt/latest
- scrypt `Cargo.toml` (features): https://docs.rs/crate/scrypt/latest/source/Cargo.toml.orig
- scrypt low-level `scrypt()` fn + constant-time warning:
  https://docs.rs/crate/scrypt/latest/source/src/lib.rs
- scrypt `Params` (`RECOMMENDED_LOG_N = 17`, `new(log_n, r, p)`):
  https://docs.rs/crate/scrypt/latest/source/src/params.rs
- sha2 crate docs (0.11.1, one-shot/incremental API, backends):
  https://docs.rs/crate/sha2/latest
- sha2 `Cargo.toml` (MSRV 1.88, default features):
  https://docs.rs/crate/sha2/latest/source/Cargo.toml.orig
- rand crate docs (0.10.4, features, "not primarily a cryptographic
  library"): https://docs.rs/crate/rand/latest
- rand `Cargo.toml` (MSRV 1.85, default features):
  https://docs.rs/crate/rand/latest/source/Cargo.toml.orig
- rand `RngExt` trait (`random`, `random_range`, `fill`):
  https://docs.rs/rand/latest/rand/trait.RngExt.html
- rand `SysRng` (OS entropy source):
  https://docs.rs/rand/latest/rand/rngs/struct.SysRng.html
- hex crate docs (0.4.3): https://docs.rs/crate/hex/latest
- subtle crate docs (2.6.1, MSRV 1.41):
  https://docs.rs/crate/subtle/latest
- Node.js v24.21.0 `crypto` docs (`scryptSync` defaults N=16384/r=8/p=1,
  `timingSafeEqual`, no Unicode normalization):
  https://nodejs.org/docs/v24.21.0/api/crypto.html
- Node.js v24.21.0 `node:sqlite` docs:
  https://nodejs.org/docs/v24.21.0/api/sqlite.html
- Node 24 bundled SQLite version (3.53.4 in Node 24.18.0):
  https://sqldocs.org/sqlite-nodejs/
- SQLite file-format stability / backwards compatibility:
  https://www2.sqlite.org/matrix/formatchng.html
- SQLite database file format (header bytes 18–19 WAL, WAL sidecars):
  https://www.sqlite.org/fileformat2.html
