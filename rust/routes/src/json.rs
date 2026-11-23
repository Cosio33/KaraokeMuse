//! Escritura JSON compatible con `JSON.stringify` de Node y conversión
//! de filas SQLite a valores JSON (ADR-002, ADR-011).
//!
//! `serde_json::to_string` NO se usa para la salida: su motor zmij
//! formatea floats en científica desde 1e16 (JS desde 1e21). Los
//! números se formatean con `karaoke_db::js::js_number_to_string` y
//! las claves se conservan en orden de inserción (`preserve_order`).

use rusqlite::types::ValueRef;
use serde_json::Value;

/// `JSON.stringify` sobre un `serde_json::Value` con formato numérico
/// de JS. Las claves salen en orden de inserción (el `Map` se construye
/// con `preserve_order`).
pub fn write_json(v: &Value) -> String {
    let mut out = String::new();
    write_value(v, &mut out);
    out
}

fn write_value(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&karaoke_db::js::js_number_to_string(
            n.as_f64().unwrap_or(f64::NAN),
        )),
        Value::String(s) => {
            // El escapado de serde_json coincide con JSON.stringify
            // para el texto que maneja esta app (ASCII y español).
            out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()));
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (k, val)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_else(|_| "\"\"".to_string()));
                out.push(':');
                write_value(val, out);
            }
            out.push('}');
        }
    }
}

/// Una fila SQLite como objeto JSON, con las claves en el orden de las
/// columnas del `SELECT` (el mismo orden que `SELECT *` de Node).
pub fn rows_to_json(
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
) -> Result<Vec<Value>, rusqlite::Error> {
    let names: Vec<String> = stmt.column_names().map(|s| s.to_string()).collect();
    let rows = stmt.query_map(params, |row| {
        let mut obj = serde_json::Map::new();
        for (i, name) in names.iter().enumerate() {
            obj.insert(name.clone(), value_ref_to_json(row.get_ref(i)?));
        }
        Ok(Value::Object(obj))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// `SELECT ... LIMIT 1` (o cero filas): la primera fila o `None`.
pub fn query_one(
    conn: &rusqlite::Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Option<Value>, rusqlite::Error> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = rows_to_json(&mut stmt, params)?;
    Ok(rows.drain(..).next())
}

/// `SELECT` completo como lista de objetos JSON.
pub fn query_all(
    conn: &rusqlite::Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Value>, rusqlite::Error> {
    let mut stmt = conn.prepare(sql)?;
    rows_to_json(&mut stmt, params)
}

/// Un valor SQLite como valor JSON. Los BLOB llegan como en Node:
/// `JSON.stringify(Buffer)` → `{"type":"Buffer","data":[...]}`.
/// (Ninguna tabla de KaraokeMuse guarda BLOBs; es por completitud.)
fn value_ref_to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Number(i.into()),
        ValueRef::Real(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => {
            let mut obj = serde_json::Map::new();
            obj.insert("type".to_string(), Value::String("Buffer".to_string()));
            obj.insert(
                "data".to_string(),
                Value::Array(b.iter().map(|x| Value::Number((*x).into())).collect()),
            );
            Value::Object(obj)
        }
    }
}

/// Truthiness de JS (`!!v`): `undefined`/`null`/`false`/`0`/`NaN`/`""`
/// son falsy; todo lo demás (incluidos `[]` y `{}`) es truthy.
/// `None` representa `undefined` (campo ausente).
pub fn js_truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            f != 0.0 && !f.is_nan()
        }
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `String(v)` de JS (ADR-011, vía `karaoke_db::js::js_to_string`).
/// `None` (`undefined`) → `"undefined"`.
pub fn js_to_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".to_string(),
        Some(v) => karaoke_db::js::js_to_string(&karaoke_db::js::JsValue::decode(v)),
    }
}

/// `String(v || '')` de JS: el valor si es truthy, `""` si no.
pub fn js_string_or_empty(v: Option<&Value>) -> String {
    if js_truthy(v) {
        js_to_string(v)
    } else {
        String::new()
    }
}

/// `String(v).trim()` con el conjunto de blancos de JS
/// (incluye U+FEFF, que el `trim` de Rust no quita).
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\u{0009}'..='\u{000D}'
                | '\u{0020}'
                | '\u{00A0}'
                | '\u{1680}'
                | '\u{2000}'..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
        )
    })
}

/// `Number(str)` de JS para cadenas de query (`minRating`, `count`).
/// `""` → 0; `"Infinity"`/`"+Infinity"` → ∞; `"0x.."` → hex sin signo;
/// resto con el parseo de Rust (que coincide con JS salvo `inf`/`nan`
/// en minúsculas, que JS rechaza).
pub fn js_number(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let lower = t.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "inf" | "+inf" | "-inf" | "infinity" | "+infinity" | "-infinity" | "nan" | "+nan" | "-nan"
    ) {
        return f64::NAN;
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        if !hex.is_empty()
            && let Ok(n) = i64::from_str_radix(hex, 16)
        {
            return n as f64;
        }
        return f64::NAN;
    }
    if let Some(bin) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        if !bin.is_empty()
            && let Ok(n) = i64::from_str_radix(bin, 2)
        {
            return n as f64;
        }
        return f64::NAN;
    }
    if let Some(oct) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        if !oct.is_empty()
            && let Ok(n) = i64::from_str_radix(oct, 8)
        {
            return n as f64;
        }
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Math.min` de JS: `NaN` se propaga (el `min` de Rust lo ignora).
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `Math.max` de JS: `NaN` se propaga.
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_use_js_format() {
        assert_eq!(write_json(&serde_json::json!(5)), "5");
        assert_eq!(write_json(&serde_json::json!(3.5)), "3.5");
        assert_eq!(write_json(&serde_json::json!(1e21)), "1e+21");
        assert_eq!(write_json(&serde_json::json!(1e16)), "10000000000000000");
    }

    #[test]
    fn keys_keep_insertion_order() {
        let mut m = serde_json::Map::new();
        m.insert("b".to_string(), serde_json::json!(1));
        m.insert("a".to_string(), serde_json::json!(2));
        assert_eq!(write_json(&Value::Object(m)), "{\"b\":1,\"a\":2}");
    }

    #[test]
    fn js_number_parity() {
        assert_eq!(js_number(""), 0.0);
        assert_eq!(js_number("  5 "), 5.0);
        assert!(js_number("abc").is_nan());
        assert!(js_number("abc").is_nan());
        assert_eq!(js_number("0x10"), 16.0);
        assert!(js_number("-0x10").is_nan());
        assert_eq!(js_number("Infinity"), f64::INFINITY);
        assert!(js_number("infinity").is_nan());
        assert_eq!(js_number("1e3"), 1000.0);
    }
}
