//! Semántica de valores y mensajes de Node.js necesaria para la paridad
//! con `server/db.js` (ADR-011).
//!
//! `serde_json` NO reproduce el formato numérico de JS (su motor zmij
//! usa notación científica desde 1e16; JS desde 1e21 — ver
//! `rust/research/json-network.md` §3.2), así que el formato de
//! números y los mensajes de error de Node se implementan aquí a mano,
//! verificados con sondeos en vivo contra Node 24 el 2026-11-20.

use serde_json::Value as Json;

/// Un valor JS tal como lo ve el código original. Los casos grabados
/// (`rust/db-rs/cases.jsonl`) codifican los valores que JSON no puede
/// representar con marcadores: `{"$undefined": true}` y
/// `{"$num": "<literal JS>"}` (p. ej. `"-0"`, `"Infinity"`, `"NaN"`).
#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<JsValue>),
    Object(Vec<(String, JsValue)>),
}

impl JsValue {
    /// Decodifica un `serde_json::Value` (posiblemente con marcadores
    /// `$undefined` / `$num` del grabador) a `JsValue`.
    pub fn decode(v: &Json) -> JsValue {
        match v {
            Json::Null => JsValue::Null,
            Json::Bool(b) => JsValue::Bool(*b),
            Json::Number(n) => JsValue::Num(n.as_f64().unwrap_or(f64::NAN)),
            Json::String(s) => JsValue::Str(s.clone()),
            Json::Array(items) => JsValue::Array(items.iter().map(JsValue::decode).collect()),
            Json::Object(map) => {
                if map.len() == 1 {
                    if let Some(Json::Bool(true)) = map.get("$undefined") {
                        return JsValue::Undefined;
                    }
                    if let Some(Json::String(lit)) = map.get("$num") {
                        return JsValue::Num(parse_js_number_literal(lit));
                    }
                }
                JsValue::Object(
                    map.iter()
                        .map(|(k, v)| (k.clone(), JsValue::decode(v)))
                        .collect(),
                )
            }
        }
    }
}

/// Parsea un literal numérico JS (`"-0"`, `"Infinity"`, `"NaN"`,
/// `"1e+21"`, `"9007199254740992"`). Rust `str::parse::<f64>` acepta
/// los mismos literales salvo `Infinity`/`NaN` con signo, que se
/// tratan aparte.
fn parse_js_number_literal(lit: &str) -> f64 {
    match lit {
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        "NaN" => f64::NAN,
        _ => lit.parse::<f64>().unwrap_or(f64::NAN),
    }
}

/// `Number::toString` de ECMAScript: la cadena decimal más corta que
/// redondea al mismo `f64`, con notación plana para
/// 1e-6 <= |x| < 1e21 y exponencial fuera de ahí. Se parte de los
/// dígitos más cortos de Rust (`{:e}`) y se reordenan con las reglas
/// de umbral de JS.
pub fn js_number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if x == 0.0 {
        // JS: String(-0) === "0".
        return "0".to_string();
    }
    let neg = x < 0.0;
    // Rust {:e} da la forma más corta: "d.dddde<exp>" o "de<exp>".
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exponent: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    let e = exponent; // valor = d[0].d[1..] × 10^e
    let body = if e >= 21 || e <= -7 {
        // Exponencial: "1e+21", "1.5e-7", "1.7976931348623157e+308".
        let sign = if e >= 0 { "+" } else { "-" };
        if n == 1 {
            format!("{digits}e{sign}{}", e.abs())
        } else {
            format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e.abs())
        }
    } else if e >= n - 1 {
        // Entero plano: dígitos + ceros.
        let zeros = (e - n + 1) as usize;
        format!("{digits}{}", "0".repeat(zeros))
    } else if e >= 0 {
        // Decimal plano: punto tras e+1 dígitos.
        let split = (e + 1) as usize;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        // 0.000ddd: "0." + ceros + dígitos.
        let zeros = (-e - 1) as usize;
        format!("0.{}{digits}", "0".repeat(zeros))
    };
    if neg { format!("-{body}") } else { body }
}

