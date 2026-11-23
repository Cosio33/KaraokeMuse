# KaraokeMuse Rust port — JSON + outbound HTTP: serde, serde_json, reqwest

Research date: **2026-11-20**. Read-only research; no code was changed.
Sources: only docs.rs pages and the official serde / serde_json / reqwest GitHub
repos (plus the crates.io API for version metadata). Every Rust claim below
cites its source URL. JavaScript behaviour is stated from the original
KaraokeMuse code and ECMAScript/Node semantics as given in the task context,
not re-verified against a spec URL in this pass.

## 0. What the original does (ground truth, `server/index.js` lines 119–141)

```js
async function fetchMetadata(url) {
  let endpoint = null;
  if (/youtube\.com|youtu\.be/.test(url)) endpoint = `https://www.youtube.com/oembed?url=${encodeURIComponent(url)}&format=json`;
  else if (/spotify\.com|open\.spotify\.com/.test(url)) endpoint = `https://open.spotify.com/oembed?url=${encodeURIComponent(url)}`;
  else return { error: 'Plataforma no soportada para metadatos automáticos. Escribe título y artista manualmente.' };
  try {
    const r = await fetch(endpoint, { signal: AbortSignal.timeout(8000), headers: { 'user-agent': 'KaraokeMuse/1.0' } });
    if (!r.ok) return { error: 'No se pudieron leer los metadatos del enlace. Escribe título y artista manualmente.' };
    const j = await r.json();
    return {
      platform: /youtube/.test(url) ? 'youtube' : 'spotify',
      title: j.title || '', artist: j.author_name || '',
      thumbnail_url: j.thumbnail_url || '',
    };
  } catch {
    return { error: 'Sin red para leer metadatos. Escribe título y artista manualmente.' };
  }
}
```

Three user-visible outcomes, and one trap: **the `catch` covers both a
transport failure *and* a `r.json()` failure** — in the original, invalid JSON
and "no network" produce the *same* message (`Sin red para leer metadatos…`).
The Rust port must preserve that grouping (see §5.4). Request bodies are
parsed with `JSON.parse` (empty body → `{}`, invalid JSON → error
`JSON inválido`); responses are built with `JSON.stringify`.

---

## 1. Current stable versions and MSRV (verified 2026-11-20)

| Crate | Latest stable | Released | MSRV (`rust-version`) | Edition | License |
|---|---|---|---|---|---|
| `serde` | **1.0.229** | 2026-07-18 | **1.56** | 2021 | MIT OR Apache-2.0 |
| `serde_json` | **1.0.154** | 2026-10-11 | **1.71** | 2021 | MIT OR Apache-2.0 |
| `reqwest` | **0.13.5** | 2026-09-08 | **1.85.0** | 2021 | MIT OR Apache-2.0 |

- serde 1.0.229: `max_stable_version: 1.0.229`, `rust_version: "1.56"`.
  Source: <https://crates.io/api/v1/crates/serde>
- serde_json 1.0.154: `max_stable_version: 1.0.154`, `rust_version: "1.71"`.
  Source: <https://crates.io/api/v1/crates/serde_json>
- reqwest 0.13.5: `max_stable_version: 0.13.5`, `rust_version: "1.85.0"`.
  Source: <https://crates.io/api/v1/crates/reqwest>
- MSRV history that matters: reqwest 0.13.3 was still 1.64.0; **0.13.4 raised
  the MSRV to 1.85** ("chore: upgrade MSRV to 1.85", release notes).
  Source: <https://github.com/seanmonstar/reqwest/releases>
- The installed toolchain for this project is Rust 1.100.0, so all three MSRVs
  are comfortably satisfied; the effective floor for the port is reqwest's
  1.85.0. Pinning `rust-version = "1.85"` in the port's `Cargo.toml` documents
  that floor.

---

## 2. Exact Cargo features needed

### 2.1 serde — `derive` only

```toml
serde = { version = "1.0.229", features = ["derive"] }
```

- `serde`'s feature set is `default = ["std"]`, `derive = ["serde_derive"]`,
  plus `alloc`, `rc`, `unstable`.
  Source: <https://crates.io/api/v1/crates/serde>
- `derive` enables `#[derive(Serialize, Deserialize)]`; setup is
  `serde = { version = "1.0", features = ["derive"] }`.
  Source: <https://serde.rs/derive.html>
- Nothing else is needed: no `rc`, no `unstable`. `default-features = false`
  is *not* recommended — the port is a normal `std` binary.

### 2.2 serde_json — `preserve_order` conditional, `float_roundtrip` not needed

```toml
serde_json = { version = "1.0.154", features = ["preserve_order"] }  # only if Value/Map is used for output
```

Feature list (current `Cargo.toml.orig`):
`default = ["std"]`, `std`, `alloc`, `preserve_order = ["indexmap", "alloc",
"dep:foldhash"]`, `float_roundtrip = []`, `arbitrary_precision = []`,
`raw_value = []`, `unbounded_depth = []`.
Source: <https://docs.rs/crate/serde_json/latest/source/Cargo.toml.orig>

- **`preserve_order`** — needed **only** if the port builds responses as
  `serde_json::Value` / `Map` (untyped) or re-serializes a parsed body.
  Without it, `Map` is backed by a `BTreeMap` and keys are **sorted** (see
  §3.1). With it, `Map` uses `IndexMap` and keeps insertion order. If all
  API responses are typed structs, omit it — struct field order is fixed by
  the struct definition either way.
- **`float_roundtrip`** — NOT needed for KaraokeMuse. It makes
  `f64 -> JSON -> f64` produce byte-identical output, at "approximately 2x
  performance cost for parsing floats" (feature comment, same source). The
  port never round-trips client floats: ratings/ids are integers and the only
  floats that could appear are inside oEmbed bodies that are read, not
  re-emitted. Skip it.
- **`arbitrary_precision`** — NOT needed. It keeps `Number` as an arbitrary
  precision string so `JSON -> Number -> JSON` is byte-identical. Without it,
  integers outside `i64`/`u64` range are an error (see §4.3). KaraokeMuse
  numbers (ids, ratings, counts) are small.
