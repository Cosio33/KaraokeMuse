//! `validateSongInput` de `server/index.js`, tal cual.

use serde_json::Value;

use crate::json::js_truthy;

const DIFFICULTIES: [&str; 3] = ["facil", "media", "dificil"];
const STATUSES: [&str; 3] = ["want", "practicing", "mastered"];
const PLATFORMS: [&str; 3] = ["youtube", "spotify", "local"];

/// Devuelve la lista de errores en el mismo orden que el original.
/// `b` es el cuerpo ya parseado (`{}` si venía vacío).
pub fn validate_song_input(b: &Value, is_guest: bool) -> Vec<String> {
    let mut errors = Vec::new();
    let title = b.get("title");
    let title_ok = matches!(title, Some(Value::String(s)) if !crate::json::js_trim(s).is_empty());
    if !title_ok {
        errors.push("title es obligatorio".to_string());
    }
    let platform_field = b.get("platform");
    if js_truthy(platform_field)
        && !matches!(platform_field, Some(Value::String(s)) if PLATFORMS.contains(&s.as_str()))
    {
        errors.push("platform inválida".to_string());
    }
    let platform = match platform_field {
        Some(Value::String(s)) => s.as_str(),
        _ => "youtube",
    };
    if is_guest && platform == "local" {
        errors.push(
            "Los invitados no pueden registrar vídeos locales; solo enlaces de YouTube o Spotify"
                .to_string(),
        );
    }
    if platform != "local" {
        let url = b.get("url");
        if !matches!(url, Some(Value::String(s)) if !s.is_empty()) {
            errors.push("url es obligatoria para enlaces".to_string());
        } else {
            let u = url.unwrap().as_str().unwrap();
            if platform == "youtube" && !u.contains("youtube.com") && !u.contains("youtu.be") {
                errors.push("url debe ser de YouTube".to_string());
            } else if platform == "spotify"
                && !u.contains("spotify.com")
                && !u.contains("open.spotify.com")
            {
                errors.push("url debe ser de Spotify".to_string());
            }
        }
    } else {
        let lp = b.get("local_path");
        if !matches!(lp, Some(Value::String(s)) if !s.is_empty()) {
            errors.push("local_path es obligatorio para vídeos locales".to_string());
        }
    }
    let difficulty = b.get("difficulty");
    if js_truthy(difficulty)
        && !matches!(difficulty, Some(Value::String(s)) if DIFFICULTIES.contains(&s.as_str()))
    {
        errors.push("difficulty inválida".to_string());
    }
    let status = b.get("status");
    if js_truthy(status)
        && !matches!(status, Some(Value::String(s)) if STATUSES.contains(&s.as_str()))
    {
        errors.push("status inválido".to_string());
    }
    if let Some(rating) = b.get("rating") {
        let ok = match rating {
            Value::Number(n) => {
                let f = n.as_f64().unwrap_or(f64::NAN);
                f.fract() == 0.0 && (0.0..=5.0).contains(&f)
            }
            _ => false,
        };
        if !ok {
            errors.push("rating debe ser entero de 0 a 5".to_string());
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_cannot_register_local() {
        let b = serde_json::json!({"title": "X", "platform": "local", "local_path": "/tmp/x.mp4"});
        let e = validate_song_input(&b, true);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("invitados"));
    }

    #[test]
    fn bad_platform_and_rating() {
        let b = serde_json::json!({"platform": "vimeo", "rating": 6});
        let e = validate_song_input(&b, false);
        assert!(e.contains(&"title es obligatorio".to_string()));
        assert!(e.contains(&"platform inválida".to_string()));
        assert!(e.contains(&"url es obligatoria para enlaces".to_string()));
        assert!(e.contains(&"rating debe ser entero de 0 a 5".to_string()));
    }
}