/// Coerción `String()` de JS (la que usa el template literal de
/// `newId` y `Array.prototype.join`):
/// - `undefined` → `"undefined"`, `null` → `"null"`, booleanos y
///   números con su formato JS.
/// - Arrays: elementos unidos con `,`, donde `null`/`undefined`
///   aportan `""` (y los objetos anidados dan `"[object Object]"`).
/// - Objetos planos → `"[object Object]"`.
pub fn js_to_string(v: &JsValue) -> String {
    match v {
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Null => "null".to_string(),
        JsValue::Bool(b) => b.to_string(),
        JsValue::Num(n) => js_number_to_string(*n),
        JsValue::Str(s) => s.clone(),
        JsValue::Array(items) => items
            .iter()
            .map(|it| match it {
                JsValue::Null | JsValue::Undefined => String::new(),
                other => js_to_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        JsValue::Object(_) => "[object Object]".to_string(),
    }
}

/// Sufijo `Received ...` de los errores `ERR_INVALID_ARG_TYPE` de
/// Node, verificado contra Node 24:
/// `type number (0)`, `type string ('abc')`, `type boolean (true)`,
/// `null`, `undefined`, `an instance of Array`, `an instance of
/// Object`. (Los strings con comillas u otros caracteres especiales
/// no aparecen en los casos grabados; se envuelven en comillas
/// simples sin más escape.)
pub fn received_suffix(v: &JsValue) -> String {
    match v {
        JsValue::Null => "null".to_string(),
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Bool(b) => format!("type boolean ({b})"),
        JsValue::Num(n) => format!("type number ({})", js_number_to_string(*n)),
        JsValue::Str(s) => format!("type string ('{s}')"),
        JsValue::Array(_) => "an instance of Array".to_string(),
        JsValue::Object(_) => "an instance of Object".to_string(),
    }
}

/// Mensaje completo `ERR_INVALID_ARG_TYPE`:
/// `The "size" argument must be of type number. Received ...`
pub fn invalid_arg_type(name: &str, expected: &str, received: &JsValue) -> String {
    format!(
        "The \"{name}\" argument must be of type {expected}. Received {}",
        received_suffix(received)
    )
}

/// Formato del valor en el `Received` de `ERR_OUT_OF_RANGE` de
/// `crypto.randomBytes(size)`, verificado con sondeos en vivo contra
/// Node 24 (ADR-011):
/// - Notación exponencial: se inserta `_` antes de cada carácter cuya
///   distancia al final de la cadena es múltiplo de 3
///   (`1e+21` → `1e_+21`; `1.5e+21` → `1_.5e_+21`).
/// - Enteros decimales con |v| > 2^32: dígitos agrupados de 3 en 3
///   desde la derecha (`9007199254740992` → `9_007_199_254_740_992`).
/// - Todo lo demás queda plano (`2147483648`, `4294967296`, `-2.5`,
///   `NaN`, `Infinity`).
pub fn range_received(n: f64) -> String {
    let s = js_number_to_string(n);
    if s.contains('e') {
        let mut out = String::with_capacity(s.len() + s.len() / 3);
        for (i, ch) in s.char_indices() {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                out.push('_');
            }
            out.push(ch);
        }
        out
    } else if n.is_finite() && n.fract() == 0.0 && n.abs() > 4294967296.0 {
        let (sign, digits) = match s.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", s.as_str()),
        };
        let mut out = String::with_capacity(s.len() + s.len() / 3);
        for (i, ch) in digits.char_indices() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push('_');
            }
            out.push(ch);
        }
        format!("{sign}{out}")
    } else {
        s
    }
}