- **`raw_value`, `unbounded_depth`** — NOT needed. `unbounded_depth` only
  matters for attacker-controlled nesting deeper than 128 levels (see §4.5);
  request bodies here are small and the 128 limit is a *feature*, not a bug.

### 2.3 reqwest — `json` + `query`; TLS = `rustls` (default)

```toml
reqwest = { version = "0.13.5", features = ["json", "query"] }
```

Default features in 0.13.5 are `["default-tls", "charset", "http2",
"system-proxy"]`, and `default-tls = ["rustls"]`.
The `rustls` feature itself is `["__rustls-aws-lc-rs",
"dep:rustls-platform-verifier", "__rustls"]`.
Source: <https://crates.io/api/v1/crates/reqwest> and
<https://docs.rs/reqwest/latest/reqwest/> (feature list).

**Breaking changes in reqwest 0.13.0 that the port must know about**
(from the release notes, <https://github.com/seanmonstar/reqwest/releases>):

- `rustls` is now the **default** TLS backend (was `native-tls`).
- The crypto provider defaults to **aws-lc-rs** (was `ring`).
- Feature `rustls-tls` was renamed to `rustls`.
- rustls roots features were removed — **`rustls-platform-verifier` is used
  by default**.
- **`query` and `form` are now optional features, disabled by default.** In
  0.12 `query` was always on; in 0.13 `.query()` does not compile without
  `features = ["query"]`. This is the easiest 0.13 migration foot-gun for
  the oEmbed call.

Feature-by-feature verdict for this port:

| Feature | Needed? | Why |
|---|---|---|
| `json` | **Yes** | `Response::json::<T>()` for the oEmbed body; also `RequestBuilder::json` if ever needed. "Provides serialization and deserialization for JSON bodies." — <https://docs.rs/reqwest/latest/reqwest/> |
| `query` | **Yes** | `RequestBuilder::query()` for `?url=…&format=json` (see §5.3). Optional since 0.13. |
| `rustls` (via `default-tls`) | **Yes, as the default** | See §2.4. |
| `native-tls` | No | Only if the port deliberately wants OS TLS instead of rustls. |
| `charset` | Default-on; keep | Needed only if `.text()` is used; harmless to keep. |
| `http2` | Default-on; keep | oEmbed endpoints speak HTTP/2; harmless. |
| `system-proxy` | Default-on; see §6 | Honours `HTTP(S)_PROXY` env vars — Node `fetch` does not. Keep unless proxy parity is required. |
| `blocking` | No | The port is async (Tokio); the blocking client would still need a runtime for TLS setup and adds API surface. |
| `cookies`, `gzip`, `multipart`, `stream`, `socks`, `hickory-dns` | No | None are used by the oEmbed call. |

### 2.4 TLS backend decision: rustls (default) vs native-tls

Backends, from <https://docs.rs/reqwest/latest/reqwest/tls/>:

- **`default-tls`**: "reqwest will pick a TLS backend by default. While it
  currently uses `rustls`…" — and "This feature is enabled by default, and
  takes precedence if any other crate enables it… You must set
  `default-features = false` instead" to turn it off. If the port wants to
  *force* rustls even if another dependency enables `native-tls`, call the
  builder method `tls_backend_rustls()` (same page).
- **`native-tls`**: "uses the `native-tls` crate. That will try to use the
  system TLS on Windows and Mac, and OpenSSL on Linux targets."
- **`rustls`**: "uses the `rustls` crate, a TLS library written in Rust."
- **`rustls-no-provider`**: rustls without a built-in crypto provider — for
  supplying your own, e.g. `ring` instead of the default `aws-lc-rs`; "You
  must install a crypto provider before building a `Client`. If none is
  installed the client will panic at construction time."

**Which works for a plain Linux binary and for Windows, and what it costs:**

- **rustls (recommended, the default).** Pure-Rust TLS plus a C crypto
  library; identical behaviour on Linux and Windows; the binary is
  self-contained (no OpenSSL to install on the target machine). Trust roots
  come from `rustls-platform-verifier` (see §2.5). Cost: a larger binary and
  a longer build, because `aws-lc-rs` compiles C code (AWS-LC) at build time
  — "Consuming projects will need a C/C++ compiler to build"; for non-FIPS
  builds "CMake is **never** required", "Bindgen is **never** required".
  On Windows x86-64, NASM is required for the assembly code, unless the
  crate-provided prebuilt NASM objects are used (feature `prebuilt-nasm` or
  env `AWS_LC_SYS_PREBUILT_NASM=1`).
  Source: <https://docs.rs/aws-lc-rs/latest/aws_lc_rs/>
  (No exact binary-size figure is published by any of these sources; measure
  the release binary with and without the TLS backend before quoting a
  number. Qualitatively: rustls+aws-lc-rs is the bigger of the two options.)
- **native-tls.** On **Windows** it links the OS SChannel DLL — nothing extra
  to ship, smallest binary, roots managed by Windows. On **Linux** it links
  the system OpenSSL: small binary, but the build needs OpenSSL development
  headers and the target machine needs a compatible `libssl` at runtime
  (and `native-tls-vendored` — which compiles a copy of OpenSSL from source —
  trades a much longer build and a bigger binary for self-containment).
  Source: <https://docs.rs/reqwest/latest/reqwest/tls/> and the older
  reqwest lib.rs note carried in the docs ("On Linux, it will use the
  available OpenSSL … or fail to build if not found. Alternatively you can
  enable the `native-tls-vendored` feature to compile a copy of OpenSSL").
- **Verdict for KaraokeMuse:** keep the default (`rustls` via
  `default-tls`). One static binary, same behaviour on the Linux VM and on
  a Windows PC, no OpenSSL to install. Revisit `native-tls` only
  if binary size or build time becomes a measured problem, or if the
  deployment needs Windows enterprise-CA handling that rustls cannot do
  (see §2.5).

### 2.5 Certificate verification — rustls-platform-verifier

With the default `rustls` feature, reqwest 0.13 uses
`rustls-platform-verifier` for root trust. From
<https://docs.rs/rustls-platform-verifier/latest/rustls_platform_verifier/>:

> "A Rust library to verify the validity of TLS certificates based on the
> operating system's certificate facilities. On operating systems that don't
> have these, `webpki` and/or `rustls-native-certs` is used instead."

| OS | Certificate store | Verification | Revocation |
|---|---|---|---|
| Windows | Windows platform certificate store | Windows API certificate verification | Yes |
| macOS (10.14+) | macOS platform roots and keychain | `Security.framework` | Yes |
| Linux | System CA bundle, or user-provided certs | webpki | No |
| WASM | webpki roots | webpki | No |

Practical consequences for the oEmbed call (YouTube/Spotify, public CAs):

- A corporate MITM proxy whose root is installed in the OS store is trusted
  on Windows (full platform verification) and on Linux (system bundle is
  read). This is close to Node's behaviour.
- On Linux the bundle is loaded once at startup and revocation (OCSP/CRL) is
  not checked; Node on Linux also does not do revocation checking by
  default, so this is parity in practice.
- Do **not** reach for `danger_accept_invalid_certs` / `danger_accept_invalid_hostnames`:
  they are not needed for public oEmbed endpoints and would silently accept
  MITM certificates.

### 2.6 Tokio (required, not optional)

- "The `reqwest::Client` is asynchronous (requiring Tokio)."
  Source: <https://docs.rs/reqwest/latest/reqwest/>
- Minimum features: `rt-multi-thread` (multi-threaded scheduler),
  `macros` (for `#[tokio::main]`), `time` (the built-in timer that
  `ClientBuilder::timeout` / `connect_timeout` rely on — the
  `connect_timeout` docs note it "requires the futures be executed in a
  tokio runtime with a tokio timer enabled").
  Source: <https://docs.rs/tokio/latest/tokio/> (feature list) and
  <https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html>.
- `tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }`
  (major line `1`, as documented on docs.rs; no patch version is asserted
  here).

---

## 3. serde_json output vs `JSON.stringify`

`serde_json::to_string` uses the compact formatter (no spaces, `{"a":1}`),
which matches `JSON.stringify` with no extra arguments.
Source: <https://docs.rs/serde_json/latest/serde_json/> (`to_string`,
`to_vec`, `to_writer`).

### 3.1 Key order

- **Typed structs: order is the field declaration order.** Serde's derive
  generates `serialize_field` calls in declaration order, and the documented
  example serializes `struct Point { x: i32, y: i32 }` as `{"x":1,"y":2}`.
  Source: <https://serde.rs/derive.html>. This matches `JSON.stringify` on
  a JS object literal, whose key order is also insertion (declaration)
  order. **Rule: declare struct fields in exactly the order the original
  object literal lists them.**
- **`serde_json::Value` / `Map`: by default keys are SORTED, not insertion
  ordered.** `Map` is backed by a `BTreeMap` unless `preserve_order` is
  enabled: "If serde_json's 'preserve_order' feature is not enabled, this
  method does no work because all JSON maps are always kept in a sorted
  state" (`Map::sort_all_objects` docs).
  Source: <https://docs.rs/serde_json/latest/serde_json/struct.Map.html>
  Without `preserve_order`, `json!({"b":1,"a":2}).to_string()` is
  `{"a":2,"b":1}` — different bytes from `JSON.stringify({b:1,a:2})`.
- **With `preserve_order`**, `Map` uses `IndexMap`: "Make
  `serde_json::Map` use a representation which maintains insertion order.
  This allows data to be read into a `Value` and written back to a JSON
  string while preserving the order of map keys in the input."
  Source: <https://docs.rs/crate/serde_json/latest/source/Cargo.toml.orig>
- **`HashMap<K, V>` serializes in arbitrary (hasher) order — never use it
  for response output.** Use a struct, a `BTreeMap` (only if sorted order is
  what the original produces), or `Map` with `preserve_order`.
- **Unknown fields in input are ignored by default** when deserializing
  into a struct ("When this attribute is not present, by default unknown
  fields are ignored for self-describing formats like JSON" —
  `#[serde(deny_unknown_fields)]` docs).
  Source: <https://serde.rs/container-attrs.html>. That matches the
  original, which only reads `title`/`author_name`/`thumbnail_url` off the
  oEmbed body and ignores the rest.

### 3.2 Numbers — integers vs floats

- **Integers are exact.** serde_json formats integers with `itoa`
  (dependency `itoa = "1.0"` in its `Cargo.toml.orig`). A Rust `i64`/`u64`
  prints the same digits as the JS number for any value inside the JS safe
  range. **Rule: model every integer JSON field (`id`, `rating`, counts,
  `minRating`) as a Rust integer type, never as `f64`.**
- **A Rust `f64` always prints with a decimal point or exponent.**
  serde_json's `Formatter::write_f64` calls
  `zmij::Buffer::format_finite(value)` (current `src/ser.rs`).
  Source: <https://github.com/serde-rs/json/blob/master/src/ser.rs>
  zmij's own tests assert `dtoa(1.0) == "1.0"` and
  `dtoa(43210.0) == "43210.0"`.
  Source: <https://github.com/dtolnay/zmij/blob/master/tests/test.rs>
  JS prints `JSON.stringify(5.0)` as `"5"`. So a rating stored as `f64`
  `5.0` would serialize as `5.0` in Rust vs `5` in Node — a byte difference
  in the API response. (This is exactly why §3.2's integer rule exists.)
- **Float formatting engine changed in serde_json 1.0.147.** Since 1.0.147
  (2025-12-23) serde_json depends on `zmij` (`zmij = "0.1.7"` in that
  version's manifest; `zmij = "1.0"` from 1.0.148 onward), replacing `ryu`.
  Source: <https://docs.rs/crate/serde_json/1.0.147/source/Cargo.toml.orig>
  and <https://docs.rs/crate/serde_json/latest/source/Cargo.toml.orig>
  zmij is "a double-to-string conversion algorithm based on Schubfach", a
  line-by-line port of Victor Zverovich's C++ implementation.
  Source: <https://docs.rs/zmij/latest/zmij/> and
  <https://github.com/dtolnay/zmij>
- **Exponent sign: zmij writes `e+`, ryu wrote `e`.** zmij tests assert
  `dtoa(3.439070283483335e+35) == "3.439070283483335e+35"`, and the
  boundary-sweep test compares zmij against ryu only after
  `.replace("e+", "e")` — i.e. zmij output is ryu output plus a `+` on
  positive exponents.
  Source: <https://github.com/dtolnay/zmij/blob/master/tests/test.rs>
  JS also writes `e+` (`JSON.stringify(1e21) === "1e+21"`), so the `+` is a
  *match*, not a difference — but only for serde_json ≥ 1.0.147. Pinning
  `serde_json >= 1.0.147` (we pin 1.0.154) is what makes this true.
- **Plain-vs-scientific thresholds differ from JS.** zmij's output is
  ryu-shaped, and ryu's `format64` prints plain decimal only when the
  decimal-point position `kk` satisfies `kk <= 16` (i.e. value < 1e16) on
  the large side and `-5 < kk <= 0` (i.e. value >= 1e-5) on the small side;
  otherwise it uses scientific notation.
  Source: <https://github.com/dtolnay/ryu/blob/master/src/pretty/mod.rs>
  JS `Number::toString` (used by `JSON.stringify`) prints plain decimal for
  `1e-6 <= |x| < 1e21`. Consequences:
  | Value | JS `JSON.stringify` | Rust `serde_json` (≥1.0.147) |
  |---|---|---|
  | `1e17` | `100000000000000000` | `1e+17` |
  | `1e20` | `100000000000000000000` | `1e+20` |
  | `1e21` | `1e+21` | `1e+21` (match) |
  | `1e-6` | `0.000001` | `1e-6` |
  | `1e-7` | `1e-7` | `1e-7` (match) |
  | `0.1` | `0.1` | `0.1` (match) |
  In practice KaraokeMuse serializes no floats at all (ratings/ids are
  integers; oEmbed floats are never re-emitted), so this is a latent trap,
  not a live one — but any future float field must be tested against this
  table.
- **`-0.0`**: zmij prints `"-0.0"` (test: `dtoa(-0.0) == "-0.0"`,
  same source as above); JS prints `"0"`. If a computation can produce
  negative zero, normalize it (`if v == 0.0 { 0.0 }`) or keep the field an
  integer.
- **Shortest round-trip**: both engines print the shortest decimal that
  round-trips to the same `f64`, so ordinary values (`0.1`, `4.5`, `19.99`)
  print identically in both languages.

### 3.3 String escaping — non-ASCII, control chars, lone surrogates

serde_json's escape table (`ESCAPE` in `src/ser.rs`) escapes exactly these
bytes and nothing else:

```rust
// 0x00..=0x1F -> \u00XX, except \x08 -> \b, \x09 -> \t, \x0A -> \n,
// \x0C -> \f, \x0D -> \r;  0x22 (") -> \",  0x5C (\) -> \\
// every other byte, including all bytes >= 0x80, is emitted raw.
```

Source: <https://github.com/serde-rs/json/blob/master/src/ser.rs>
(`ESCAPE` table and `write_char_escape`; control chars other than the five
shortcuts are written as `\u00` + lowercase hex).

- **Non-ASCII is emitted raw UTF-8** — `á é í ó ú ñ ¿ ¡` and emoji go out
  unescaped, exactly like `JSON.stringify`. No `\uXXXX` escaping of
  non-ASCII happens on either side. (The task's "no escaping of Spanish
  accents" is the default behaviour; nothing special is needed.)
- **`/` is NOT escaped** (the `CharEscape::Solidus` variant exists but the
  table maps `0x2F` to "no escape"), same as `JSON.stringify`.
- **Control characters** (`\n`, `\t`, `\r`, other `< 0x20`) are escaped with
  the same short forms JS uses, and `\u00XX` for the rest, with lowercase
  hex digits — JS also emits lowercase hex in `\u00XX`.
- **A Rust `&str`/`String` is always valid UTF-8 by construction**, so
  serializing can never emit a lone surrogate or invalid UTF-8. JS strings
  *can* contain lone surrogates, and `JSON.stringify` escapes them as
  `\ud800`; serde_json has no equivalent input (see §4.4 — it rejects them
  on parse instead).
- `to_string` "guarantees it only feeds valid UTF-8 sequences to the
  writer" (same source).

### 3.4 Making a Rust struct serialize byte-identically to the original's objects

Checklist, in order:

1. Use `#[derive(Serialize)]` structs with fields in the original's key
   order (§3.1).
2. Use Rust integer types for integer fields (§3.2); use `f64` only where
   the original truly has a float, and accept the §3.2 threshold table.
3. `Option<T>` serializes as `null` when `None` — which matches a JS field
   explicitly set to `null`, but **not** a JS field that is `undefined`
   (JS omits `undefined` fields). If the original builds an object whose
   key is sometimes absent, add `#[serde(skip_serializing_if =
   "Option::is_none")]` to that field.
4. `serde_json::to_string` (compact, no trailing newline) is the direct
   equivalent of `JSON.stringify(value)`; do **not** use `to_string_pretty`.
5. Booleans, `null`, arrays, nesting: identical in both.
6. If a response is ever built as `Value` instead of a struct, enable
   `preserve_order` (§2.2) or the keys come out sorted.

---

## 4. serde_json parsing vs `JSON.parse`

Entry point: `serde_json::from_str::<T>(&str)` (also `from_slice`,
`from_reader`).
Source: <https://docs.rs/serde_json/latest/serde_json/> and
<https://docs.rs/serde_json/latest/serde_json/fn.from_str.html>

### 4.1 Empty body

- `from_str("")` fails with `EofWhileParsingValue` ("EOF while parsing a
  value"). The original treats an empty body as `{}`. **Replicate that
  explicitly before parsing: `if body.trim().is_empty() { /* treat as {} */ }`.**
  Error message source:
  <https://github.com/serde-rs/json/blob/master/src/error.rs>
  (`ErrorCode::EofWhileParsingValue`).
- Whitespace-only bodies: `from_str("  ")` also fails the same way; the
  `trim().is_empty()` check covers both.

### 4.2 Duplicate keys

- **Into `Value`/`Map`: last wins.** `Map::insert`: "If the map did have
  this key present, the value is updated, and the old value is returned."
  Source: <https://docs.rs/serde_json/latest/serde_json/struct.Map.html>
  This matches `JSON.parse` (last duplicate wins in a JS object).
- **Into a typed struct: error.** The derive-generated visitor returns
  `Error::duplicate_field(name)` when a field appears twice.
  Source: <https://github.com/serde-rs/serde/blob/master/serde_derive/src/de/struct_.rs>
  ("`__Field::#name => { if Option::is_some(&#name) { return Err(
  <__A::Error as _serde::de::Error>::duplicate_field(#deser_name)); } …`").
  If the original must accept duplicate keys in a request body, parse into
  `Value` (or a `Map`) first, not directly into the struct.

### 4.3 Numbers out of range

- **Integer overflow → error.** An integer literal that does not fit in
  `i64`/`u64` produces `ErrorCode::NumberOutOfRange` ("Number is bigger
  than the maximum value of its type").
  Source: <https://github.com/serde-rs/json/blob/master/src/error.rs> and
  the `parse_integer` paths in
  <https://github.com/serde-rs/json/blob/master/src/de.rs>.
  JS `JSON.parse("99999999999999999999999")` silently rounds to the nearest
  `f64` instead of erroring.
- **Float overflow → error.** `f64_from_parts` returns
  `NumberOutOfRange` when the parsed value is infinite, in both the
  `float_roundtrip` and default paths; `parse_exponent_overflow` has the
  comment "Error instead of +/- infinity."
  Source: <https://github.com/serde-rs/json/blob/master/src/de.rs>
  JS `JSON.parse("1e400")` returns `Infinity` with no error. So a client
  sending `1e400` gets `JSON inválido` from the Rust port where Node
  accepted it — a deliberate strictness difference; the correct behaviour
  for this API is to reject it.
- **Underflow to zero is fine** (`1e-400` → `0.0`, no error, same as JS).

### 4.4 Trailing data, top-level values, lone surrogates, control chars

- **Trailing data → error.** After a value is parsed, `Deserializer::end`
  validates "that the input stream is at the end or that it only has
  trailing whitespace", returning `ErrorCode::TrailingCharacters` ("JSON has
  non-whitespace trailing characters after the value") otherwise.
  Source: <https://github.com/serde-rs/json/blob/master/src/de.rs> and
  <https://github.com/serde-rs/json/blob/master/src/error.rs>.
  `JSON.parse('{} {}')` also throws, so this matches.
- **Top-level values that are not objects.** `from_str` "can fail if the
  structure of the input does not match the structure expected by `T`, for
  example if `T` is a struct type but the input contains something other
  than a JSON map."
  Source: <https://docs.rs/serde_json/latest/serde_json/fn.from_str.html>
  `JSON.parse` accepts any top-level value (`"5"`, `"[1]"`, `"\"x\""`).
  The original presumably checks the parsed value is an object before
  using it; in Rust, deserializing directly into the request struct gives
  that check for free (a top-level `5` or `"x"` is a data error), or parse
  to `Value` and check `.is_object()` explicitly.
- **Lone surrogates → error.** `"\ud800"` (a high surrogate with no low
  half) is rejected with `ErrorCode::LoneLeadingSurrogateInHexEscape`
  ("Lone leading surrogate in hex escape").
  Source: <https://github.com/serde-rs/json/blob/master/src/error.rs>.
  `JSON.parse` accepts it and produces a JS string containing the lone
  surrogate. Rust `String` cannot hold one, so rejection is the only
  option — map it to `JSON inválido`.
- **Raw control characters inside strings → error**
  (`ControlCharacterWhileParsingString`), same as `JSON.parse`.
- **Numbers as map keys, trailing commas, single quotes, `NaN` /
  `Infinity` literals** are all rejected by both parsers (they are not
  valid JSON).

### 4.5 Nesting depth — recursion limit 128

- The deserializer starts with `remaining_depth: 128`, and nesting "more
  than 128 layers deep" fails with `ErrorCode::RecursionLimitExceeded`.
  Source: <https://github.com/serde-rs/json/blob/master/src/de.rs> and
  <https://github.com/serde-rs/json/blob/master/src/error.rs>.
- `JSON.parse` has no fixed depth limit (it is bounded by the V8 stack).
  A body nested 200 levels deep parses in Node and errors in Rust. For
  this API that is desirable (it is a stack-overflow guard for
  attacker-controlled input). The `unbounded_depth` feature can remove the
  limit, but its own docs warn to wrap the deserializer in `serde_stacker`
  and to be careful about `Display`/`Debug`/`Drop` overflowing the stack
  afterwards — do not enable it here.

### 4.6 Error classification (for mapping to `JSON inválido`)

`serde_json::Error` exposes `classify() -> Category` (`Io`, `Syntax`,
`Data`, `Eof`) plus `is_syntax()`, `is_data()`, `is_eof()`, `is_io()`,
`line()` and `column()`.
Source: <https://github.com/serde-rs/json/blob/master/src/error.rs>.
All four categories collapse to the original's single `JSON inválido`
response for request bodies; the category is useful only for logging
(`Syntax`/`Eof` = malformed JSON, `Data` = well-formed JSON of the wrong
shape, e.g. a top-level array where an object was expected).

---

## 5. reqwest — the oEmbed client

### 5.1 Client construction (build once, reuse)

```rust
use std::time::Duration;

let client = reqwest::Client::builder()
    .user_agent("KaraokeMuse/1.0")
    .timeout(Duration::from_secs(8))
    .build()?;
```

- "The `reqwest::Client` is asynchronous (requiring Tokio). … If you plan
  to perform multiple requests, it is best to create a `Client` and reuse
  it, taking advantage of keep-alive connection pooling."
  Source: <https://docs.rs/reqwest/latest/reqwest/>
- "The `Client` holds a connection pool internally … You do **not** have to
  wrap the `Client` in an `Rc` or `Arc` to **reuse** it, because it already
  uses an `Arc` internally."
  Source: <https://github.com/seanmonstar/reqwest/blob/master/src/async_impl/client.rs>
- Do **not** use the `reqwest::get()` shortcut for repeated calls: "This
  function creates a new internal `Client` on each call, and so should not
  be used if making many requests."
  Source: <https://github.com/seanmonstar/reqwest/blob/master/src/lib.rs>

### 5.2 Timeout — 8 s total, matching `AbortSignal.timeout(8000)`

- `ClientBuilder::timeout`: "Enables a total request timeout. The timeout
  is applied from when the request starts connecting until the response
  body has finished. Also considered a total deadline. Default is no
  timeout."
  Source: <https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html>
  That is the same scope as `AbortSignal.timeout(8000)`: connect + headers
  + body. `.timeout(Duration::from_secs(8))` on the builder (or
  `.timeout(...)` per-request on the `RequestBuilder`, which "overrides the
  timeout configured using `ClientBuilder::timeout()`") is the direct
  equivalent.
  Source for the per-request override:
  <https://docs.rs/reqwest/latest/reqwest/struct.RequestBuilder.html>
- `connect_timeout` is a separate, narrower knob (connect phase only) and
  "requires the futures be executed in a tokio runtime with a tokio timer
  enabled" — not needed here; the total timeout already covers connect.
- A timeout surfaces as `reqwest::Error` with `is_timeout() == true`.
  Source: <https://docs.rs/reqwest/latest/reqwest/struct.Error.html>

### 5.3 Custom User-Agent, GET, query params

- **No default User-Agent is sent.** Since reqwest 0.10: "Change to no
  longer send a default `User-Agent` header. Add one via
  `ClientBuilder::user_agent()`."
  Source: <https://github.com/seanmonstar/reqwest/blob/master/CHANGELOG.md>
  The original sets `'user-agent': 'KaraokeMuse/1.0'` explicitly, so the
  port must too: `.user_agent("KaraokeMuse/1.0")` on the builder. "Sets the
  `User-Agent` header to be used by this client."
  Source: <https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html>
  (The header value is also passed to the TLS connector for ALPN
  negotiation; that is internal — the wire header is what matters here.)
- **Query params** — the `query` feature is required (see §2.3). "Modify
  the query string of the URL. … This method appends and does not
  overwrite. … This requires the optional `query` feature to be enabled."
  Source: <https://docs.rs/reqwest/latest/reqwest/struct.RequestBuilder.html>

  ```rust
  let resp = client
      .get("https://www.youtube.com/oembed")
      .query(&[("url", video_url), ("format", "json")])
      .send()
      .await?;
  // Spotify: .get("https://open.spotify.com/oembed").query(&[("url", video_url)])
  ```

  Encoding note: `query()` serializes with `serde_urlencoded`
  (form-style: space → `+`, `+` → `%2B`). The original uses
  `encodeURIComponent` (space → `%20`). For a video URL the two agree on
  every character that normally appears (`:/?=&` are percent-encoded by
  both); they differ only for a literal space or `+` inside the URL, which
  is not a valid video URL in practice. If byte-exact query strings are
  ever required, build the URL string manually with `url` + `format!` +
  percent-encoding instead of `.query()`.
- **No request body, no `Content-Type`, no cookies** are needed for oEmbed
  (the original sends none).

### 5.4 Reading the JSON body and mapping errors to the original's messages

`Response::json::<T>()` (requires the `json` feature): "Try to deserialize
the response body as JSON. … This method fails whenever the response body
is not in JSON format, or it cannot be properly deserialized to target type
`T`."
Source: <https://docs.rs/reqwest/latest/reqwest/struct.Response.html>

Recommended shape for `fetch_metadata` (preserves the original's exact
messages and grouping):

```rust
// error strings copied verbatim from server/index.js fetchMetadata
const ERR_UNSUPPORTED: &str = "Plataforma no soportada para metadatos automáticos. Escribe título y artista manualmente.";
const ERR_NOT_OK: &str = "No se pudieron leer los metadatos del enlace. Escribe título y artista manualmente.";
const ERR_NO_NETWORK: &str = "Sin red para leer metadatos. Escribe título y artista manualmente.";

match client.get(endpoint).query(&params).send().await {
    Err(e) => {
        // transport / DNS / TLS / timeout / redirect-loop — log e.is_timeout(),
        // e.is_connect(), e.is_redirect() for diagnostics, but return ERR_NO_NETWORK
        Err(ERR_NO_NETWORK)
    }
    Ok(resp) if !resp.status().is_success() => Err(ERR_NOT_OK),
    Ok(resp) => match resp.json::<OEmbed>().await {
        // NOTE: invalid JSON lands here and MUST map to ERR_NO_NETWORK,
        // because in the original r.json() throwing is caught by the same
        // catch that handles network errors.
        Err(_) => Err(ERR_NO_NETWORK),
        Ok(meta) => Ok(meta),
    },
}
```

Why this shape, and what the docs say:

- **`send()` only fails on transport problems.** "This method fails if
  there was an error while sending request, redirect loop was detected or
  redirect limit was exhausted."
  Source: <https://docs.rs/reqwest/latest/reqwest/struct.RequestBuilder.html>
  A 404/500 from YouTube does **not** fail `send()` — unlike `fetch`,
  where `!r.ok` also does not throw, so the shapes line up: check the
  status explicitly, exactly as the original checks `!r.ok`.
- **Non-2xx → `ERR_NOT_OK`.** Either check `resp.status().is_success()`
  (as above) or call `resp.error_for_status()`: "Turn a response into an
  error if the server returned an error", producing an error with
  `is_status() == true` and `status() == Some(code)`.
  Source: <https://docs.rs/reqwest/latest/reqwest/struct.Response.html> and
  <https://docs.rs/reqwest/latest/reqwest/struct.Error.html>
  (`is_status`: "Returns true if the error is from
  `Response::error_for_status`"). Prefer the explicit status check: it
  keeps the response available and reads more like the original.
- **Invalid JSON → `ERR_NO_NETWORK` (same as transport errors).** The
  `json()` failure is a decode error: `Error::is_decode` "Returns true if
  the error is related to decoding the response's body".
  Source: <https://docs.rs/reqwest/latest/reqwest/struct.Error.html>
  Internally you may log `is_decode()` separately, but the user-facing
  string must be the same one as for network errors, or the port changes
  observable behaviour.
- **Error predicates available for logging/diagnostics** (all from the
  `Error` docs, same source): `is_timeout()` (timeout), `is_connect()`
  (connect phase), `is_dns()` (DNS resolution), `is_redirect()`
  (redirect policy), `is_status()` (from `error_for_status`), `is_decode()`
  (body decoding), `is_body()` (request/response body), `is_request()`,
  `status()` (the HTTP status, if the error came from a response). Note
  that `reqwest::Error` may include the full request URL — strip it before
  logging (the docs warn about API keys in query params; not an issue for
  oEmbed, but keep the habit).
- **oEmbed body shape.** Deserialize into a small struct with
  `#[serde(default)]` on every field (`title`, `author_name`,
  `thumbnail_url`), because the original reads them with `|| ''` fallbacks.
  Unknown fields are ignored by default (§3.1). If YouTube ever returns a
  non-object (it returns an object), `json::<OEmbed>()` fails → decode
  error → `ERR_NO_NETWORK`, same as the original's `r.json()` + property
  access would behave.
- **`.text()` vs `.json()` vs `.bytes()`.** `.text()` "decodes the response
  body with BOM sniffing and with malformed sequences replaced with the
  `char::REPLACEMENT_CHARACTER`" — i.e. it silently repairs invalid UTF-8.
  Source: <https://docs.rs/reqwest/latest/reqwest/struct.Response.html>
  Prefer `.json()` directly (it parses from bytes, strict UTF-8 via
  serde_json). If the body is ever needed as text first, use `.bytes()` +
  `serde_json::from_slice` to keep errors strict.

### 5.5 Redirects, proxies, HTTP version

- **Redirects: default max 10 hops.** "By default, a `Client` will
  automatically handle HTTP redirects, having a maximum redirect chain of
  10 hops. To customize this behavior, a `redirect::Policy` can be used
  with a `ClientBuilder`."
  Source: <https://docs.rs/reqwest/latest/reqwest/> and
  <https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html>
  ("Set a `RedirectPolicy` for this client. Default will follow redirects
  up to a maximum of 10."). Node `fetch` follows redirects too, with a
  higher cap (20 per the Fetch Standard). oEmbed endpoints do not redirect
  in practice; if one ever did, both follow it. A redirect loop surfaces
  as `is_redirect() == true` → maps to `ERR_NO_NETWORK` in the table
  above.
- **Proxies: system proxies are enabled by default.** "System proxies are
  enabled by default. System proxies look in environment variables…
  `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY`…" and the `system-proxy`
  feature "Use Windows and macOS system proxy settings automatically."
  Source: <https://docs.rs/reqwest/latest/reqwest/>
  Node `fetch` (undici) ignores proxy environment variables. In a proxied
  environment (corporate network, some CI runners) reqwest would route
  oEmbed through the proxy where Node would go direct. If that parity
  matters, call `.no_proxy()` on the builder; otherwise keep the default.
- **HTTP/2 by default** (`http2` is a default feature). Node `fetch` is
  HTTP/1.1-only. YouTube/Spotify support HTTP/2, so this is invisible in
  practice; `.http1_only()` exists if a server ever misbehaves over h2.

---

## 6. Semantic traps — Node vs Rust (checklist)

| # | Behaviour | Node (original) | Rust (this port) | Action |
|---|---|---|---|---|
| 1 | Float `5.0` | `JSON.stringify(5.0)` → `5` | `serde_json` → `5.0` | Use integer Rust types for integer fields (§3.2) |
| 2 | Large float `1e17`, `1e20` | plain decimal (`100000000000000000`) | scientific (`1e+17`, `1e+20`) — thresholds differ (§3.2) | Avoid floats in API output; test any future float field |
| 3 | Small float `1e-6` | `0.000001` | `1e-6` | Same as #2 |
| 4 | `-0.0` | `0` | `-0.0` | Normalize `0.0` or use integers |
| 5 | `1e21` | `1e+21` | `1e+21` | Match (serde_json ≥ 1.0.147, zmij) |
| 6 | `NaN` / `Infinity` on serialize | `null` | `null` (serializer writes null) | Match — but see #7 |
| 7 | `NaN` / `Infinity` via `Number::from_f64` | n/a (JS has no such API) | returns `None` | Don't build `Value::Number` from unchecked `f64`; check `is_finite()` first. `Number::from_f64`: "Converts a finite `f64` to a `Number`. Infinite or NaN values are not JSON numbers." — <https://docs.rs/serde_json/latest/serde_json/struct.Number.html> |
| 8 | `1e400` on parse | `Infinity`, no error | `NumberOutOfRange` error | Deliberate strictness; maps to `JSON inválido` |
| 9 | Integer > `u64::MAX` on parse | rounded `f64` | `NumberOutOfRange` error | Same as #8 |
| 10 | Duplicate keys → `Value` | last wins | last wins (`Map::insert`) | Match |
| 11 | Duplicate keys → struct | last wins | `duplicate_field` error | Parse to `Value` first if duplicates must be tolerated (§4.2) |
| 12 | Nesting > 128 deep | parses (stack-bound) | `RecursionLimitExceeded` error | Keep the limit; do not enable `unbounded_depth` (§4.5) |
| 13 | Lone surrogate `"\ud800"` | accepted | `LoneLeadingSurrogateInHexEscape` error | Maps to `JSON inválido` (§4.4) |
| 14 | Empty request body | `JSON.parse` throws; original treats as `{}` | `from_str("")` → `EofWhileParsingValue` | Special-case empty/whitespace body before parsing (§4.1) |
| 15 | Trailing data `{} {}` | throws | `TrailingCharacters` error | Match |
| 16 | `Option::None` field | omitted if `undefined`, `null` if `null` | always `null` | Add `skip_serializing_if = "Option::is_none"` where the original omits the key (§3.4) |
| 17 | Non-object top-level body | accepted by `JSON.parse` | struct target → data error | Rely on the struct error, or check `Value::is_object()` (§4.4) |
| 18 | `fetch` non-2xx | `!r.ok`, no throw | `send()` Ok; check status | Check `status().is_success()` explicitly (§5.4) |
| 19 | `fetch` invalid JSON body | `r.json()` throws → same `catch` as network | `resp.json()` → decode error | Map decode errors to the *same* `Sin red…` message as network errors (§5.4) |
| 20 | Default User-Agent | `fetch` sends one (`node`) unless overridden | reqwest sends **none** unless set | Always `.user_agent("KaraokeMuse/1.0")` (§5.3) |
| 21 | Redirect cap | 20 (Fetch Standard) | 10 | Irrelevant for oEmbed; note only |
| 22 | Proxy env vars | ignored by `fetch` | honoured by default (`system-proxy`) | `.no_proxy()` if parity is required (§5.5) |
| 23 | TLS trust roots | OS store (Node/OpenSSL) | `rustls-platform-verifier`: OS store on Windows/macOS, system bundle on Linux | Parity for public CAs; see §2.5 |
| 24 | Timeout scope | `AbortSignal.timeout(8000)` aborts whole fetch incl. body | `ClientBuilder::timeout` = total deadline incl. body read | Equivalent; use `.timeout(Duration::from_secs(8))` (§5.2) |
| 25 | HTTP version | HTTP/1.1 only | HTTP/2 by default | Invisible for oEmbed; `.http1_only()` exists |

---

## 7. Recommended versions / features

```toml
[dependencies]
# JSON: typed structs everywhere; preserve_order only if Value/Map is used for output.
serde = { version = "1.0.229", features = ["derive"] }
serde_json = { version = "1.0.154", features = ["preserve_order"] }
# HTTP: default TLS (rustls) + JSON bodies + query strings. No native-tls, no blocking.
reqwest = { version = "0.13.5", features = ["json", "query"] }
# Async runtime for the reqwest client (rt-multi-thread + macros for #[tokio::main],
# time for the timeout machinery).
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
```

Notes on the block:

- **Do not pin `serde_json` below 1.0.147.** That is the version that
  switched float formatting from `ryu` to `zmij` (and therefore from `1e21`
  to `1e+21`, matching JS). 1.0.154 is current; `1.0.154` also pulls
  `foldhash` under `preserve_order`, a change from 1.0.153 and earlier —
  no action needed, it is internal.
- **`float_roundtrip` and `arbitrary_precision`: omit.** Neither is needed
  for integer ratings/ids and string-only oEmbed reads (§2.2).
- **`reqwest` with default features kept** (do not use
  `default-features = false` unless proxy parity forces it — see §5.5).
  That keeps `default-tls` → `rustls`, `charset`, `http2`,
  `system-proxy`. If `native-tls` is ever chosen instead, it must be
  `default-features = false, features = ["native-tls", "json", "query",
  "charset", "http2"]` — and the Linux build then needs OpenSSL headers
  while Windows uses SChannel (§2.4).
- **MSRV floor: Rust 1.85.0** (from reqwest 0.13.5). The project's Rust
  1.100.0 toolchain satisfies it with headroom.
- **oEmbed client (recap):** one reused `Client` built with
  `.user_agent("KaraokeMuse/1.0")` and `.timeout(Duration::from_secs(8))`;
  `.query(&[("url", url), ("format", "json")])` for YouTube,
  `.query(&[("url", url)])` for Spotify; check `status().is_success()`
  before `.json::<OEmbed>()`; map transport errors *and* JSON decode
  errors to `Sin red para leer metadatos. Escribe título y artista
  manualmente.`, non-2xx to `No se pudieron leer los metadatos del enlace.
  Escribe título y artista manualmente.`, unsupported platforms to
  `Plataforma no soportada para metadatos automáticos. Escribe título y
  artista manualmente.` (all three strings verbatim from
  `server/index.js`).

## Sources (all primary: docs.rs or the official GitHub repos)

- Versions/MSRV/features: <https://crates.io/api/v1/crates/serde>,
  <https://crates.io/api/v1/crates/serde_json>,
  <https://crates.io/api/v1/crates/reqwest>
- serde_json manifest + feature comments:
  <https://docs.rs/crate/serde_json/latest/source/Cargo.toml.orig>,
  <https://docs.rs/crate/serde_json/1.0.147/source/Cargo.toml.orig>
- serde_json docs: <https://docs.rs/serde_json/latest/serde_json/>,
  <https://docs.rs/serde_json/latest/serde_json/fn.from_str.html>,
  <https://docs.rs/serde_json/latest/serde_json/struct.Map.html>,
  <https://docs.rs/serde_json/latest/serde_json/struct.Number.html>
- serde_json source: <https://github.com/serde-rs/json/blob/master/src/ser.rs>,
  <https://github.com/serde-rs/json/blob/master/src/de.rs>,
  <https://github.com/serde-rs/json/blob/master/src/error.rs>
- serde derive + attributes: <https://serde.rs/derive.html>,
  <https://serde.rs/container-attrs.html>,
  <https://github.com/serde-rs/serde/blob/master/serde_derive/src/de/struct_.rs>
- zmij: <https://docs.rs/zmij/latest/zmij/>,
  <https://github.com/dtolnay/zmij>,
  <https://github.com/dtolnay/zmij/blob/master/tests/test.rs>
- ryu pretty-printer thresholds:
  <https://github.com/dtolnay/ryu/blob/master/src/pretty/mod.rs>
- reqwest docs: <https://docs.rs/reqwest/latest/reqwest/>,
  <https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html>,
  <https://docs.rs/reqwest/latest/reqwest/struct.RequestBuilder.html>,
  <https://docs.rs/reqwest/latest/reqwest/struct.Response.html>,
  <https://docs.rs/reqwest/latest/reqwest/struct.Error.html>,
  <https://docs.rs/reqwest/latest/reqwest/tls/>
- reqwest source + releases: <https://github.com/seanmonstar/reqwest/blob/master/src/lib.rs>,
  <https://github.com/seanmonstar/reqwest/blob/master/src/async_impl/client.rs>,
  <https://github.com/seanmonstar/reqwest/blob/master/CHANGELOG.md>,
  <https://github.com/seanmonstar/reqwest/releases>
- TLS roots / crypto provider: <https://docs.rs/rustls-platform-verifier/latest/rustls_platform_verifier/>,
  <https://docs.rs/aws-lc-rs/latest/aws_lc_rs/>
- Tokio features: <https://docs.rs/tokio/latest/tokio/>
