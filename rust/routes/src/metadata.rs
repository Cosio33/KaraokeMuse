//! `fetchMetadata` de `server/index.js`: oEmbed de YouTube/Spotify.
//!
//! Tres resultados visibles, y una trampa: el `catch` del original
//! agrupa el fallo de transporte y el fallo de `r.json()` en el mismo
//! mensaje (`Sin red para leer metadatos…`). Aquí se preserva esa
//! agrupación (ver `rust/research/json-network.md` §5.4).

use serde_json::Value;

use crate::http_util::encode_uri_component;
use crate::json::js_truthy;

/// Lo que devuelve `fetchMetadata`: o el error en español, o los
/// metadatos (`platform`, `title`, `artist`, `thumbnail_url`). El
/// payload `Ok` va en `Box` para que el enum no tenga variantes de
/// tamaño muy distinto (clippy `large_enum_variant`).
pub enum Metadata {
    Error(String),
    Ok(Box<MetadataOk>),
}

#[derive(Debug, Clone)]
pub struct MetadataOk {
    pub platform: &'static str,
    pub title: Value,
    pub artist: Value,
    pub thumbnail_url: Value,
}

/// `j.title || ''` de JS: el valor si es truthy, `""` si no.
fn or_empty(v: Option<&Value>) -> Value {
    if js_truthy(v) {
        v.unwrap().clone()
    } else {
        Value::String(String::new())
    }
}

/// Plataforma de un enlace por su dominio. Ojo: `youtu.be` no
/// contiene la cadena `youtube`, así que hay que comprobarlo aparte;
/// sin eso, Detectar etiquetaba los enlaces cortos de YouTube como
/// Spotify y al agregarlos el servidor respondía
/// "url debe ser de Spotify" (ADR-024).
pub fn detect_platform(url: &str) -> &'static str {
    if url.contains("youtube") || url.contains("youtu.be") {
        "youtube"
    } else {
        "spotify"
    }
}

pub async fn fetch_metadata(client: &reqwest::Client, url: &str) -> Metadata {
    let endpoint = if url.contains("youtube.com") || url.contains("youtu.be") {
        format!(
            "https://www.youtube.com/oembed?url={}&format=json",
            encode_uri_component(url)
        )
    } else if url.contains("spotify.com") || url.contains("open.spotify.com") {
        format!(
            "https://open.spotify.com/oembed?url={}",
            encode_uri_component(url)
        )
    } else {
        return Metadata::Error(
            "Plataforma no soportada para metadatos automáticos. Escribe título y artista manualmente."
                .to_string(),
        );
    };
    let outcome: Result<Value, String> = async {
        let r = client
            .get(&endpoint)
            .header("user-agent", "KaraokeMuse/1.0")
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
            .map_err(|_| "transport".to_string())?;
        if !r.status().is_success() {
            return Err("not-ok".to_string());
        }
        r.json::<Value>().await.map_err(|_| "transport".to_string())
    }
    .await;
    match outcome {
        Err(kind) => {
            if kind == "not-ok" {
                Metadata::Error(
                    "No se pudieron leer los metadatos del enlace. Escribe título y artista manualmente."
                        .to_string(),
                )
            } else {
                Metadata::Error(
                    "Sin red para leer metadatos. Escribe título y artista manualmente."
                        .to_string(),
                )
            }
        }
        Ok(j) => Metadata::Ok(Box::new(MetadataOk {
            platform: detect_platform(url),
            title: or_empty(j.get("title")),
            artist: or_empty(j.get("author_name")),
            thumbnail_url: or_empty(j.get("thumbnail_url")),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_platform_youtu_be_is_youtube() {
        assert_eq!(detect_platform("https://youtu.be/elOen9IhAL4"), "youtube");
        assert_eq!(
            detect_platform(
                "https://www.youtube.com/watch?v=elOen9IhAL4&list=RDelOen9IhAL4&start_radio=1"
            ),
            "youtube"
        );
        assert_eq!(
            detect_platform("https://www.youtube.com/shorts/elOen9IhAL4"),
            "youtube"
        );
        assert_eq!(
            detect_platform("https://music.youtube.com/watch?v=elOen9IhAL4"),
            "youtube"
        );
    }

    #[test]
    fn detect_platform_spotify_links() {
        assert_eq!(
            detect_platform("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"),
            "spotify"
        );
        assert_eq!(
            detect_platform("https://open.spotify.com/intl-es/track/4uLU6hMCjMI75M1A2tKUQC"),
            "spotify"
        );
    }
}
