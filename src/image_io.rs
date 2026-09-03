use crate::grok::{GrokImageRef, MAX_EDIT_IMAGES};
use base64::Engine;
use tracing::info;

/// Detect MIME type from base64-encoded image data by inspecting magic bytes.
pub fn detect_mime_type(b64: &str) -> &'static str {
    if b64.starts_with("iVBOR") {
        "image/png"
    } else if b64.starts_with("/9j/") {
        "image/jpeg"
    } else if b64.starts_with("R0lGOD") {
        "image/gif"
    } else if b64.starts_with("UklGR") {
        "image/webp"
    } else {
        "image/png"
    }
}

pub fn mime_from_magic(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF") {
        Some("image/gif")
    } else if data.len() >= 12 && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

pub fn image_ext_from_bytes(data: &[u8]) -> &'static str {
    match mime_from_magic(data) {
        Some("image/png") => "png",
        Some("image/jpeg") => "jpg",
        Some("image/gif") => "gif",
        Some("image/webp") => "webp",
        _ => "png",
    }
}

/// Normalize a user-supplied image source to a value the API accepts.
/// HTTP(S) URLs and `data:` URIs pass through; anything else is treated as a local file path.
pub fn resolve_image_source(src: &str) -> Result<String, String> {
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
        Ok(src.to_string())
    } else {
        info!(
            path = src,
            "Encoding local file as data URI for image source"
        );
        local_file_to_data_uri(src)
    }
}

fn local_file_to_data_uri(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("Failed to read file {path}: {e}"))?;
    let mime = mime_from_path_or_bytes(path, &data);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    info!(path, mime, "Encoded local file as data URI");
    Ok(format!("data:{mime};base64,{b64}"))
}

fn mime_from_path_or_bytes(path: &str, data: &[u8]) -> &'static str {
    if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".gif") {
        "image/gif"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else {
        mime_from_magic(data).unwrap_or("application/octet-stream")
    }
}

pub fn bytes_to_data_uri(data: &[u8], mime: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    format!("data:{mime};base64,{b64}")
}

/// Decode a `data:` URI into raw bytes.
pub fn decode_data_uri(src: &str) -> Result<Vec<u8>, String> {
    let rest = src
        .strip_prefix("data:")
        .ok_or_else(|| "Not a data URI".to_string())?;
    let b64 = if let Some(idx) = rest.find(',') {
        let meta = &rest[..idx];
        let payload = &rest[idx + 1..];
        if !meta.contains(";base64") {
            return Err("Only base64 data URIs are supported".to_string());
        }
        payload
    } else {
        return Err("Malformed data URI (missing comma)".to_string());
    };
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("Failed to decode data URI: {e}"))
}

pub fn resolve_edit_image_fields(
    image_url: Option<String>,
    images: Option<Vec<String>>,
) -> Result<(Option<GrokImageRef>, Option<Vec<GrokImageRef>>), String> {
    match (image_url, images) {
        (Some(_), Some(_)) => Err("Provide either image_url or images, not both.".into()),
        (None, None) => Err("Provide either image_url or images.".into()),
        (Some(src), None) => {
            let url = resolve_image_source(&src)?;
            Ok((Some(GrokImageRef { url }), None))
        }
        (None, Some(list)) => {
            if list.is_empty() {
                return Err("images must contain at least one source image.".into());
            }
            if list.len() > MAX_EDIT_IMAGES {
                return Err(format!(
                    "images supports at most {MAX_EDIT_IMAGES} entries, got {}.",
                    list.len()
                ));
            }
            let mut refs = Vec::with_capacity(list.len());
            for src in list {
                refs.push(GrokImageRef {
                    url: resolve_image_source(&src)?,
                });
            }
            Ok((None, Some(refs)))
        }
    }
}

pub async fn load_image_bytes(http: &reqwest::Client, src: &str) -> Result<Vec<u8>, String> {
    if src.starts_with("data:") {
        decode_data_uri(src)
    } else if src.starts_with("http://") || src.starts_with("https://") {
        info!(url = src, "Downloading image for headshot pad");
        let resp = http
            .get(src)
            .send()
            .await
            .map_err(|e| format!("Failed to download image: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!(
                "Failed to download image (HTTP {}): {src}",
                resp.status()
            ));
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read downloaded image body: {e}"))
    } else {
        std::fs::read(src).map_err(|e| format!("Failed to read file {src}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_data_uris_pass_through() {
        assert_eq!(
            resolve_image_source("https://example.com/a.png").unwrap(),
            "https://example.com/a.png"
        );
        assert_eq!(
            resolve_image_source("data:image/png;base64,abc").unwrap(),
            "data:image/png;base64,abc"
        );
    }

    #[test]
    fn magic_bytes_to_ext() {
        assert_eq!(image_ext_from_bytes(&[0x89, 0x50, 0x4E, 0x47]), "png");
        assert_eq!(image_ext_from_bytes(&[0xFF, 0xD8, 0xFF, 0x00]), "jpg");
        assert_eq!(image_ext_from_bytes(b"GIF89a"), "gif");
        assert_eq!(image_ext_from_bytes(b"not-an-image"), "png");
    }

    #[test]
    fn edit_sources_are_mutually_exclusive() {
        let err =
            resolve_edit_image_fields(Some("https://a".into()), Some(vec!["https://b".into()]))
                .unwrap_err();
        assert!(err.contains("not both"));
        assert!(resolve_edit_image_fields(None, None).is_err());
        assert!(resolve_edit_image_fields(None, Some(vec![])).is_err());
        let too_many = vec!["https://x".into(); 6];
        let err = resolve_edit_image_fields(None, Some(too_many)).unwrap_err();
        assert!(err.contains("at most 5"));
    }

    #[test]
    fn b64_prefix_mime() {
        assert_eq!(detect_mime_type("iVBORw0KGgo"), "image/png");
        assert_eq!(detect_mime_type("/9j/4AAQ"), "image/jpeg");
    }
}
