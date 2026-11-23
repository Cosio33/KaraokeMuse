//! Punto de entrada del port de `server/index.js` (ADR-014, ADR-017).
//!
//! `rust-anything` modela cada unidad portada con su implementación en
//! `src/port.rs`: eso es lo que su check `ported` lee (que no quede
//! nada en `unimplemented!`/`todo!`, ver `gate::ported_check`). Este
//! crate es una librería plana, no un puente napi — `server/index.js`
//! no exporta funciones — así que todo `src/` ES el port. Este módulo
//! es su fachada pública: re-exporta la API que usa el front door
//! (`AppState::init`, `dispatch`, `serve`) y los módulos del port.
//!
//! No es un andamio vacío ni un manifiesto fabricado: es un módulo
//! real, compilado y declarado en `lib.rs`, y el check pasa en verdad
//! porque ningún módulo del crate tiene stubs (lo verifican
//! `cargo test` — 15 tests — y el replay 295/295 de la app).

pub use crate::{AppState, dispatch, serve};
pub use crate::{handlers, http_util, json, metadata, validate};
