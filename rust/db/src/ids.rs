//! Identificadores y códigos aleatorios: port de `randomCode` y
//! `newId` de `server/db.js`.

use crate::NodeError;
use crate::js::{self, JsValue};

/// Alfabeto de `randomCode`: sin `0`/`O`/`1`/`I` para dictarlo fácil.
pub const RANDOM_CODE_CHARS: &str = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const RANDOM_CODE_LEN: usize = RANDOM_CODE_CHARS.len(); // 31

/// `randomCode(length = 6)`: `length` caracteres aleatorios del
/// alfabeto. `length` ausente o `undefined` → 6 (default de JS).
/// Fracciones se truncan hacia cero (`0.5` → 0, `1.5` → 1) como
/// `crypto.randomBytes`. Tipos no numéricos → `ERR_INVALID_ARG_TYPE`;
/// `NaN`, `±Infinity`, negativos o `> 2147483647` →
/// `ERR_OUT_OF_RANGE` con el mensaje exacto de Node.
pub fn random_code(length: Option<&JsValue>) -> Result<String, NodeError> {
    let n = match length {
        None | Some(JsValue::Undefined) => 6.0,
        Some(JsValue::Num(n)) => *n,
        Some(other) => {
            return Err(NodeError::Message(js::invalid_arg_type(
                "size", "number", other,
            )));
        }
    };
    if n.is_nan() || n.is_infinite() || n < 0.0 || n > 2147483647.0 {
        return Err(NodeError::Message(js::out_of_range_size(n)));
    }
    let len = n.trunc() as usize;
    let mut buf = vec![0u8; len];
    karaoke_crypto::fill_random(&mut buf)?;
    let alphabet = RANDOM_CODE_CHARS.as_bytes();
    Ok(buf
        .iter()
        .map(|b| alphabet[(b % RANDOM_CODE_LEN as u8) as usize] as char)
        .collect())
}

/// `newId(prefix)`: `"<prefix>_<16 hex>"` con 8 bytes aleatorios.
/// El prefijo se coerciona con `String()` de JS (`undefined` →
/// `"undefined"`, `{}` → `"[object Object]"`, `[3,1,2]` → `"3,1,2"`).
/// Solo falla si el RNG del SO falla (el original lanzaría igual).
pub fn new_id(prefix: &JsValue) -> Result<String, NodeError> {
    let mut buf = [0u8; 8];
    karaoke_crypto::fill_random(&mut buf)?;
    Ok(format!("{}_{}", js::js_to_string(prefix), hex::encode(buf)))
}

/// Atajo para prefijos que ya son `&str` (lo que usan `seed` y el
/// resto del servidor: `newId('u')`, `newId('room')`...).
pub fn new_id_str(prefix: &str) -> Result<String, NodeError> {
    new_id(&JsValue::Str(prefix.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_code_shape() {
        let code = random_code(None).unwrap();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| RANDOM_CODE_CHARS.contains(c)));
        let code = random_code(Some(&JsValue::Num(0.0))).unwrap();
        assert_eq!(code, "");
        let code = random_code(Some(&JsValue::Num(0.5))).unwrap();
        assert_eq!(code, "", "0.5 se trunca a 0 como randomBytes");
        let code = random_code(Some(&JsValue::Num(1.5))).unwrap();
        assert_eq!(code.len(), 1);
        let code = random_code(Some(&JsValue::Undefined)).unwrap();
        assert_eq!(code.len(), 6, "undefined toma el default 6");
    }

    #[test]
    fn random_code_errors_match_node() {
        let err = random_code(Some(&JsValue::Num(f64::NAN))).unwrap_err();
        assert_eq!(
            err.to_string(),
            "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received NaN"
        );
        let err = random_code(Some(&JsValue::Num(-1.0))).unwrap_err();
        assert_eq!(
            err.to_string(),
            "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received -1"
        );
        let err = random_code(Some(&JsValue::Num(1e21))).unwrap_err();
        assert_eq!(
            err.to_string(),
            "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received 1e_+21"
        );
        let err = random_code(Some(&JsValue::Str("abc".into()))).unwrap_err();
        assert_eq!(
            err.to_string(),
            "The \"size\" argument must be of type number. Received type string ('abc')"
        );
    }

    #[test]
    fn new_id_shape() {
        let id = new_id(&JsValue::Str("u".into())).unwrap();
        let (prefix, hex) = id.split_once('_').unwrap();
        assert_eq!(prefix, "u");
        assert_eq!(hex.len(), 16);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        let id = new_id(&JsValue::Undefined).unwrap();
        assert!(id.starts_with("undefined_"));
        let id = new_id(&JsValue::Array(vec![
            JsValue::Num(3.0),
            JsValue::Num(1.0),
            JsValue::Num(2.0),
        ]))
        .unwrap();
        assert!(id.starts_with("3,1,2_"));
    }
}
