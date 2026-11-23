//! The Rust port of `server/db.js`.
//!
//! Replace each `unimplemented!` with the port of the source function. Return `Err` where
//! the source throws or raises: the caller gets an error there too. After changing a
//! signature, run `rust-anything refresh` to regenerate the bindings. `rust-anything diff` checks
//! behavior against what the source recorded.

#![forbid(unsafe_code)]

/// Port of `randomCode` (`server/db.js:9`).
pub fn random_code(length: Option<f64>) -> Result<serde_json::Value, String> {
    let _ = &length;
    unimplemented!("port `randomCode` from server/db.js")
}

/// Port of `newId` (`server/db.js:17`).
pub fn new_id(prefix: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &prefix;
    unimplemented!("port `newId` from server/db.js")
}

/// Port of `openDatabase` (`server/db.js:21`).
pub fn open_database(data_dir: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &data_dir;
    unimplemented!("port `openDatabase` from server/db.js")
}

/// Port of `seed` (`server/db.js:100`).
pub fn seed(db: serde_json::Value, opts: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = (&db, &opts);
    unimplemented!("port `seed` from server/db.js")
}
