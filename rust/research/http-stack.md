# KaraokeMuse Rust port — HTTP stack research: axum, tokio, tower-http

Research date: **2026-11-20**. Scope: the Rust HTTP stack only (axum + tokio + tower-http
on top of hyper). SQLite, scrypt, oEmbed fetching and the frontend are out of scope here.
All claims below are drawn from primary sources (docs.rs crate pages/source, the official
tokio/axum/tower-http GitHub repos, tokio.rs, nodejs.org). Where a claim could only be
confirmed from a secondary write-up, it is flagged as such and listed under
§11 as an item for the characterization test suite.

## 1. Current stable versions and MSRV

| Crate | Latest stable (verified 2026-11-20) | Released | MSRV (`rust-version` in its `Cargo.toml`) |
|---|---|---|---|
| axum | **0.8.9** | 2026-04-14 | **1.80** |
| tokio | **1.54.1** | 2026-10-21 | **1.85** |
| tower-http | **0.7.1** | by 2026-09-07 (see note) | **1.65** |
| hyper (transitive, via axum) | 1.x (axum 0.8.9 requires `^1.1.0`) | — | — |
| serde | 1.0.x | — | — |
| serde_json | 1.0.x (axum 0.8.9 requires `^1.0`, with the `raw_value` feature) | — | — |

Sources:

- axum 0.8.9, release date 2026-04-14: https://lib.rs/crates/axum
- axum 0.8.9 `rust-version = "1.80"`, `version = "0.8.9"`, MSRV statement
  ("axum's MSRV is 1.80"): https://docs.rs/crate/axum/latest/source/Cargo.toml and
  https://docs.rs/crate/axum/latest
- tokio 1.54.1, changelog heading "1.54.1 (October 21st, 2026)":
  https://docs.rs/crate/tokio/latest/source/CHANGELOG.md ; release table
  "1.54.1 | Oct 21, 2026": https://lib.rs/crates/tokio
- tokio 1.54.1 `rust-version = "1.85"`, `version = "1.54.1"`:
  https://docs.rs/crate/tokio/latest/source/Cargo.toml
- tokio MSRV policy and history ("1.54 to now — Rust 1.85", "1.48 to 1.53 — Rust 1.71",
  LTS `1.53.x` until September 2027): https://docs.rs/crate/tokio/latest
- tower-http 0.7.1, `rust-version = "1.65"`, `version = "0.7.1"`:
  https://docs.rs/crate/tower-http/latest/source/Cargo.toml
- tower-http 0.7.0 released 2026-06-15: https://lib.rs/crates/tower-http ; 0.7.1 release
  notes ("tower-http-0.7.1", `fs`: `ignore_multi_range_requests`, strong ETags):
  quoted in https://github.com/libp2p/rust-libp2p/commit/80077b1874461d8940f2b7c72055afdb6499f0e8
  (dependabot bump 0.7.0 → 0.7.1 merged 2026-09-07, so 0.7.1 was published between
  2026-06-15 and 2026-09-07 — no exact day was found on a primary source).
- axum 0.8.9 dependency on hyper `1.1.0`, tokio `1.44`, tower-http `0.6.8` (optional),
  serde_json `1.0`:
  https://docs.rs/crate/axum/latest/source/Cargo.toml

**Effective MSRV for the port is Rust 1.85** (the tokio requirement dominates).
The VM's rust-anything environment already has Rust 1.100.0, so this is not a constraint.

## 2. Cargo features needed

### 2.1 tokio

`tokio` has **no features enabled by default** (`default = []`).
The `full` feature enables everything (`fs`, `io-util`, `io-std`, `macros`, `net`,
`parking_lot`, `process`, `rt`, `rt-multi-thread`, `signal`, `sync`, `time`) and the
tokio docs recommend `full` when unsure, but the port only needs a subset:

- `macros` — `#[tokio::main]` / `#[tokio::test]`.
- `rt-multi-thread` — the multi-threaded work-stealing scheduler (implies `rt`).
- `net` — `tokio::net::TcpListener`, which `axum::serve` takes.
- `fs`, `io-util` — async file I/O; required by tower-http's `fs` feature
  (see §2.3) and by any hand-rolled file streaming.
- `time` — pulled in by axum itself (`tokio` with `features = ["time"]` in axum's
  own `Cargo.toml`); harmless to list explicitly.
- `sync` — only if the port uses `tokio::sync` primitives (e.g. `Mutex`/`RwLock`
  for shared state). Include it; it is cheap.
- NOT needed: `process`, `signal`, `parking_lot`, `io-std`, `test-util`.

Source: https://docs.rs/crate/tokio/latest/source/Cargo.toml
(`full = [...]`, `rt-multi-thread = ["rt"]`, `macros = ["tokio-macros"]`) and
https://docs.rs/tokio/latest/tokio/ ("the easiest way to get started is to enable
all features… `full`"; "`rt-multi-thread`: Enables the heavier, multi-threaded,
work-stealing scheduler").

