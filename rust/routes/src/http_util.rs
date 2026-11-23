//! Utilidades HTTP fieles al original de Node: lectura del cuerpo con
//! límite de 1 MiB, `URLSearchParams`, normalización del pathname al
//! estilo WHATWG URL y `encodeURIComponent`.

use axum::body::Body;
use serde_json::Value;

/// Límite del cuerpo como en `readBody` de Node (1024 * 1024).
pub const BODY_LIMIT: usize = 1024 * 1024;

/// `readBody(req)`: acumula hasta 1 MiB; cuerpo vacío → `{}`; JSON
/// inválido → error; cuerpo `null` → error (en JS, acceder a un campo
/// de `null` lanzaría `TypeError`, que el `catch` de `handle` convierte
/// en 500). El error se convierte en 500 en `dispatch`.
pub async fn read_body(body: Body) -> Result<Value, String> {
    let bytes = axum::body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|e| format!("readBody: {e}"))?;
    if bytes.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let v: Value =
        serde_json::from_slice(&bytes).map_err(|_| "readBody: JSON inválido".to_string())?;
    if v.is_null() {
        return Err("readBody: cuerpo null (acceso a campo lanzaría en JS)".to_string());
    }
    Ok(v)
}

/// Campo de un cuerpo JSON: `None` si el cuerpo no es un objeto o si
/// el campo no existe (como `undefined` en JS).
pub fn field<'a>(body: &'a Value, key: &str) -> Option<&'a Value> {
    body.as_object().and_then(|m| m.get(key))
}

/// ¿El cuerpo trae la clave (aunque sea `null`)? En JS, `b[f]` con
/// `null` explícito NO es `undefined`: en el PATCH sí se procesa.
pub fn has_field(body: &Value, key: &str) -> bool {
    body.as_object()
        .map(|m| m.contains_key(key))
        .unwrap_or(false)
}

/// `url.searchParams.get(name)` de WHATWG URL: primer valor, con `+`
/// como espacio y `%XX` decodificado (las secuencias inválidas quedan
/// literales; los bytes no UTF-8 se reemplazan, como en el estándar).
pub fn query_param(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        if percent_decode_form(k) == name {
            return Some(percent_decode_form(v));
        }
    }
    None
}

fn hex_val(b: Option<&u8>) -> Option<u8> {
    b.and_then(|c| (*c as char).to_digit(16).map(|d| d as u8))
}

fn percent_decode_form(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                match (hex_val(bytes.get(i + 1)), hex_val(bytes.get(i + 2))) {
                    (Some(h), Some(l)) => {
                        out.push(h * 16 + l);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `encodeURIComponent` de JS: escapa todo salvo `A-Z a-z 0-9 - _ . !
/// ~ * ' ( )`. Se usa para construir las URLs oEmbed.
pub fn encode_uri_component(s: &str) -> String {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    const KEEP: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    utf8_percent_encode(s, KEEP).to_string()
}

fn is_single_dot(seg: &str) -> bool {
    seg.eq_ignore_ascii_case(".") || seg.eq_ignore_ascii_case("%2e")
}

fn is_double_dot(seg: &str) -> bool {
    seg.eq_ignore_ascii_case("..")
        || seg.eq_ignore_ascii_case(".%2e")
        || seg.eq_ignore_ascii_case("%2e.")
        || seg.eq_ignore_ascii_case("%2e%2e")
}

/// Normalización del pathname como `new URL(req.url, base).pathname`
/// (WHATWG URL, esquema especial `http:`): `\` cuenta como `/`, los
/// segmentos `.`/`%2e` se eliminan y `..` (y sus formas con `%2e`)
/// suben un nivel sin pasar de la raíz. El percent-encoding se
/// conserva (el pathname de Node NO decodifica `%C3%A9` a `é`).
pub fn normalize_pathname(raw: &str) -> String {
    let replaced = raw.replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = replaced.split('/').collect();
    let mut trailing_slash = false;
    for (i, seg) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        if i == 0 {
            continue; // el "" antes de la primera '/'
        }
        if is_single_dot(seg) {
            if last {
                trailing_slash = true;
            }
        } else if is_double_dot(seg) {
            out.pop();
            if last {
                trailing_slash = true;
            }
        } else {
            out.push(seg);
            trailing_slash = last && seg.is_empty();
        }
    }
    let mut path = String::from("/");
    path.push_str(&out.join("/"));
    if trailing_slash && !path.ends_with('/') {
        path.push('/');
    }
    path
}

/// `String.slice(0, n)` de JS sobre unidades UTF-16. Si el corte
/// parte un par sustituto, Rust lo reemplaza por U+FFFD (JS dejaría
/// el sustituto suelto; solo afecta a nombres con astrales justo en
/// el corte de 30).
pub fn js_slice_utf16(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().take(n).collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pathname_normalization() {
        assert_eq!(normalize_pathname("/api/songs"), "/api/songs");
        assert_eq!(normalize_pathname("/api/./songs"), "/api/songs");
        assert_eq!(normalize_pathname("/api/%2e/songs"), "/api/songs");
        assert_eq!(normalize_pathname("/api/x/../songs"), "/api/songs");
        assert_eq!(normalize_pathname("/api/%2e%2e/songs"), "/songs");
        assert_eq!(normalize_pathname("/../etc/passwd"), "/etc/passwd");
        assert_eq!(normalize_pathname("/api//login"), "/api//login");
        assert_eq!(normalize_pathname("/api/songs/"), "/api/songs/");
        assert_eq!(normalize_pathname("/api/h%C3%A9llo"), "/api/h%C3%A9llo");
        assert_eq!(normalize_pathname("/a/b/."), "/a/b/");
        assert_eq!(normalize_pathname("/a/b/.."), "/a/");
    }

    #[test]
    fn query_params() {
        assert_eq!(
            query_param("q=a+b&minRating=5", "q"),
            Some("a b".to_string())
        );
        assert_eq!(query_param("q=%C3%A9&x=1", "q"), Some("é".to_string()));
        assert_eq!(query_param("a=1&a=2", "a"), Some("1".to_string()));
        assert_eq!(query_param("", "q"), None);
        assert_eq!(query_param("flag", "flag"), Some(String::new()));
    }

    #[test]
    fn encode_component() {
        assert_eq!(
            encode_uri_component("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            "https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3DdQw4w9WgXcQ"
        );
        assert_eq!(encode_uri_component("a b!~*'()"), "a%20b!~*'()");
    }
}
