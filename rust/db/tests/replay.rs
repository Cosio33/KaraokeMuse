//! Replay de los 140 casos grabados de `server/db.js`
//! (`rust/db-rs/cases.jsonl`) contra el port `karaoke-db`.
//!
//! Un solo test (el cambio de directorio de trabajo para el caso ok
//! de `openDatabase` es global al proceso). Las salidas aleatorias se
//! comparan por forma, no por valor (ADR-006/ADR-010):
//! - `randomCode` ok → misma longitud + solo caracteres del alfabeto.
//! - `newId` ok → mismo prefijo (`String()` de JS) + `_` + 16 hex.
//! - `openDatabase` ok → `isOpen`/`isTransaction`/`limits` iguales
//!   al objeto grabado + archivo con las 7 tablas.
//! - Errores → mensaje exacto de Node, byte a byte.
//!
//! Ruta de los casos: variable `DB_CASES`, o la copia dentro del
//! crate (`tests/cases.jsonl`, snapshot de `../db-rs/cases.jsonl`,
//! que sigue siendo el artefacto original de la grabación — si los
//! casos se regraban hay que refrescar la copia), o el artefacto
//! original `../db-rs/cases.jsonl`. La copia viaja con el crate, así
//! que el test también pasa en la copia del lift (ADR-018).

use std::path::PathBuf;

use karaoke_db::js::{JsValue, js_to_string};
use karaoke_db::{RANDOM_CODE_CHARS, open_database, random_code, seed_from_json};
use serde_json::Value as Json;

