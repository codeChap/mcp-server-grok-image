use crate::grok::quality_supported;
use schemars::JsonSchema;
use serde::Deserialize;

const VALID_ASPECT_RATIOS: &[&str] = &[
    "1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "2:1", "1:2", "19.5:9", "9:19.5", "20:9",
    "9:20", "21:9", "5:2", "auto",
];

const VALID_QUALITIES: &[&str] = &["low", "medium", "auto"];

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateImageParams {
    #[schemars(description = "Text description of the desired image")]
    pub prompt: String,
    #[schemars(description = "Number of images to generate (1-10, default 1)")]
    pub n: Option<u8>,
    #[schemars(description = "Output format: \"url\" (default, temporary) or \"b64_json\"")]
    pub response_format: Option<String>,
    #[schemars(
        description = "Aspect ratio. Options: 1:1, 16:9, 9:16, 4:3, 3:4, 3:2, 2:3, 2:1, 1:2, 19.5:9, 9:19.5, 20:9, 9:20, 21:9, 5:2, auto"
    )]
    pub aspect_ratio: Option<String>,
    #[schemars(
        description = "Output resolution. Options: \"1k\" (~1024px, default), \"2k\" (~2048px)."
    )]
    pub resolution: Option<String>,
    #[schemars(
        description = "Quality for grok-imagine-image-2.0 only: \"low\", \"medium\", or \"auto\" (default when omitted). Auto currently serves low for generation and medium for editing; billed at the tier served."
    )]
    pub quality: Option<String>,
    #[schemars(
        description = "Model to use. Default: \"grok-imagine-image-2.0\". Also \"grok-imagine-image\" (1.0). \"grok-imagine-image-quality\" retires 2026-11-02 and redirects to 2.0 at quality low."
    )]
    pub model: Option<String>,
    #[schemars(
        description = "Optional style name. Use list_styles to see available options. When set, your prompt is wrapped in the style's template — avoid including style language in the prompt itself to prevent conflicts."
    )]
    pub style: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditImageParams {
    #[schemars(
        description = "URL, base64 data URI, or local file path of the source image. Mutually exclusive with `images`."
    )]
    pub image_url: Option<String>,
    #[schemars(
        description = "Up to 5 source images (URLs, base64 data URIs, or local file paths) for multi-image editing. Reference them in your prompt as <IMAGE_0>, <IMAGE_1>, etc. Mutually exclusive with `image_url`."
    )]
    pub images: Option<Vec<String>>,
    #[schemars(description = "Natural language edit instructions")]
    pub prompt: String,
    #[schemars(description = "Number of image variations to generate (1-10, default 1)")]
    pub n: Option<u8>,
    #[schemars(description = "Output format: \"url\" (default, temporary) or \"b64_json\"")]
    pub response_format: Option<String>,
    #[schemars(
        description = "Aspect ratio. Options: 1:1, 16:9, 9:16, 4:3, 3:4, 3:2, 2:3, 2:1, 1:2, 19.5:9, 9:19.5, 20:9, 9:20, 21:9, 5:2, auto"
    )]
    pub aspect_ratio: Option<String>,
    #[schemars(
        description = "Output resolution. Options: \"1k\" (~1024px, default), \"2k\" (~2048px)."
    )]
    pub resolution: Option<String>,
    #[schemars(
        description = "Quality for grok-imagine-image-2.0 only: \"low\", \"medium\", or \"auto\" (default when omitted). Auto currently serves medium for editing; billed at the tier served."
    )]
    pub quality: Option<String>,
    #[schemars(
        description = "Model to use. Default: \"grok-imagine-image-2.0\". Also \"grok-imagine-image\" (1.0). \"grok-imagine-image-quality\" retires 2026-11-02 and redirects to 2.0 at quality low."
    )]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HeadshotParams {
    #[schemars(
        description = "Source portrait: local path, http(s) URL, or data: URI. Never cropped — full image is letterboxed with white (Gemini-style expand prep)."
    )]
    pub image: String,
    #[schemars(
        description = "Optional clothing description for filling missing shoulders only (e.g. \"navy blazer over open white shirt\")."
    )]
    pub clothing: Option<String>,
    #[schemars(
        description = "Optional features that must stay exact (glasses, logo text, beard, earrings, …)."
    )]
    pub notes: Option<String>,
    #[schemars(
        description = "Pronoun for expand prompt: \"his\", \"her\", or \"their\" (default \"their\")."
    )]
    pub pronoun: Option<String>,
    #[schemars(
        description = "Letterbox gravity: North (default), NorthWest, NorthEast, Center, West, East, South, SouthWest, SouthEast."
    )]
    pub gravity: Option<String>,
    #[schemars(
        description = "Resize source to this width (px) before side-padding. Default 550 (matches original Gemini pipeline)."
    )]
    pub content_width: Option<u32>,
    #[schemars(
        description = "Padded canvas width (px). Default 780. Height follows the resized source (not forced 3:2)."
    )]
    pub canvas_width: Option<u32>,
    #[schemars(description = "Output resolution: \"1k\" or \"2k\" (default \"2k\").")]
    pub resolution: Option<String>,
    #[schemars(
        description = "Optional absolute path for the final image (also saved under save_dir)."
    )]
    pub output_path: Option<String>,
    #[schemars(description = "Number of variations (1-10, default 1)")]
    pub n: Option<u8>,
    #[schemars(
        description = "Quality for grok-imagine-image-2.0: \"low\", \"medium\", or \"auto\". Default \"medium\" (edit fidelity). Auto currently serves medium for editing."
    )]
    pub quality: Option<String>,
    #[schemars(
        description = "Model. Default: \"grok-imagine-image-2.0\" at quality medium. \"grok-imagine-image-quality\" retires 2026-11-02 and redirects to 2.0 at quality low."
    )]
    pub model: Option<String>,
}