### 2.2 axum

axum's **default features** already cover most of what the port needs:
`default = ["form", "http1", "json", "matched-path", "original-uri", "query",
"tokio", "tower-log", "tracing"]`.

- `json` (default) — enables `axum::Json` (extractor + responder); pulls in
  `serde_json` and `serde_path_to_error`.
  Source: https://docs.rs/crate/axum/latest/source/Cargo.toml
  (`json = ["dep:serde_json", "dep:serde_path_to_error"]`).
- `http1` (default) — hyper HTTP/1.1 server support. Keep it. (`http2` is a
  separate opt-in feature; the Node original is HTTP/1.1 only, so do **not**
  enable `http2`.)
- `tokio` (default) — enables `axum::serve` and pulls `tokio/net`, `tokio/rt`,
  `tokio/macros`.
- `macros` (NOT default) — only provides `#[debug_handler]`; not required for
  the port, but harmless and useful while developing. Optional.
- `query` / `form` / `matched-path` / `original-uri` (default) — `query` is
  needed for query-string extraction (`?random`, filters); the rest are
  harmless.
- `multipart`, `ws`, `tracing` — not needed for KaraokeMuse.

Note: axum 0.8.9 itself depends on `tower-http 0.6.8` as an **optional**
dependency (used only by its `__private_docs` feature), so a direct dependency
on tower-http 0.7.1 does not conflict in practice.
Source: https://docs.rs/crate/axum/latest/source/Cargo.toml

### 2.3 tower-http

`tower-http` also has **no default features** (`default = []`). The port needs:

- `fs` — `ServeDir` / `ServeFile`. It pulls `tokio` (`fs`, `io-util`),
  `http-range-header` 0.4.2, `mime_guess`, `mime`, `httpdate`, `set-status`.
  Source: https://docs.rs/crate/tower-http/latest/source/Cargo.toml
  (`fs = ["dep:tokio", "tokio?/fs", "tokio?/io-util", …, "dep:http-range-header",
  "mime_guess", …]`).
- `limit` — only if the port uses `tower_http::limit::RequestBodyLimitLayer`
  instead of axum's `DefaultBodyLimit` (not recommended; see §8).
- `normalize-path` — only if the port wants trailing-slash normalization
  (see §10).

### 2.4 serde / serde_json (direct dependencies)

axum's `json` feature enables `serde_json` for axum's own code, but the port's
own DTO structs need their own direct dependencies:

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

`serde` with `derive` is **not** pulled in by axum (axum depends on
`serde_core`, and on `serde` only optionally). Without the direct `serde`
dependency, `#[derive(Serialize, Deserialize)]` will not resolve.

## 3. Routing with path params

```rust
use axum::{Router, routing::{get, post, patch, delete}, extract::{Path, State}};

let app = Router::new()
    .route("/api/songs/{id}", patch(patch_song))
    .route("/api/songs/{id}", delete(delete_song));

async fn patch_song(Path(id): Path<String>, State(state): State<AppState>, Json(body): Json<PatchSong>) -> … {
    // …
}
```

Key points:

- **axum 0.8 changed the capture syntax.** Captures are now `{name}`
  (`/api/songs/{id}`) and wildcards are `{*rest}`. The old 0.7 syntax `:id`
  is now **literal text**, and axum 0.8 **panics at router construction** for
  any segment starting with `:` or `*` (the "v07 checks"), because it is
  almost certainly a stale 0.7-style capture. The task brief's
  `PATCH /api/songs/:id` must therefore be written as `/api/songs/{id}`.
  Source: https://www.irurueta.com/backend/axum/routing.html
  ("axum 0.8 changed the path syntax from `/:id` and `/*rest` to `/{id}` and
  `/{*rest}`"; "axum 0.8 refuses paths whose segments start with `:` or `*`…
  `Path segments must not start with ':'. For capture groups, use '{capture}'").
- A capture matches **exactly one segment**: `/api/songs/{id}` matches
  `/api/songs/42` but not `/api/songs/42/extra` and not `/api/songs/`.
  A wildcard `{*path}` matches one or more remaining segments.
  Source: https://www.irurueta.com/backend/axum/routing.html
- **Captures are percent-decoded before extraction.** `Path` "will get captures
  from the URL and parse them using `serde`. Any percent encoded parameters
  will be automatically decoded. The decoded parameters must be valid UTF-8,
  otherwise `Path` will fail and return a `400 Bad Request` response."
  E.g. `/search/caf%C3%A9` yields `"café"`.
  Source: https://docs.rs/axum/latest/axum/extract/path/struct.Path.html
- `Path<T>` deserializes with serde: `Path<u32>`, `Path<(String, u32)>`, or a
  `#[derive(Deserialize)]` struct whose field names match the capture names.
  A capture that cannot be parsed into the target type (e.g. `abc` into `u32`)
  is rejected with **400 Bad Request** and a plain-text message, before the
  handler runs.
  Source: https://docs.rs/axum/latest/axum/extract/path/struct.Path.html
- Method routers: `get`, `post`, `put`, `patch`, `delete`, `head`, `options`,
  `trace`, `any`, `on(...)` from `axum::routing`. Multiple methods on one path
  chain: `get(list).post(create)`.
  Source: https://www.irurueta.com/backend/axum/routing.html
- **Route order does not matter** — axum compiles routes into a matchit
  radix tree; static segments beat captures, captures beat wildcards.
  Source: https://www.tutorialscenter.com/axum/routing-basics
  (secondary; consistent with matchit being axum's router per
  https://www.irurueta.com/backend/axum/routing.html).
- Serving: `axum::serve(listener, app)` where `listener` is a
  `tokio::net::TcpListener`; the entry point is `#[tokio::main]`.
  Source: https://docs.rs/axum/latest (crate docs) and
  https://www.irurueta.com/backend/axum/getting-started.html.

## 4. JSON request/response

### 4.1 `Json<T>` as extractor (request)

- `Json<T>` checks the request `Content-Type` is `application/json` (or
  `application/*+json`), buffers the whole body (subject to the size limit,
  §8), and deserializes with `serde_json::from_slice`.
- It must be the **last** handler argument — it consumes the body, and at most
  one body-consuming extractor is allowed per handler.
  Source: https://docs.rs/axum/latest/axum/struct.Json.html
  ("Since parsing JSON requires consuming the request body, the `Json`
  extractor must be *last* if there are multiple extractors in a handler.")
- Rejection table (default behavior — plain-text bodies; see §4.3 for the
  status codes and §10 for the shape trap):

  | Situation | Status | axum rejection variant |
  |---|---|---|
  | Missing or wrong `Content-Type` | **415** Unsupported Media Type | `MissingJsonContentType` |
  | Body is not valid JSON (incl. trailing characters after the document, since 0.8.5) | **400** Bad Request | `JsonSyntaxError` |
  | Valid JSON, wrong shape (missing field, wrong type) | **422** Unprocessable Entity | `JsonDataError` |
  | Body exceeds the size limit | **413** Payload Too Large | `BytesRejection` |
  | Body bytes cannot be read | 500 | `BytesRejection` |

  Sources: https://www.irurueta.com/backend/axum/extractors.html
  (rejection table "verified against axum 0.8.9": 415 / 400 / 422 with the exact
  default messages `Expected request with 'Content-Type: application/json'`,
  `Failed to parse the request body as JSON: …`,
  `Failed to deserialize the JSON body into the target type: …`);
  https://jsonic.io/guides/json-in-axum (same 415/422/400 mapping, and
  `JsonRejection` variants `MissingJsonContentType`, `JsonDataError`,
  `JsonSyntaxError`, `BytesRejection`);
  https://www.tutorialscenter.com/axum/json-and-form-bodies (413 for
  over-limit bodies). The 413 mapping is from secondary sources only —
  verify in the characterization suite (§11).
- Error messages name the failing field (via `serde_path_to_error`), e.g.
  `Failed to deserialize the JSON body into the target type: missing field
  'email' at line 1 column 15`.
  Source: https://www.irurueta.com/backend/axum/extractors.html

### 4.2 `Json<T>` as responder (response)

- `Json<T>` serializes any `T: Serialize` and "will automatically set
  `Content-Type: application/json` header".
  Source: https://docs.rs/axum/latest/axum/struct.Json.html
- **It sets bare `application/json` — no `; charset=utf-8` suffix.** The Node
  original sets `Content-Type: application/json; charset=utf-8`. If the port
  must reproduce the header byte-for-byte, `Json<T>` alone is not enough;
  return `( [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
  Json(body) )` or build the response by hand. See §10.
- Returning `Json<T>` alone always sends **200 OK**. To send any other status,
  return a tuple: `(StatusCode::CREATED, Json(value))`. `IntoResponse` is
  implemented for `(StatusCode, R)` tuples.
  Source: https://docs.rs/axum/latest/axum/response/index.html
  ("A tuple of `StatusCode` and something that implements `IntoResponse` can
  be used to override the status code"; "`Json` gives a content-type of
  `application/json`").
- If serialization itself fails (practically impossible for ordinary DTOs —
  e.g. a non-finite float in a struct), axum returns **500** with a
  `text/plain; charset=utf-8` body containing the error.
  Source: axum 0.7.9 `IntoResponse for Json` source, same logic in 0.8:
  https://docs.rs/crate/axum/0.7.9/source/src/json.rs

### 4.3 Exact status codes

Any status is expressible: `StatusCode::OK`, `CREATED` (201), `NO_CONTENT`
(204), `BAD_REQUEST` (400), `NOT_FOUND` (404), `METHOD_NOT_ALLOWED` (405),
`RANGE_NOT_SATISFIABLE` (416), etc., from the `http` crate (re-exported as
`axum::http::StatusCode`). For a 204 with no body, return `(StatusCode::NO_CONTENT, ())`.

## 5. Static files: `ServeDir` / `ServeFile`

```rust
use tower_http::services::ServeDir;

let app = Router::new()
    .nest_service("/static", ServeDir::new("public"))
    // or, to serve public/ at the root with an API router alongside:
    .fallback_service(ServeDir::new("public"));
```

- `ServeDir` is a tower `Service`, not a handler: mount with `nest_service`
  or `fallback_service`. It is only available with the **`fs` feature**.
  Source: https://docs.rs/tower-http/latest/tower_http/services/struct.ServeDir.html
  ("Available on **crate feature `fs`** only"; "Service that serves files from
  a given directory and all its sub directories. The `Content-Type` will be
  guessed from the file extension.")
- **Traversal guard is built in.** An empty **404** response is returned if:
  "The file doesn't exist; Any segment of the path contains `..`; Any segment
  of the path contains a backslash; On unix, any segment of the path referenced
  as directory is actually an existing file (`/file.html/something`); We don't
  have necessary permissions to read the file."
  Source: https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_dir/mod.rs.html
  (lines 77–94 of the `ServeDir` docs). The Node original's hand-rolled guard
  can therefore be dropped — but note the failure mode is a bare 404 with an
  empty body, so if the original returns a JSON 404 for traversal attempts,
  the port must wrap `ServeDir` or handle it at the router level.
- `ServeFile::new(path)` serves one fixed file (it is implemented as a
  special case of `ServeDir`); `ServeFile::new_with_mime(path, mime)` pins the
  content type instead of guessing it.
  Source: https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_file.rs.html
- Defaults that differ from a plain file server:
  - `append_index_html_on_directories` defaults to **true**: a request for a
    directory serves `index.html` inside it.
  - `redirect_to_trailing_slash` defaults to **true**: a request for `/dir`
    is answered with a **307 Temporary Redirect** to `/dir/`
    (see `OpenFileOutput::Redirect` → `StatusCode::TEMPORARY_REDIRECT` in
    the source below).
  - Read buffer chunk size defaults to **64 KiB**.
  Source: https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_dir/mod.rs.html
  and https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_dir/future.rs.html
  (lines 208–216).
- Non-GET/HEAD methods get **405 Method Not Allowed** with
  `Allow: GET,HEAD` (or the configured fallback is called, if
  `call_fallback_on_method_not_allowed` is set).
  Source: https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_dir/future.rs.html
  (lines 350–360).
- Missing file with no fallback configured: empty **404**. With
  `.fallback(...)` / `.not_found_service(...)` the fallback service is called
  instead (e.g. an SPA `index.html`); `ServeDir::not_found_service` forces the
  fallback's status to 404.
  Source: https://docs.rs/tower-http/latest/tower_http/services/struct.ServeDir.html
- Since 0.7.0, `ServeDir` also handles conditional requests: `If-None-Match` /
  `If-Modified-Since` → **304 Not Modified** (with `ETag` / `Last-Modified`),
  `If-Match` / `If-Unmodified-Since` failures → **412 Precondition Failed**,
  and it emits **strong ETags**.
  Source: https://www.irurueta.com/backend/axum/tower-http-catalog.html
  ("new in 0.7 — strong `ETag`s with `If-Match`/`If-None-Match` handling")
  and https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_dir/future.rs.html
  (lines 236–280, `PreconditionFailed`, `NotModified` arms). These are
  extra behaviors the Node original does not have; they are harmless for
  browsers but add headers the characterization suite should expect.
- **Content-Type guessing** uses `mime_guess` (mime-db based). The mappings
  the port cares about match the Node original exactly:
  `.mp4` → `video/mp4`, `.webm` → `video/webm`, `.mkv` → `video/x-matroska`,
  `.html` → `text/html`, `.css` → `text/css`, `.js` → `text/javascript`.
  Sources: https://docs.rs/crate/tower-http/latest/source/Cargo.toml
  (`fs` feature includes `mime_guess`); mapping table:
  https://flaviocopes.com/tools/mime-types/ and
  https://useotools.com/audio-video-mime-types (reference tables; the
  authoritative mapping is mime-db, which `mime_guess` embeds).

## 6. HTTP Range support — built in, no hand parsing needed

`ServeDir` / `ServeFile` support single-range requests natively, via the
`http-range-header` crate (0.4.2 in tower-http 0.7.1). **Do not hand-parse the
`Range` header** for the standard video-streaming case.

What the service sends (verified from the 0.7.1 source):

- Every file response carries `Accept-Ranges: bytes`, `Content-Type` (guessed),
  `Last-Modified`, and `ETag`.
- Valid single range (`Range: bytes=0-1023`, `bytes=500-`, `bytes=-500`):
  **206 Partial Content** with `Content-Range: bytes <start>-<end>/<size>`,
  `Content-Length` = range length (set explicitly), and a body streamed from
  the file in 64 KiB chunks.
  Source: https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_dir/future.rs.html
  (lines 458–574: `build_response`, `Some(Ok(range))` arm,
  `StatusCode::PARTIAL_CONTENT`, `Content-Range: bytes {}-{}/{}`).
- Full (non-range) request: **200** with `Content-Length` = file size set
  explicitly (so the body is **not** chunked).
  Source: same file, lines 626–654 (`None` arm, `.header(header::CONTENT_LENGTH, size)`).
- Unsatisfiable single range: **416 Range Not Satisfiable** with
  `Content-Range: bytes */<size>` and an empty body; `Content-Type` and
  `Content-Encoding` headers are stripped from the 416 response.
  Source: same file, lines 604–621.
- **Multi-range requests are NOT supported**: `Range: bytes=0-0,2-2` →
  **416** with body `Cannot serve multipart range requests` and
  `Content-Range: bytes */<size>`.
  Source: same file, lines 586–600; also
  https://github.com/tower-rs/tower-http/issues/660
  ("`ServeDir` / `ServeFile` do not support multipart range responses").
- Opt-out: `.ignore_multi_range_requests(true)` serves a multi-range request
  as a normal **200** full response, as if the `Range` header were absent.
  Malformed range headers and unsatisfiable single ranges still return 416.
  Source: https://docs.rs/tower-http/latest/tower_http/services/struct.ServeDir.html
  (`ignore_multi_range_requests` docs).
- HEAD requests return the same headers with an empty body.
  Source: same `future.rs` (`FileRequestExtent::Head` arm, lines 460–466).

Comparison with the Node original: Node's `fs.createReadStream({start, end})`
with a hand-set `Content-Range` produces the same wire result for single
ranges. The one behavioral difference to check: if the original serves a
multi-range request as multipart/byteranges or as a plain 200, the Rust port
must set `ignore_multi_range_requests(true)` or hand-roll that endpoint —
the default tower-http 416 will differ.

## 7. Request body size limits

- axum's default limit is **2 MiB** for `Bytes` and every extractor built on
  it (`String`, `Json`, `Form`): "For security reasons, `Bytes` will, by
  default, not accept bodies larger than 2MB. This also applies to extractors
  that uses `Bytes` internally such as `String`, `Json`, and `Form`."
  Source: https://docs.rs/axum/latest/axum/extract/struct.DefaultBodyLimit.html
- The Node original rejects bodies **> 1 MiB**, so the port must lower the
  limit explicitly — the axum default (2 MiB) would silently accept bodies
  the original rejects:

  ```rust
  use axum::extract::DefaultBodyLimit;

  let app = Router::new()
      .route(…)
      .layer(DefaultBodyLimit::max(1024 * 1024));
  ```

  Source: same docs.rs page (`DefaultBodyLimit::max(limit)`; "Replace the
  default of 2MB with 1024 bytes" example).
- `DefaultBodyLimit` is a **layer** that only applies to extractors that
  honor it (the `Bytes`-based ones). If a handler reads the body directly
  (`Body::data`, `Body::poll_frame`), the limit is **not** applied.
  Source: same page ("Note that if an extractor consumes the body directly
  with `Body::data`, or similar, the default limit is *not* applied").
- The global alternative is `tower_http::limit::RequestBodyLimitLayer`, which
  applies to all requests regardless of extractor; axum's docs recommend
  `DefaultBodyLimit` in general and `RequestBodyLimit` only when third-party
  extractors must also be limited. For this port `DefaultBodyLimit` is the
  right tool.
  Source: same page ("Difference between `DefaultBodyLimit` and
  `RequestBodyLimit`").
- Over-limit `Json` requests are rejected with **413** (see §4.1 table;
  secondary sources only — verify, §11).

## 8. Invalid JSON bodies

Covered by the rejection table in §4.1. The important subtleties:

- A request with no `Content-Type`, or `text/plain`, is rejected with **415**
  before the body is even read. The Node original presumably parses the body
  regardless of `Content-Type` (Node's `http` does not check it); if the
  original accepts JSON with a missing/wrong content type, `Json<T>` will
  diverge — use `Bytes` + `serde_json::from_slice` by hand for those routes
  instead.
- Since axum 0.8.5, **trailing characters after the JSON document** are also
  a 400 (`JsonSyntaxError`); earlier versions silently accepted trailing
  whitespace only. `serde_json::from_slice` is strict about trailing data.
  Source: https://www.irurueta.com/backend/axum/extractors.html
  ("not valid JSON, or trailing characters after the document (since 0.8.5)").
- A body that is valid JSON but the wrong shape is a **422**, not a 400.
  Node code that returns 400 for both cases will diverge; the port must
  either accept axum's 400/422 split (recommended — it is more correct) or
  map rejections to a uniform status with a custom extractor.

## 9. Exact JSON output format

`Json<T>` serializes with `serde_json::to_writer` in **compact** form: no
spaces, no trailing newline, `{"key":value}`.
Source: axum 0.7.9 source (same code in 0.8),
https://docs.rs/crate/axum/0.7.9/source/src/json.rs
(`serde_json::to_writer(&mut buf, &self.0)`).

### 9.1 Field order

- **Structs serialize in declaration order.** serde derives emit fields in the
  order they are declared in the Rust struct, so a DTO declared in the same
  order as the Node object literal produces byte-identical key order.
- **`serde_json::Value` / `Map` do NOT preserve insertion order by default.**
  Without the `preserve_order` feature, `Map` is backed by a `BTreeMap` and
  keys are emitted **sorted alphabetically** ("If serde_json's
  `preserve_order` feature is not enabled, this method does no work because
  all JSON maps are always kept in a sorted state").
  With `preserve_order`, insertion order is kept (IndexMap-backed).
  Source: https://docs.rs/serde_json/latest/serde_json/struct.Map.html
- **Trap vs `JSON.stringify`:** JavaScript objects preserve insertion order
  for string keys (integer-like keys come first, numerically ascending).
  So if the port builds responses as `serde_json::json!({...})` / `Value`
  without `preserve_order`, key order will be alphabetical, not the
  original's insertion order. The fix is to either use typed structs
  (declaration order) or enable `preserve_order` on `serde_json`
  (`serde_json = { version = "1", features = ["preserve_order"] }`).
  Recommendation: typed structs everywhere; `Value` only for pass-through
  payloads, with `preserve_order` enabled as a safety net.

### 9.2 Non-ASCII escaping

- serde_json emits non-ASCII characters as **raw UTF-8, never `\u`-escaped**.
  Only these are escaped: `"` → `\"`, `\` → `\\`, the named controls
  `\b \t \n \f \r`, and other control characters `U+0000..U+001F` as
  `\u00XX` with **lowercase** hex. `/` is not escaped, `U+007F` DEL is not
  escaped.
  Source: https://www.rcxprotocol.org/spec/v1/03-canonical-json
  (specification of serde_json 1.0.139's exact escaping, pinned to
  `serde_json::to_string`).
- This **matches `JSON.stringify`** for all practical purposes: JS also emits
  raw UTF-8 and escapes only `"`, `\` and controls. Two edge differences:
  (a) `JSON.stringify` escapes lone surrogates as `\udXXX`; serde_json
  **fails** on lone surrogates when parsing (a `JsonSyntaxError`/400) and can
  never produce them from a Rust `String` (Rust strings are valid UTF-8 by
  construction); (b) no Unicode normalization is applied on either side.
  Source: same rcxprotocol page ("No Unicode normalization is applied");
  serde_json Deserializer docs on unpaired surrogates:
  https://docs.rs/serde_json/latest/serde_json/struct.Deserializer.html
  (serde_json "is specified to fail on non-UTF-8 strings when deserializing
  into Rust UTF-8 string types").
- **Numbers:** integers are formatted with `itoa`, floats with `ryu`.
  Source: https://www.rcxprotocol.org/spec/v1/03-canonical-json
  ("formats integers with `itoa 1.0.18` and floats with `ryu 1.0.23`").
  Trap: serde_json serializes the float `1.0` as `1.0`, while
  `JSON.stringify(1.0)` produces `1`. If any endpoint emits floats, the
  characterization suite must compare bytes, not parsed values.

## 10. Semantic traps: axum/tokio/hyper vs node:http

| # | Behavior | Node `http` (original) | axum / hyper (port) | What to do |
|---|---|---|---|---|
| 1 | Route capture syntax | N/A (hand-rolled router) | `{id}` in 0.8; `:id` is literal and panics at startup | Write `/api/songs/{id}` (§3) |
| 2 | Trailing slash | Depends on the original's router (verify) | `/api/songs` ≠ `/api/songs/`; no redirect by default | If the original treats them the same, add `tower_http::normalize_path::NormalizePathLayer::trim_trailing_slash()` |
| 3 | HEAD | Hand-rolled: GET handler does **not** answer HEAD unless coded | `get()` routes answer HEAD automatically, body stripped | Free behavior — but if the original 404s/405s on HEAD, the port diverges; check TESTS.md expectations |
| 4 | Unknown path | Original's own 404 (likely JSON) | Router default fallback: **404 with empty body** | Set `.fallback(...)` returning the original's 404 JSON shape |
| 5 | Known path, wrong method | Original's own response | **405 Method Not Allowed** with an `Allow` header (e.g. `Allow: GET,HEAD`) | Accept, or override with `.method_not_allowed_fallback(...)` |
| 6 | `Content-Type` on JSON responses | `application/json; charset=utf-8` | `Json<T>` sets bare `application/json` | Set the header explicitly if byte parity matters (§4.2) |
| 7 | JSON error bodies | Original's own JSON error shape | axum rejections are **plain-text** bodies (e.g. `Failed to parse the request body as JSON: …`) | Wrap `Json` in a custom extractor or map `JsonRejection` → the original's error JSON |
| 8 | Body limit | 1 MiB, hand-checked | Default 2 MiB | `.layer(DefaultBodyLimit::max(1024 * 1024))` (§7) |
| 9 | Invalid JSON status | Probably a single 400 | 400 syntax / 422 shape / 415 content-type split | Accept the split, or map to one status (§8) |
| 10 | `Content-Type` check on requests | Probably none | `Json` requires `application/json` → 415 otherwise | Use `Bytes` + manual parse where the original is lenient (§8) |
| 11 | Key order in JSON | Insertion order (integer-like keys first) | Structs: declaration order; `Value` without `preserve_order`: alphabetical | Typed structs; enable `preserve_order` if `Value` is used (§9.1) |
| 12 | Float formatting | `JSON.stringify(1.0)` → `1` | serde_json/ryu → `1.0` | Byte-compare in tests; avoid floats in DTOs if possible (§9.2) |
| 13 | `Date` header | Sent automatically (`Date`, `Connection: keep-alive`) | hyper sends `Date` automatically too (default `true`, `date_header(false)` to disable); **no `Server` header** on either side | Nothing to do; do not add a `Server` header |
| 14 | `Connection` header | Node sends `Connection: keep-alive` explicitly | hyper omits it for HTTP/1.1 keep-alive (it is the protocol default) | Harmless wire difference; clients must not be asserted on it |
| 15 | Header name casing | Node writes header names as set (typically `Content-Type`) | hyper writes header names **lowercase** by default (`title_case_headers` defaults to `false`) | Harmless per RFC 9110 (header names are case-insensitive), but byte-level response comparisons in tests must normalize case |
| 16 | Idle keep-alive timeout | `server.keepAliveTimeout` default **5 s** | hyper has no equivalent idle-close by default in the `axum::serve` path | Open item: decide whether idle sockets staying open longer matters on the LAN; see §11 |
| 17 | Static directory redirect | Original probably serves `index.html` directly or 404s | `ServeDir` 307-redirects `/dir` → `/dir/` by default | Set `.redirect_to_trailing_slash(false)` if the original does not redirect |
| 18 | Static index handling | Original's own logic | `append_index_html_on_directories` defaults to true | Set `.append_index_html_on_directories(false)` if the original does not do this |
| 19 | Range: multi-range | Hand-rolled (verify what it does) | 416 `Cannot serve multipart range requests` by default | Set `.ignore_multi_range_requests(true)` if the original serves 200-full (§6) |
| 20 | Range: conditional requests | Original likely ignores `If-None-Match` / `If-Modified-Since` | `ServeDir` answers 304 / 412 with strong ETags (new in 0.7) | Accept (browsers benefit); note the extra `ETag`/`Last-Modified` headers in tests |
| 21 | Percent-decoding | `decodeURIComponent` on params (presumably) | `Path` percent-decodes captures; non-UTF-8 → 400 | Same semantics; note `%2F` decodes to `/` inside a capture on both sides |
| 22 | Concurrency model | Single-threaded event loop; blocking fs calls stall all requests | Multi-threaded tokio runtime; blocking calls must go in `spawn_blocking` | SQLite access (separate research topic) must not block runtime threads |

Sources for the table:

- 405 + `Allow` header, GET-answers-HEAD: https://www.irurueta.com/backend/axum/getting-started.html
  ("unmatched paths go to the router's default fallback (`404`)… a matched
  path with an unsupported method gets `405` with an `Allow` header. A `GET`
  route also answers `HEAD` requests automatically, with the body stripped.")
  and axum source https://docs.rs/axum/latest/src/axum/routing/method_routing.rs.html
  ("Note that `get` routes will also be called for `HEAD` requests but will
  have the response body removed").
- Trailing slashes differ: https://www.tutorialscenter.com/axum/routing-basics
  ("`/notes` and `/notes/` are different routes. Axum does not redirect
  between them").
- hyper `Date` header default true: https://docs.rs/hyper/1.4.0/hyper/server/conn/http1/struct.Builder.html
  ("Set whether the `date` header should be included in HTTP responses…
  Default is true"); title-case headers default false: same page.
- Node default response headers (`Date`, `Connection: keep-alive`,
  `Transfer-Encoding: chunked` when streaming):
  https://www.w3tutorials.net/blog/node-response-writehead-method/
  (secondary); Node `server.keepAliveTimeout` default 5000 ms:
  https://nodejs.org/docs/latest-v11.x/api/http.html
  (`server.keepAliveTimeout`, "Default: 5000 (5 sec)").
- `ServeDir` redirect / index defaults: §5 sources above.

## 11. Open verification items (for the characterization suite)

1. Confirm the over-limit `Json` rejection status is 413 (secondary sources
   only so far).
2. Confirm the original's behavior for: HEAD on API routes; trailing-slash
   URLs; unknown paths (exact 404 body); wrong-method on a known path;
   multi-range requests; `If-None-Match` on static files; JSON posted with a
   missing/wrong `Content-Type`.
3. Confirm the original's exact `Content-Type` header value on JSON responses
   (`application/json; charset=utf-8` vs bare) and on 404/error responses.
4. Byte-compare one JSON response containing Spanish text (accents, `ñ`,
   `¿¡`) and one containing a float, Node vs Rust.
5. Idle keep-alive: measure whether the Node server closes idle connections
   after 5 s and whether the Rust server's longer-lived connections matter
   for the karaoke clients (browsers, phones on LAN — almost certainly not).
6. Header-name casing: byte-level test comparisons must lowercase header
   names before comparing (hyper lowercase vs Node as-set).

## 12. Recommended versions / features

```toml
[dependencies]
# HTTP stack — verified 2026-11-20 (see §1 for sources)
axum = { version = "0.8.9", features = ["json"] }          # default features already include http1, tokio, query, form
tokio = { version = "1.54.1", features = ["macros", "rt-multi-thread", "net", "fs", "io-util", "sync", "time"] }
tower-http = { version = "0.7.1", features = ["fs"] }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }  # preserve_order: safety net for Value-based responses (§9.1)

# Effective MSRV: Rust 1.85 (set by tokio 1.54.1). Pin it:
# rust-version = "1.85"   (in [package])
```

Notes on the recommendation:

- `axum = "0.8"` alone would resolve to 0.8.9 today; pinning `0.8.9` records
  what was actually researched. Same for tokio `1.54.1` and tower-http `0.7.1`.
- Do **not** use `tokio = { features = ["full"] }` for the final binary: it
  pulls `process`, `signal`, `parking_lot`, `io-std`, which the port never
  uses. The subset above is sufficient and keeps the binary smaller.
- Do **not** enable axum's `http2`, `multipart`, or `ws` features: the Node
  original is HTTP/1.1 + JSON only.
- `tower-http` needs only `fs`. Add `normalize-path` only if the trailing-slash
  decision in §10 row 2 goes that way.
- `serde_json`'s `preserve_order` is a belt-and-braces choice: typed structs
  already preserve order, but any `Value`-based response stays insertion-ordered
  instead of silently sorting keys alphabetically (§9.1).

## Sources (all URLs cited above)

- https://lib.rs/crates/axum
- https://docs.rs/crate/axum/latest/source/Cargo.toml
- https://docs.rs/crate/axum/latest
- https://docs.rs/axum/latest/axum/struct.Json.html
- https://docs.rs/axum/latest/axum/response/index.html
- https://docs.rs/axum/latest/axum/extract/path/struct.Path.html
- https://docs.rs/axum/latest/axum/extract/struct.DefaultBodyLimit.html
- https://docs.rs/axum/latest/src/axum/routing/method_routing.rs.html
- https://docs.rs/crate/axum/0.7.9/source/src/json.rs
- https://lib.rs/crates/tokio
- https://docs.rs/crate/tokio/latest/source/Cargo.toml
- https://docs.rs/crate/tokio/latest/source/CHANGELOG.md
- https://docs.rs/crate/tokio/latest
- https://lib.rs/crates/tower-http
- https://docs.rs/crate/tower-http/latest/source/Cargo.toml
- https://docs.rs/tower-http/latest/tower_http/services/struct.ServeDir.html
- https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_dir/mod.rs.html
- https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_dir/future.rs.html
- https://docs.rs/tower-http/latest/src/tower_http/services/fs/serve_file.rs.html
- https://github.com/tower-rs/tower-http/issues/660
- https://github.com/libp2p/rust-libp2p/commit/80077b1874461d8940f2b7c72055afdb6499f0e8
- https://docs.rs/serde_json/latest/serde_json/struct.Map.html
- https://docs.rs/serde_json/latest/serde_json/struct.Deserializer.html
- https://docs.rs/hyper/1.4.0/hyper/server/conn/http1/struct.Builder.html
- https://www.rcxprotocol.org/spec/v1/03-canonical-json
- https://www.irurueta.com/backend/axum/routing.html
- https://www.irurueta.com/backend/axum/getting-started.html
- https://www.irurueta.com/backend/axum/extractors.html
- https://www.irurueta.com/backend/axum/tower-http-catalog.html
- https://www.tutorialscenter.com/axum/routing-basics
- https://www.tutorialscenter.com/axum/json-and-form-bodies
- https://jsonic.io/guides/json-in-axum
- https://flaviocopes.com/tools/mime-types/
- https://useotools.com/audio-video-mime-types
- https://nodejs.org/docs/latest-v11.x/api/http.html
- https://www.w3tutorials.net/blog/node-response-writehead-method/