fn cases_path() -> PathBuf {
    if let Ok(p) = std::env::var("DB_CASES") {
        return PathBuf::from(p);
    }
    let in_crate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases.jsonl");
    if in_crate.is_file() {
        return in_crate;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("db-rs")
        .join("cases.jsonl")
}

fn limits_json(db: &karaoke_db::Db) -> Json {
    let l = db.limits().expect("límites SQLite legibles");
    serde_json::json!({
        "length": l.length,
        "sqlLength": l.sql_length,
        "column": l.column,
        "exprDepth": l.expr_depth,
        "compoundSelect": l.compound_select,
        "vdbeOp": l.vdbe_op,
        "functionArg": l.function_arg,
        "attach": l.attach,
        "likePatternLength": l.like_pattern_length,
        "variableNumber": l.variable_number,
        "triggerDepth": l.trigger_depth,
    })
}

#[test]
fn replay_db_cases() {
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
        140,
        "se esperaban 140 casos en {} (copia desactualizada?)",
        path.display()
    );

    // Directorio temporal para los casos de openDatabase (el caso ok
    // usa la ruta relativa "x"). Bajo target/ (~/workspace), nunca /tmp.
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("replay-tmp")
        .join(format!("run-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let orig_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&base).unwrap();

    let mut failures: Vec<String> = Vec::new();
    let mut counts: Vec<(String, usize, usize)> = Vec::new(); // (fn, ok, total)
    let mut count = |name: &str, passed: bool| match counts.iter_mut().find(|(n, _, _)| n == name) {
        Some(e) => {
            e.2 += 1;
            if passed {
                e.1 += 1;
            }
        }
        None => counts.push((name.to_string(), passed as usize, 1)),
    };

    for (i, case) in cases.iter().enumerate() {
        let fname = case["fn"].as_str().unwrap_or("?");
        let args: Vec<JsValue> = case["args"]
            .as_array()
            .map(|a| a.iter().map(JsValue::decode).collect())
            .unwrap_or_default();
        let recorded_ok = case.get("ok");
        let recorded_err = case.get("error").and_then(|e| e.as_str());
        let tag = format!("caso #{i} {fname} args={}", case["args"]);

        match fname {
            "randomCode" => {
                let arg = args.first();
                match random_code(arg) {
                    Ok(out) => {
                        let want_len = recorded_ok
                            .and_then(|v| v.as_str())
                            .map(str::len)
                            .unwrap_or(usize::MAX);
                        let ok = out.len() == want_len
                            && out.chars().all(|c| RANDOM_CODE_CHARS.contains(c));
                        if !ok {
                            failures.push(format!(
                                "{tag}: randomCode ok con forma incorrecta: {out:?} (len esperado {want_len})"
                            ));
                        }
                        count(fname, ok);
                    }
                    Err(e) => {
                        let ok = recorded_err == Some(e.to_string().as_str());
                        if !ok {
                            failures.push(format!(
                                "{tag}: randomCode error distinto:\n  port:     {e}\n  grabado:  {}",
                                recorded_err.unwrap_or("<sin error grabado>")
                            ));
                        }
                        count(fname, ok);
                    }
                }
            }
            "newId" => {
                let arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                let out = karaoke_db::new_id(&arg).expect("RNG del SO disponible");
                let want_prefix = format!("{}_", js_to_string(&arg));
                let ok = recorded_ok.is_some()
                    && out.starts_with(&want_prefix)
                    && out[want_prefix.len()..].len() == 16
                    && out[want_prefix.len()..]
                        .chars()
                        .all(|c| c.is_ascii_hexdigit());
                if !ok {
                    failures.push(format!(
                        "{tag}: newId con forma incorrecta: {out:?} (prefijo esperado {want_prefix:?})"
                    ));
                }
                count(fname, ok);
            }
            "openDatabase" => {
                let arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                match open_database(&arg) {
                    Ok(db) => {
                        let mut ok = true;
                        let mut why = String::new();
                        if !db.is_open() {
                            ok = false;
                            why.push_str("isOpen != true; ");
                        }
                        if db.is_transaction() {
                            ok = false;
                            why.push_str("isTransaction != false; ");
                        }
                        if let Some(want) = recorded_ok.and_then(|v| v.get("limits")) {
                            if limits_json(&db) != *want {
                                ok = false;
                                why.push_str(&format!(
                                    "limits distintos: port={} grabado={want}; ",
                                    limits_json(&db)
                                ));
                            }
                        } else {
                            ok = false;
                            why.push_str("sin objeto ok grabado; ");
                        }
                        match db.table_names() {
                            Ok(names) => {
                                for t in [
                                    "users",
                                    "rooms",
                                    "songs",
                                    "queue",
                                    "requests",
                                    "sessions",
                                    "player_state",
                                ] {
                                    if !names.iter().any(|n| n == t) {
                                        ok = false;
                                        why.push_str(&format!("falta tabla {t}; "));
                                    }
                                }
                            }
                            Err(e) => {
                                ok = false;
                                why.push_str(&format!("table_names falló: {e}; "));
                            }
                        }
                        if !base.join("x").join("karaokemuse.sqlite").exists() {
                            ok = false;
                            why.push_str("no se creó x/karaokemuse.sqlite; ");
                        }
                        drop(db);
                        if !ok {
                            failures.push(format!("{tag}: openDatabase ok incorrecto: {why}"));
                        }
                        count(fname, ok);
                    }
                    Err(e) => {
                        let ok = recorded_err == Some(e.to_string().as_str());
                        if !ok {
                            failures.push(format!(
                                "{tag}: openDatabase error distinto:\n  port:     {e}\n  grabado:  {}",
                                recorded_err.unwrap_or("<sin error grabado>")
                            ));
                        }
                        count(fname, ok);
                    }
                }
            }
            "seed" => {
                let db_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                let opts_arg = args.get(1).cloned().unwrap_or(JsValue::Undefined);
                match seed_from_json(&db_arg, &opts_arg) {
                    Ok(_) => {
                        failures.push(format!("{tag}: seed_from_json no debería tener éxito"));
                        count(fname, false);
                    }
                    Err(e) => {
                        let ok = recorded_err == Some(e.to_string().as_str());
                        if !ok {
                            failures.push(format!(
                                "{tag}: seed error distinto:\n  port:     {e}\n  grabado:  {}",
                                recorded_err.unwrap_or("<sin error grabado>")
                            ));
                        }
                        count(fname, ok);
                    }
                }
            }
            other => {
                failures.push(format!("{tag}: función desconocida {other}"));
                count(fname, false);
            }
        }
    }

    std::env::set_current_dir(&orig_cwd).unwrap();
    let _ = std::fs::remove_dir_all(&base);

    let total: usize = counts.iter().map(|c| c.2).sum();
    let passed: usize = counts.iter().map(|c| c.1).sum();
    println!("replay db: {passed}/{total} casos");
    for (name, ok, n) in &counts {
        println!("  {name}: {ok}/{n}");
    }
    assert!(
        failures.is_empty(),
        "replay db: {}/{} fallos:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );
}
