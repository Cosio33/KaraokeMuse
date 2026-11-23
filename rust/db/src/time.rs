//! Fechas ISO 8601 UTC con milisegundos, como `new Date().toISOString()`
//! de JS (`"2026-11-20T15:30:00.123Z"`). Sin dependencias: la fecha
//! civil se calcula desde los días desde la época (algoritmo de
//! Howard Hinnant).

use std::time::{SystemTime, UNIX_EPOCH};

/// Fecha/hora actual en formato ISO 8601 UTC con milisegundos y `Z`.
pub fn now_iso() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_iso(now.as_secs() as i64, now.subsec_millis())
}

/// Fecha/hora actual + `days` días, en el mismo formato. Lo usa
/// `createSession` (auth) para `expires_at`.
pub fn now_plus_days_iso(days: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64 + days * 24 * 60 * 60;
    format_iso(secs, now.subsec_millis())
}

/// Milisegundos actuales desde la época (para comparar expiraciones:
/// `new Date(expires_at) < new Date()` en JS).
pub fn now_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn format_iso(secs: i64, millis: u32) -> String {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Días desde 1970-01-01 → (año, mes, día). Algoritmo de Hinnant
/// (dominio público), válido para todo el rango de `i64` práctico.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Parsea un ISO 8601 UTC con la forma exacta de `toISOString()`
/// (`YYYY-MM-DDTHH:MM:SS[.mmm]Z`) a milisegundos desde la época.
/// Devuelve `None` si no parsea — el original JS daría `Invalid Date`
/// (`NaN`), y las comparaciones con `NaN` son siempre `false`.
pub fn iso_to_unix_millis(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |a: usize, c: usize| -> Option<i64> {
        std::str::from_utf8(&b[a..c]).ok()?.parse::<i64>().ok()
    };
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let min = num(14, 16)?;
    let sec = num(17, 19)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    // Milisegundos opcionales: ".123" antes de la Z final.
    let mut millis: i64 = 0;
    let mut rest = &b[19..];
    if !rest.is_empty() && rest[0] == b'.' {
        rest = &rest[1..];
        let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        let frac = std::str::from_utf8(&rest[..digits]).ok()?;
        let mut v: i64 = frac.parse().ok()?;
        for _ in digits..3 {
            v *= 10;
        }
        // Más de 3 dígitos: se truncan (JS también los trunca al ms).
        for _ in 3..digits {
            v /= 10;
        }
        millis = v;
        rest = &rest[digits..];
    }
    if rest != b"Z" {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(((days * 86_400 + hour * 3600 + min * 60 + sec) * 1000) + millis)
}

/// (año, mes, día) → días desde 1970-01-01. Inversa de
/// `civil_from_days` (Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip() {
        for (y, m, d) in [(1970, 1, 1), (2026, 11, 20), (2000, 2, 29), (1999, 12, 31)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m as u32, d as u32));
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn iso_parse_matches_js_dates() {
        // `Date.parse("2026-11-20T15:30:00.123Z")` en Node
        // (verificado con `date -d '2026-11-20 15:30:00 UTC' +%s`).
        assert_eq!(
            iso_to_unix_millis("2026-11-20T15:30:00.123Z"),
            Some(1_795_188_600_123)
        );
        assert_eq!(iso_to_unix_millis("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(
            iso_to_unix_millis("2026-11-20T15:30:00Z"),
            Some(1_795_188_600_000)
        );
        assert_eq!(iso_to_unix_millis("no-fecha"), None);
        assert_eq!(iso_to_unix_millis("2026-13-01T00:00:00Z"), None);
        assert_eq!(iso_to_unix_millis("2026-11-20 15:30:00Z"), None);
    }

    #[test]
    fn now_iso_shape() {
        let s = now_iso();
        assert!(s.ends_with('Z'), "{s}");
        assert_eq!(s.len(), 24, "{s}");
        assert!(iso_to_unix_millis(&s).is_some(), "{s}");
    }
}
