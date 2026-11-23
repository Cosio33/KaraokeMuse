//! Primitivas criptográficas puras de KaraokeMuse (sin BD).
//!
//! Port fiel de las funciones puras de `server/auth.js` (`hashPassword`,
//! `verifyPassword`, `tokenHash`). Parámetros fijos por ADR-003 y
//! ADR-012: scrypt N=2^14, r=8, p=1, salida de 64 bytes — los mismos
//! defaults de Node `crypto.scryptSync`.
//!
//! NO usar `scrypt::Params::RECOMMENDED` (log_n=17, salida 32): daría
//! un hash distinto y todas las contraseñas guardadas dejarían de
//! verificar (ver `rust/research/storage-auth.md` §3.3).

use scrypt::{Params, scrypt as scrypt_kdf};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Error de una primitiva criptográfica. El original JS lanzaría una
/// excepción en estos casos; aquí se devuelve `Result`.
#[derive(Debug)]
pub enum CryptoError {
    Scrypt(scrypt::errors::InvalidOutputLen),
    BadHex(hex::FromHexError),
    Rng(getrandom::Error),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::Scrypt(e) => write!(f, "scrypt failed: {e}"),
            CryptoError::BadHex(e) => write!(f, "invalid hex: {e}"),
            CryptoError::Rng(e) => write!(f, "random bytes failed: {e}"),
        }
    }
}

impl std::error::Error for CryptoError {}

/// Llena `buf` con bytes aleatorios del SO (lo que Node hace con
/// `crypto.randomBytes`). En rand 0.10 `OsRng` ya no existe (ahora es
/// `SysRng` vía getrandom), así que se usa `getrandom` directamente.
pub fn fill_random(buf: &mut [u8]) -> Result<(), CryptoError> {
    getrandom::fill(buf).map_err(CryptoError::Rng)
}

/// Parámetros scrypt idénticos a los defaults de Node: log_n=14
/// (N=16384), r=8, p=1. La longitud de salida la fija el buffer (64).
pub fn scrypt_params() -> Params {
    // INFALLIBLE: los parámetros están hardcodeados y son válidos.
    Params::new(14, 8, 1).expect("parámetros scrypt válidos")
}

/// Genera una sal aleatoria de 16 bytes y el hash scrypt de 64 bytes.
/// Devuelve `(salt_hex, hash_hex)`, igual que `hashPassword` de auth.js.
///
/// OJO (ADR-015): Node pasa la sal hex COMO CADENA a `scryptSync`, así
/// que la sal del KDF son los bytes UTF-8 de la cadena hex (32 bytes),
/// no los 16 bytes crudos. `hash_password` y `verify_password` usan
/// esa misma convención, o las contraseñas creadas por Node no
/// verificarían en Rust (y al revés).
pub fn hash_password(password: &str) -> Result<(String, String), CryptoError> {
    let mut salt = [0u8; 16];
    fill_random(&mut salt)?;
    let salt_hex = hex::encode(salt);
    let mut out = [0u8; 64];
    scrypt_kdf(
        password.as_bytes(),
        salt_hex.as_bytes(),
        &scrypt_params(),
        &mut out,
    )
    .map_err(CryptoError::Scrypt)?;
    Ok((salt_hex, hex::encode(out)))
}

/// Verifica una contraseña contra `(salt_hex, hash_hex)` guardados.
/// Devuelve `false` (nunca error) si la sal o el hash son inválidos,
/// vacíos o de longitud incorrecta — igual que el `try/catch` de
/// `verifyPassword` en auth.js. La comparación es en tiempo constante.
///
/// La sal es la cadena hex tal cual (ADR-015): Node acepta cualquier
/// cadena no vacía como sal, sin validar que sea hex. El hash sí debe
/// decodificar a 64 bytes, o `timingSafeEqual` lanzaría en Node.
pub fn verify_password(password: &str, salt_hex: &str, hash_hex: &str) -> bool {
    if salt_hex.is_empty() || hash_hex.is_empty() {
        return false;
    }
    let Ok(stored) = hex::decode(hash_hex) else {
        return false;
    };
    if stored.len() != 64 {
        return false;
    }
    let mut candidate = [0u8; 64];
    if scrypt_kdf(
        password.as_bytes(),
        salt_hex.as_bytes(),
        &scrypt_params(),
        &mut candidate,
    )
    .is_err()
    {
        return false;
    }
    // Ambos lados tienen exactamente 64 bytes: `ct_eq` no filtra longitud.
    bool::from(candidate.ct_eq(stored.as_slice()))
}

