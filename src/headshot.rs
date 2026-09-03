use crate::grok::{DEFAULT_HEADSHOT_QUALITY, quality_supported};
use crate::params::ensure_quality_supported;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use std::io::Cursor;

pub struct PaddedHeadshot {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Gemini-equivalent prep: resize full image (no crop), then letterbox with white
/// so the model only expands into the white gutters. No cutout / no rembg.
///
///   convert src -resize {content_width}x resized
///   convert resized -gravity North -background white -extent {canvas_width}x padded
pub fn pad_to_headshot_canvas(
    img: &DynamicImage,
    content_width: u32,
    canvas_width: u32,
    gravity: &str,
) -> Result<RgbImage, String> {
    if content_width < 64 || content_width > 4096 {
        return Err(format!(
            "content_width must be between 64 and 4096, got {content_width}"
        ));
    }
    if canvas_width < content_width || canvas_width > 8192 {
        return Err(format!(
            "canvas_width must be between content_width ({content_width}) and 8192, got {canvas_width}"
        ));
    }

    let rgb = img.to_rgb8();
    let (ow, oh) = rgb.dimensions();
    if ow == 0 || oh == 0 {
        return Err("Source image has zero dimensions".to_string());
    }

    let scale = content_width as f32 / ow as f32;
    let nw = content_width;
    let nh = ((oh as f32 * scale).round() as u32).max(1);
    let resized = imageops::resize(&rgb, nw, nh, FilterType::Lanczos3);

    let canvas_height = nh;
    let mut canvas = RgbImage::from_pixel(canvas_width, canvas_height, Rgb([255, 255, 255]));
    let (x, y) = gravity_offset(gravity, canvas_width, canvas_height, nw, nh)?;
    imageops::overlay(&mut canvas, &resized, x as i64, y as i64);
    Ok(canvas)
}

fn gravity_offset(gravity: &str, cw: u32, ch: u32, nw: u32, nh: u32) -> Result<(u32, u32), String> {
    let g = gravity.to_ascii_lowercase().replace(['_', '-'], "");
    let x_left = 0u32;
    let x_center = cw.saturating_sub(nw) / 2;
    let x_right = cw.saturating_sub(nw);
    let y_top = 0u32;
    let y_center = ch.saturating_sub(nh) / 2;
    let y_bottom = ch.saturating_sub(nh);

    let pos = match g.as_str() {
        "north" | "n" => (x_center, y_top),
        "northwest" | "nw" => (x_left, y_top),
        "northeast" | "ne" => (x_right, y_top),
        "west" | "w" => (x_left, y_center),
        "east" | "e" => (x_right, y_center),
        "center" | "centre" | "c" => (x_center, y_center),
        "south" | "s" => (x_center, y_bottom),
        "southwest" | "sw" => (x_left, y_bottom),
        "southeast" | "se" => (x_right, y_bottom),
        _ => {
            return Err(format!(
                "Invalid gravity \"{gravity}\". Valid: North, NorthWest, NorthEast, Center, West, East, South, SouthWest, SouthEast"
            ));
        }
    };
    Ok(pos)
}

/// Expand-only prompt — same job as the original Gemini headshot skill.
/// White background only. No cutout, no reframe.
pub fn build_headshot_prompt(clothing: Option<&str>, notes: Option<&str>, pronoun: &str) -> String {
    let clothing_fill = clothing
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("the same clothing as in the photo");

    let mut parts: Vec<String> = Vec::new();
    parts.push(
        "Only expand this photograph into the pure white padded areas if the subject is cut off. \
         Do not reframe, re-pose, or re-shoot the subject. \
         Keep every pixel of the existing person unchanged: exact face, head angle, pose, hair, \
         expression, skin texture, clothing, logos/text on clothing, colors, and lighting."
            .into(),
    );

    if let Some(n) = notes.filter(|s| !s.trim().is_empty()) {
        parts.push(format!("Must preserve exactly: {n}."));
    }

    parts.push(format!(
        "Where blank white space begins at the edges and {pronoun} body is cut off, \
         fill that white only by completing {pronoun} shoulders/body in {clothing_fill}. \
         If the subject is already fully visible, do not invent extra limbs or a second pair of shoulders. \
         Put the whole scene on a clean solid pure white background \
         (replace any non-white backdrop with white; keep the person unchanged). \
         Do not change logos or branding on the clothing. Do not cut out hair or change the silhouette."
    ));

    parts.join(" ")
}

pub fn normalize_pronoun(raw: Option<&str>) -> Result<String, String> {
    let p = raw.unwrap_or("their").trim().to_ascii_lowercase();
    match p.as_str() {
        "his" | "her" | "their" => Ok(p),
        "he" => Ok("his".into()),
        "she" => Ok("her".into()),
        "they" => Ok("their".into()),
        _ => Err(format!(
            "pronoun must be \"his\", \"her\", or \"their\" (got \"{}\")",
            raw.unwrap_or("")
        )),
    }
}

pub fn resolve_headshot_quality(
    model: &str,
    quality: Option<String>,
) -> Result<Option<String>, String> {
    if quality_supported(model) {
        Ok(Some(
            quality.unwrap_or_else(|| DEFAULT_HEADSHOT_QUALITY.to_string()),
        ))
    } else {
        ensure_quality_supported(model, quality.as_deref())?;
        Ok(None)
    }
}

pub fn prepare_padded_jpeg(
    source_bytes: &[u8],
    content_width: u32,
    canvas_width: u32,
    gravity: &str,
) -> Result<PaddedHeadshot, String> {
    let dyn_img = image::load_from_memory(source_bytes)
        .map_err(|e| format!("Failed to decode source image: {e}"))?;
    let padded = pad_to_headshot_canvas(&dyn_img, content_width, canvas_width, gravity)?;
    let (width, height) = padded.dimensions();
    let mut jpeg = Vec::new();
    DynamicImage::ImageRgb8(padded)
        .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .map_err(|e| format!("Failed to encode padded JPEG: {e}"))?;
    Ok(PaddedHeadshot {
        jpeg,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn letterbox_never_crops_and_adds_white_sides() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(100, 200, Rgb([10, 20, 30])));
        let out = pad_to_headshot_canvas(&img, 550, 780, "North").unwrap();
        assert_eq!(out.dimensions(), (780, 1100));
        assert_eq!(out.get_pixel(0, 0), &Rgb([255, 255, 255]));
        assert_eq!(out.get_pixel(779, 0), &Rgb([255, 255, 255]));
        assert_eq!(out.get_pixel(390, 10), &Rgb([10, 20, 30]));
    }

    #[test]
    fn northwest_shifts_content_right() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(100, 50, Rgb([255, 0, 0])));
        let out = pad_to_headshot_canvas(&img, 200, 400, "NorthWest").unwrap();
        assert_eq!(out.get_pixel(0, 0), &Rgb([255, 0, 0]));
        assert_eq!(out.get_pixel(399, 0), &Rgb([255, 255, 255]));
    }

    #[test]
    fn prompt_is_expand_only_white_bg_no_cutout() {
        let p = build_headshot_prompt(Some("navy blazer"), Some("glasses"), "his");
        assert!(p.contains("Only expand"));
        assert!(p.contains("navy blazer"));
        assert!(p.contains("Do not reframe"));
        assert!(p.to_lowercase().contains("white background"));
        assert!(!p.to_lowercase().contains("transparent"));
        assert!(!p.to_lowercase().contains("cutout"));
        assert!(!p.to_lowercase().contains("face centered"));
    }

    #[test]
    fn pronoun_normalization() {
        assert_eq!(normalize_pronoun(Some("He")).unwrap(), "his");
        assert_eq!(normalize_pronoun(None).unwrap(), "their");
        assert!(normalize_pronoun(Some("x")).is_err());
    }

    #[test]
    fn quality_defaults_medium_on_2_0() {
        let q = resolve_headshot_quality("grok-imagine-image-2.0", None).unwrap();
        assert_eq!(q.as_deref(), Some("medium"));
        let q = resolve_headshot_quality("grok-imagine-image-quality", None).unwrap();
        assert_eq!(q, None);
        assert!(
            resolve_headshot_quality("grok-imagine-image-quality", Some("low".into())).is_err()
        );
    }
}
