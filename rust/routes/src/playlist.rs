//! Importación de listas completas de YouTube (ADR-025, solo admin).
//!
//! YouTube no tiene oEmbed para listas, así que la lista se lee de la
//! página pública `youtube.com/playlist?list=…`: el `ytInitialData`
//! trae los vídeos (título, canal e id). La página actual usa
//! `lockupViewModel` por vídeo; la vieja usaba `playlistVideoRenderer`
//! y se sigue aceptando. Las mezclas automáticas (`list=RD…`) no se
//! pueden importar: su página no trae la lista de vídeos.
//!
//! Todo el parseo es puro (sin red) para poder probarlo con una
//! fixture; la red vive solo en `fetch_playlist`.

use serde_json::Value;

use crate::http_util::encode_uri_component;

/// Un vídeo de la lista, listo para registrarse como canción.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistVideo {
    pub video_id: String,
    pub title: String,
    pub artist: String,
}

/// Una lista leída: título y vídeos (como mucho 100).
#[derive(Debug, Clone)]
pub struct Playlist {
    pub title: String,
    pub videos: Vec<PlaylistVideo>,
}

/// `true` si la URL es la página de una lista de YouTube
/// (`youtube.com/playlist?list=…` o `music.youtube.com/playlist?`).
/// Un `watch?v=…&list=…` es un solo vídeo con lista, no una lista.
pub fn is_playlist_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    (lower.contains("youtube.com/playlist?") || lower.contains("music.youtube.com/playlist?"))
        && lower.contains("list=")
}

/// El valor del parámetro `list=` de la URL, si lo hay.
pub fn playlist_id_from_url(url: &str) -> Option<String> {
    let lower = url.to_lowercase();
    let start = lower.find("list=")? + "list=".len();
    let rest = &url[start..];
    let end = rest.find(['&', '#', '?']).unwrap_or(rest.len());
    let id = &rest[..end];
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Texto de un campo de YouTube, que viene como cadena, como
/// `{"content": "…"}` o como `{"runs": [{"text": "…"}]}`.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(_) => {
            if let Some(Value::String(s)) = v.get("content") {
                return s.clone();
            }
            if let Some(Value::String(s)) = v.get("simpleText") {
                return s.clone();
            }
            if let Some(Value::Array(runs)) = v.get("runs") {
                return runs
                    .iter()
                    .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("");
            }
            String::new()
        }
        _ => String::new(),
    }
}

/// Extrae el JSON de `var ytInitialData = {...};` del HTML, con conteo
/// de llaves que respeta las cadenas (un título puede tener `{` o `}`).
fn extract_initial_data(html: &str) -> Option<String> {
    let marker = "var ytInitialData = ";
    let i = html.find(marker)?;
    let rest = &html[i + marker.len()..];
    let start = rest.find('{')?;
    let bytes = rest.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut k = start;
    while k < bytes.len() {
        let c = bytes[k] as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(rest[start..=k].to_string());
            }
        }
        k += 1;
    }
    None
}

/// Vídeos de un `lockupViewModel` (página actual) o de un
/// `playlistVideoRenderer` (página vieja). Devuelve `None` si el
/// elemento no es un vídeo con título.
fn video_from_item(item: &Value) -> Option<PlaylistVideo> {
    if let Some(l) = item.get("lockupViewModel") {
        let video_id = l.get("contentId")?.as_str()?.to_string();
        let meta = l.get("metadata")?.get("lockupMetadataViewModel")?;
        let title = text_of(meta.get("title")?);
        let artist = meta
            .get("metadata")
            .and_then(|m| m.get("contentMetadataViewModel"))
            .and_then(|m| m.get("metadataRows"))
            .and_then(|r| r.as_array())
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("metadataParts"))
            .and_then(|p| p.as_array())
            .and_then(|parts| parts.first())
            .and_then(|part| part.get("text"))
            .map(text_of)
            .unwrap_or_default();
        if video_id.is_empty() || title.is_empty() {
            return None;
        }
        return Some(PlaylistVideo {
            video_id,
            title,
            artist,
        });
    }
    if let Some(p) = item.get("playlistVideoRenderer") {
        let video_id = p.get("videoId")?.as_str()?.to_string();
        let title = p.get("title").map(text_of).unwrap_or_default();
        let artist = p.get("shortBylineText").map(text_of).unwrap_or_default();
        if video_id.is_empty() || title.is_empty() {
            return None;
        }
        return Some(PlaylistVideo {
            video_id,
            title,
            artist,
        });
    }
    None
}