/// sha256 hex del token. OJO: Node hashea la cadena hex de 64
/// caracteres del token, no los 32 bytes crudos — igual que `tokenHash`
/// en auth.js (`crypto.createHash('sha256').update(token)`).
pub fn token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectores del KDF scrypt en crudo (sal en bytes, no la cadena
    // hex): verifican que el KDF coincide con Node, no la convención
    // de `hashPassword`/`verifyPassword` (ver ADR-015).
    #[test]
    fn scrypt_matches_node_vector_1() {
        let mut out = [0u8; 64];
        let salt = hex::decode("000102030405060708090a0b0c0d0e0f").unwrap();
        scrypt_kdf(b"AdminTest123!", &salt, &scrypt_params(), &mut out).unwrap();
        assert_eq!(
            hex::encode(out),
            "a334b3f26652b2a71e39cf9c8d4331c78bf3b357faf3b583edcbfcf0dad82580d09d15f13f27e7df2afc8bfc0e446689bdf9243c9c5ecc8a054c8552c1655d7a"
        );
    }

    #[test]
    fn scrypt_matches_node_vector_2() {
        let mut out = [0u8; 64];
        let salt = hex::decode("aabbccddeeff00112233445566778899").unwrap();
        scrypt_kdf(b"test", &salt, &scrypt_params(), &mut out).unwrap();
        assert_eq!(
            hex::encode(out),
            "6f0d4c8677168d4eecb23a89c40768629a54f0cce0d9fafe92ccc0b192389b3eadbcd3b23556ae85a61cf9f3a67b182b475c3016701dbc0628f0b4c7d7c90e6b"
        );
    }

    #[test]
    fn token_hash_matches_node() {
        assert_eq!(
            token_hash(&"ab".repeat(32)),
            "271a413bd339c5709fdceaec41f14f11e9fbfb5042d72d331c65f32b284cd09a"
        );
    }

    /// Vector de `verifyPassword` con la convención REAL de Node
    /// (ADR-015): `crypto.scryptSync('Secreta123!', saltHex, 64)` con
    /// la sal hex COMO CADENA. Generado con Node 2026-11-20; si falla,
    /// las contraseñas creadas por el original no verificarían (fue
    /// el bug de `login-good` en el replay del front door).
    #[test]
    fn verify_password_matches_node_string_salt() {
        let salt_hex = "000102030405060708090a0b0c0d0e0f";
        let hash_hex = "0c5f3622d1c4d89e02d071d17f906b401efc3dee9758e1cfb6d548430becb31ea0e6c28c09996b51f959e031851b0fefe7faaea7a4c4384614526a58876f5439";
        assert!(verify_password("Secreta123!", salt_hex, hash_hex));
        assert!(!verify_password("secreta123!", salt_hex, hash_hex));
    }

    #[test]
    fn hash_and_verify_roundtrip() {
        let (salt, hash) = hash_password("Secreta123!").unwrap();
        assert_eq!(salt.len(), 32, "sal: 16 bytes en hex");
        assert_eq!(hash.len(), 128, "hash: 64 bytes en hex");
        assert!(verify_password("Secreta123!", &salt, &hash));
        assert!(!verify_password("secreta123!", &salt, &hash));
        assert!(!verify_password("Secreta123!", &salt, &hash[..120]));
        assert!(!verify_password("Secreta123!", "", &hash));
        assert!(!verify_password("Secreta123!", &salt, ""));
        assert!(!verify_password("Secreta123!", "zz", &hash));
    }
}