/// Mensaje completo `ERR_OUT_OF_RANGE` de `crypto.randomBytes`:
/// `The value of "size" is out of range. It must be >= 0 &&
/// <= 2147483647. Received ...`
pub fn out_of_range_size(n: f64) -> String {
    format!(
        "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received {}",
        range_received(n)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_matches_js() {
        // Tabla verificada contra Node 24 (`String(x)`) el 2026-11-20.
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.1, "0.1"),
            (0.5, "0.5"),
            (1.5, "1.5"),
            (2.9, "2.9"),
            (0.30000000000000004, "0.30000000000000004"),
            (1234567.89, "1234567.89"),
            (0.000001, "0.000001"),
            (0.00001, "0.00001"),
            (1e-7, "1e-7"),
            (5e-324, "5e-324"),
            (1e15, "1000000000000000"),
            (1e16, "10000000000000000"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (9007199254740992.0, "9007199254740992"),
            (2147483648.0, "2147483648"),
            (1234567890123456800.0, "1234567890123456800"),
            (1.5e20, "150000000000000000000"),
            (-2.5, "-2.5"),
            (-7.5, "-7.5"),
            (100000.0, "100000"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ];
        for (x, want) in cases {
            assert_eq!(js_number_to_string(*x), *want, "x={x}");
        }
    }

    #[test]
    fn range_received_matches_node() {
        // Mensajes `Received` reales de `crypto.randomBytes` (Node 24).
        let cases: &[(f64, &str)] = &[
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
            (-1.0, "-1"),
            (-2.5, "-2.5"),
            (-7.5, "-7.5"),
            (2147483648.0, "2147483648"),
            (3000000000.0, "3000000000"),
            (4294967296.0, "4294967296"),
            (4294967297.0, "4_294_967_297"),
            (5000000000.0, "5_000_000_000"),
            (9999999999.0, "9_999_999_999"),
            (10000000000.0, "10_000_000_000"),
            (-10000000000.0, "-10_000_000_000"),
            (9007199254740992.0, "9_007_199_254_740_992"),
            (-9007199254740992.0, "-9_007_199_254_740_992"),
            (123456789012345.0, "123_456_789_012_345"),
            (1234567890123456800.0, "1_234_567_890_123_456_800"),
            (2147483648.5, "2147483648.5"),
            (1234567890123.5, "1234567890123.5"),
            (1e21, "1e_+21"),
            (1e22, "1e_+22"),
            (1.5e21, "1_.5e_+21"),
            (1.23e25, "1._23e_+25"),
            (-1e21, "-1e_+21"),
            (-1.5e21, "-1_.5e_+21"),
            (-1.23e25, "-1._23e_+25"),
            (1.7976931348623157e308, "1._797_693_134_862_315_7e+_308"),
        ];
        for (x, want) in cases {
            assert_eq!(range_received(*x), *want, "x={x}");
        }
    }

    #[test]
    fn to_string_coercion_matches_js() {
        // Coerción `String()` de JS, verificada contra los casos
        // grabados de `newId` (prefijo antes del `_` + 16 hex).
        assert_eq!(
            js_to_string(&JsValue::decode(&serde_json::json!({"a": {"b": "c"}}))),
            "[object Object]"
        );
        assert_eq!(
            js_to_string(&JsValue::decode(&serde_json::json!(["a", 1]))),
            "a,1"
        );
        assert_eq!(
            js_to_string(&JsValue::decode(&serde_json::json!([3, 1, 2]))),
            "3,1,2"
        );
        assert_eq!(js_to_string(&JsValue::decode(&serde_json::json!([]))), "");
        assert_eq!(js_to_string(&JsValue::Null), "null");
        assert_eq!(js_to_string(&JsValue::Undefined), "undefined");
        assert_eq!(js_to_string(&JsValue::Bool(true)), "true");
        assert_eq!(js_to_string(&JsValue::Num(0.0)), "0");
        assert_eq!(js_to_string(&JsValue::Num(1e21)), "1e+21");
        assert_eq!(
            js_to_string(&JsValue::decode(&serde_json::json!([null, "x", null]))),
            ",x,"
        );
    }

    #[test]
    fn invalid_arg_type_messages_match_node() {
        assert_eq!(
            invalid_arg_type("size", "number", &JsValue::Str("abc".into())),
            "The \"size\" argument must be of type number. Received type string ('abc')"
        );
        assert_eq!(
            invalid_arg_type("size", "number", &JsValue::Bool(true)),
            "The \"size\" argument must be of type number. Received type boolean (true)"
        );
        assert_eq!(
            invalid_arg_type("size", "number", &JsValue::Null),
            "The \"size\" argument must be of type number. Received null"
        );
        assert_eq!(
            invalid_arg_type("size", "number", &JsValue::Undefined),
            "The \"size\" argument must be of type number. Received undefined"
        );
        assert_eq!(
            invalid_arg_type(
                "path",
                "string or an instance of Buffer or URL",
                &JsValue::Num(0.0)
            ),
            "The \"path\" argument must be of type string or an instance of Buffer or URL. Received type number (0)"
        );
        assert_eq!(
            invalid_arg_type(
                "path",
                "string or an instance of Buffer or URL",
                &JsValue::Array(vec![])
            ),
            "The \"path\" argument must be of type string or an instance of Buffer or URL. Received an instance of Array"
        );
    }
}