/// Parsea la página de una lista. Como mucho 100 vídeos, sin
/// duplicados (la página repite el vídeo que suena).
pub fn parse_playlist_page(html: &str) -> Result<Playlist, String> {
    let no_list = "No se pudo leer los vídeos de la lista. Revisa que sea una lista pública de YouTube (las mezclas automáticas no se pueden importar).".to_string();
    let json = extract_initial_data(html).ok_or_else(|| no_list.clone())?;
    let data: Value = serde_json::from_str(&json).map_err(|_| no_list.clone())?;
    let title = data
        .get("metadata")
        .and_then(|m| m.get("playlistMetadataRenderer"))
        .and_then(|r| r.get("title"))
        .map(text_of)
        .unwrap_or_default();
    let mut videos: Vec<PlaylistVideo> = Vec::new();
    let tabs = data
        .get("contents")
        .and_then(|c| c.get("twoColumnBrowseResultsRenderer"))
        .and_then(|r| r.get("tabs"))
        .and_then(|t| t.as_array());
    if let Some(tabs) = tabs {
        for tab in tabs {
            let sections = tab
                .get("tabRenderer")
                .and_then(|t| t.get("content"))
                .and_then(|c| c.get("sectionListRenderer"))
                .and_then(|s| s.get("contents"))
                .and_then(|c| c.as_array());
            let Some(sections) = sections else {
                continue;
            };
            for section in sections {
                let items = section
                    .get("itemSectionRenderer")
                    .and_then(|s| s.get("contents"))
                    .and_then(|c| c.as_array());
                let Some(items) = items else {
                    continue;
                };
                for item in items {
                    if videos.len() >= 100 {
                        break;
                    }
                    if let Some(v) = video_from_item(item)
                        && !videos.iter().any(|x| x.video_id == v.video_id)
                    {
                        videos.push(v);
                    }
                }
            }
        }
    }
    if videos.is_empty() {
        return Err(no_list);
    }
    Ok(Playlist { title, videos })
}

/// Lee una lista de YouTube por su id (`PL…`, `UU…`, …).
pub async fn fetch_playlist(
    client: &reqwest::Client,
    playlist_id: &str,
) -> Result<Playlist, String> {
    let url = format!(
        "https://www.youtube.com/playlist?list={}",
        encode_uri_component(playlist_id)
    );
    let r = client
        .get(&url)
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .header("accept-language", "es-ES,es;q=0.9")
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
        .map_err(|_| "Sin red para leer la lista. Revisa la conexión del servidor.".to_string())?;
    if !r.status().is_success() {
        return Err(
            "No se pudo leer la lista. Revisa que sea una lista pública de YouTube.".to_string(),
        );
    }
    let html = r.text().await.map_err(|_| {
        "No se pudo leer la lista. Revisa que sea una lista pública de YouTube.".to_string()
    })?;
    parse_playlist_page(&html)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixture mínima: un lockupViewModel (página actual), un
    // playlistVideoRenderer (página vieja) y un duplicado.
    const FIXTURE: &str = r#"<html><head></head><body><script>var ytInitialData = {"metadata": {"playlistMetadataRenderer": {"title": "Karaoke noche"}}, "contents": {"twoColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [{"itemSectionRenderer": {"contents": [{"lockupViewModel": {"contentId": "AAA111AAA11", "metadata": {"lockupMetadataViewModel": {"title": {"content": "Cielito {lindo}"}, "metadata": {"contentMetadataViewModel": {"metadataRows": [{"metadataParts": [{"text": {"content": "Tradicional"}}]}]}}}}}}, {"lockupViewModel": {"contentId": "AAA111AAA11", "metadata": {"lockupMetadataViewModel": {"title": {"content": "Cielito {lindo}"}, "metadata": {"contentMetadataViewModel": {"metadataRows": [{"metadataParts": [{"text": {"content": "Tradicional"}}]}]}}}}}}, {"playlistVideoRenderer": {"videoId": "BBB222BBB22", "title": {"runs": [{"text": "La Bamba"}]}, "shortBylineText": {"runs": [{"text": "Ritchie Valens"}]}}}]}}]}}}}]}}};</script></body></html>"#;

    #[test]
    fn parse_fixture_lockup_and_old_renderer() {
        let p = parse_playlist_page(FIXTURE).unwrap();
        assert_eq!(p.title, "Karaoke noche");
        // El duplicado (mismo videoId) se quita; el título con llaves
        // no rompe el conteo de llaves del extractor.
        assert_eq!(p.videos.len(), 2);
        assert_eq!(p.videos[0].video_id, "AAA111AAA11");
        assert_eq!(p.videos[0].title, "Cielito {lindo}");
        assert_eq!(p.videos[0].artist, "Tradicional");
        assert_eq!(p.videos[1].video_id, "BBB222BBB22");
        assert_eq!(p.videos[1].title, "La Bamba");
        assert_eq!(p.videos[1].artist, "Ritchie Valens");
    }

    #[test]
    fn parse_page_without_list_errors() {
        assert!(parse_playlist_page("<html>sin datos</html>").is_err());
    }

    #[test]
    fn playlist_url_detection() {
        assert!(is_playlist_url(
            "https://www.youtube.com/playlist?list=PLusOJN4fPK_u0dMxl_xRCTNT1GKaM6vtU"
        ));
        assert!(is_playlist_url(
            "https://music.youtube.com/playlist?list=PL123"
        ));
        assert!(!is_playlist_url(
            "https://www.youtube.com/watch?v=elOen9IhAL4&list=RDelOen9IhAL4&start_radio=1"
        ));
        assert!(!is_playlist_url("https://youtu.be/elOen9IhAL4"));
        assert_eq!(
            playlist_id_from_url("https://www.youtube.com/playlist?list=PLabc123&si=x"),
            Some("PLabc123".to_string())
        );
        assert_eq!(playlist_id_from_url("https://youtu.be/elOen9IhAL4"), None);
    }
}
