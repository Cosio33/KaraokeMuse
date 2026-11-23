//! Replay de los casos puros de `server/auth.js`
//! (`rust/auth-cases.jsonl`, generados por `rust/auth-record.mjs`
//! contra el Node original). Las sesiones se prueban en
//! `tests/sessions.rs` (necesitan una BD viva).
//!
//! Ruta de los casos: variable `AUTH_CASES`, o la copia dentro del
//! crate (`tests/auth-cases.jsonl`, snapshot de
//! `../auth-cases.jsonl`, que sigue siendo el artefacto original —
//! si los casos se regeneran hay que refrescar la copia), o el
//! artefacto original `../auth-cases.jsonl`. La copia viaja con el
//! crate, así que el test también pasa en la copia del lift (ADR-018).

use std::path::PathBuf;

use karaoke_auth::{PublicUser, UserRow, hash_password, public_user, verify_password};
use serde_json::Value as Json;

fn cases_path() -> PathBuf {
    if let Ok(p) = std::env::var("AUTH_CASES") {
        return PathBuf::from(p);
    }
    let in_crate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("auth-cases.jsonl");
    if in_crate.is_file() {
        return in_crate;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("auth-cases.jsonl")
}

fn is_hex(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

#[test]
fn replay_auth_cases() {
    let path = cases_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", path.display()));
    let cases: Vec<Json> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("línea JSON válida"))
        .collect();
    assert!(!cases.is_empty(), "sin casos en {}", path.display());
    assert_eq!(
        cases.len(),
        9,
        "se esperaban 9 casos en {} (copia desactualizada?)",
        path.display()
    );

    let mut failures: Vec<String> = Vec::new();
    let mut passed = 0usize;

    for (i, case) in cases.iter().enumerate() {
        let fname = case["fn"].as_str().unwrap_or("?");
        let tag = format!("caso #{i} {fname} args={}", case["args"]);
        let ok = match fname {
            "verifyPassword" => {
                let args = case["args"].as_array().unwrap();
                let got = verify_password(
                    args[0].as_str().unwrap(),
                    args[1].as_str().unwrap(),
                    args[2].as_str().unwrap(),
                );
                let want = case["ok"].as_bool().unwrap();
                if got != want {
                    failures.push(format!("{tag}: verifyPassword = {got}, grabado {want}"));
                }
                got == want
            }
            "hashPassword" => {
                let pw = case["args"][0].as_str().unwrap();
                let (salt, hash) = hash_password(pw).expect("hashPassword no falla");
                let shape_ok = salt.len() == 32
                    && hash.len() == 128
                    && is_hex(&salt)
                    && is_hex(&hash)
                    && verify_password(pw, &salt, &hash);
                if !shape_ok {
                    failures.push(format!(
                        "{tag}: hashPassword con forma incorrecta: salt={salt:?} hash={hash:?}"
                    ));
                }
                shape_ok
            }
            "publicUser" => {
                let arg = &case["args"][0];
                let user = if arg.is_null() {
                    None
                } else {
                    Some(UserRow {
                        id: arg["id"].as_str().unwrap().to_string(),
                        username: arg["username"].as_str().unwrap().to_string(),
                        role: arg["role"].as_str().unwrap().to_string(),
                        created_at: arg["created_at"].as_str().unwrap().to_string(),
                    })
                };
                let got = public_user(user.as_ref());
                let want = if case["ok"].is_null() {
                    None
                } else {
                    Some(PublicUser {
                        id: case["ok"]["id"].as_str().unwrap().to_string(),
                        username: case["ok"]["username"].as_str().unwrap().to_string(),
                        role: case["ok"]["role"].as_str().unwrap().to_string(),
                    })
                };
                if got != want {
                    failures.push(format!("{tag}: publicUser = {got:?}, grabado {want:?}"));
                }
                got == want
            }
            other => {
                failures.push(format!("{tag}: función desconocida {other}"));
                false
            }
        };
        if ok {
            passed += 1;
        }
    }

    println!("replay auth: {passed}/{} casos", cases.len());
    assert!(
        failures.is_empty(),
        "replay auth: {} fallos:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