pub fn validate_common_params(
    n: Option<u8>,
    response_format: Option<&str>,
    resolution: Option<&str>,
) -> Result<(), String> {
    if let Some(n) = n {
        if n < 1 || n > 10 {
            return Err(format!("n must be between 1 and 10, got {n}"));
        }
    }
    if let Some(rf) = response_format {
        if rf != "url" && rf != "b64_json" {
            return Err(format!(
                "response_format must be \"url\" or \"b64_json\", got \"{rf}\""
            ));
        }
    }
    if let Some(res) = resolution {
        if res != "1k" && res != "2k" {
            return Err(format!(
                "resolution must be \"1k\" or \"2k\", got \"{res}\""
            ));
        }
    }
    Ok(())
}

pub fn validate_aspect_ratio(aspect_ratio: Option<&str>) -> Result<(), String> {
    if let Some(ar) = aspect_ratio {
        if !VALID_ASPECT_RATIOS.contains(&ar) {
            return Err(format!(
                "Invalid aspect_ratio \"{ar}\". Valid options: {}",
                VALID_ASPECT_RATIOS.join(", ")
            ));
        }
    }
    Ok(())
}

pub fn validate_quality(quality: Option<&str>) -> Result<(), String> {
    if let Some(q) = quality {
        if !VALID_QUALITIES.contains(&q) {
            return Err(format!(
                "quality must be \"low\", \"medium\", or \"auto\", got \"{q}\""
            ));
        }
    }
    Ok(())
}

pub fn ensure_quality_supported(model: &str, quality: Option<&str>) -> Result<(), String> {
    if quality.is_some() && !quality_supported(model) {
        return Err(format!(
            "quality is only supported on grok-imagine-image-2.0 (got \"{model}\")"
        ));
    }
    Ok(())
}

pub fn validate_image_params(
    n: Option<u8>,
    response_format: Option<&str>,
    resolution: Option<&str>,
    aspect_ratio: Option<&str>,
    quality: Option<&str>,
) -> Result<(), String> {
    validate_common_params(n, response_format, resolution)?;
    validate_aspect_ratio(aspect_ratio)?;
    validate_quality(quality)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n_bounds() {
        assert!(validate_common_params(Some(1), None, None).is_ok());
        assert!(validate_common_params(Some(10), None, None).is_ok());
        assert!(validate_common_params(Some(0), None, None).is_err());
        assert!(validate_common_params(Some(11), None, None).is_err());
    }

    #[test]
    fn aspect_includes_ultrawide() {
        assert!(validate_aspect_ratio(Some("21:9")).is_ok());
        assert!(validate_aspect_ratio(Some("5:2")).is_ok());
        assert!(validate_aspect_ratio(Some("3:7")).is_err());
    }

    #[test]
    fn quality_enum_and_model_gate() {
        assert!(validate_quality(Some("auto")).is_ok());
        assert!(validate_quality(Some("high")).is_err());
        assert!(ensure_quality_supported("grok-imagine-image-2.0", Some("low")).is_ok());
        assert!(ensure_quality_supported("grok-imagine-image", Some("low")).is_err());
        assert!(ensure_quality_supported("grok-imagine-image", None).is_ok());
    }
}
