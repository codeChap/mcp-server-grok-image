use base64::Engine;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, info, warn};

const GROK_GENERATE_URL: &str = "https://api.x.ai/v1/images/generations";
const GROK_EDIT_URL: &str = "https://api.x.ai/v1/images/edits";

const VALID_ASPECT_RATIOS: &[&str] = &[
    "1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "2:1", "1:2", "19.5:9", "9:19.5",
    "20:9", "9:20", "auto",
];

// --- Styles ---

#[derive(Clone)]
struct Style {
    name: String,
    description: String,
    template: String,
}

const BUILTIN_STYLES: &[(&str, &str, &str)] = &[
    ("watercolor", "Watercolor painting style", "{prompt}, as a watercolor painting"),
    ("oil-painting", "Oil painting with visible brushstrokes", "{prompt}, as an oil painting with visible brushstrokes"),
    ("pencil-sketch", "Detailed pencil sketch", "{prompt}, as a detailed pencil sketch"),
    ("pixel-art", "Retro pixel art", "{prompt}, as retro pixel art"),
    ("anime", "Anime style illustration", "{prompt}, in anime style"),
    ("pop-art", "Bold pop art style", "{prompt}, in bold pop art style"),
    ("art-nouveau", "Art nouveau with flowing organic lines", "{prompt}, in art nouveau style with flowing organic lines"),
    ("cinematic", "Cinematic photography with dramatic lighting", "{prompt}, cinematic photography with dramatic lighting"),
    ("portrait", "Professional portrait photography", "{prompt}, professional portrait photography with shallow depth of field"),
    ("macro", "Extreme macro photography", "{prompt}, extreme macro photography with sharp detail"),
    ("aerial", "Aerial drone photography", "{prompt}, aerial drone photography"),
    ("studio", "Studio photography on clean background", "{prompt}, studio photography on clean background with controlled lighting"),
    ("noir", "Dark film noir style", "{prompt}, in dark film noir style with high contrast black and white"),
    ("vintage", "Faded vintage photograph", "{prompt}, as a faded vintage photograph with warm tones"),
];

fn build_styles(custom: &[StyleConfig]) -> Vec<Style> {
    let mut styles: Vec<Style> = BUILTIN_STYLES
        .iter()
        .map(|(name, desc, tmpl)| Style {
            name: name.to_string(),
            description: desc.to_string(),
            template: tmpl.to_string(),
        })
        .collect();

    for cs in custom {
        if !cs.template.contains("{prompt}") {
            warn!(
                name = cs.name,
                template = cs.template,
                "Custom style template missing {{prompt}} placeholder, skipping"
            );
            continue;
        }
        if let Some(existing) = styles.iter_mut().find(|s| s.name == cs.name) {
            info!(name = cs.name, "Custom style overrides built-in");
            existing.description = cs.description.clone();
            existing.template = cs.template.clone();
        } else {
            styles.push(Style {
                name: cs.name.clone(),
                description: cs.description.clone(),
                template: cs.template.clone(),
            });
        }
    }

    styles
}

// --- Config ---

#[derive(Deserialize)]
struct StyleConfig {
    name: String,
    description: String,
    template: String,
}

#[derive(Deserialize)]
struct Config {
    api_key: String,
    #[serde(default = "default_save_dir")]
    save_dir: String,
    #[serde(default)]
    styles: Vec<StyleConfig>,
}

fn default_save_dir() -> String {
    "/tmp/grok-images".to_string()
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let path = PathBuf::from(home)
        .join(".config")
        .join("mcp-server-grok-image")
        .join("config.toml");

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let config: Config = toml::from_str(&content)
                .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
            info!(path = %path.display(), "Config loaded from file");
            Ok(config)
        }
        Err(_) => {
            // Fall back to environment variable
            match std::env::var("XAI_API_KEY") {
                Ok(api_key) => {
                    info!("Config loaded from XAI_API_KEY environment variable");
                    Ok(Config {
                        api_key,
                        save_dir: default_save_dir(),
                        styles: Vec::new(),
                    })
                }
                Err(_) => Err(format!(
                    "No config found. Either:\n\
                     1. Create {} with:\n\
                     \n\
                     api_key = \"xai-...\"\n\
                     save_dir = \"/tmp/grok-images\"  # optional\n\
                     \n\
                     2. Or set the XAI_API_KEY environment variable.",
                    path.display()
                )
                .into()),
            }
        }
    }
}

// --- Grok API request/response types ---

#[derive(Serialize)]
struct GrokGenerateRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    n: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolution: Option<String>,
}

#[derive(Serialize)]
struct GrokImageRef {
    url: String,
}

#[derive(Serialize)]
struct GrokEditRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<GrokImageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<GrokImageRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    n: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolution: Option<String>,
}

#[derive(Deserialize)]
struct GrokImageResponse {
    data: Vec<GrokImageData>,
}

#[derive(Deserialize)]
struct GrokImageData {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    b64_json: Option<String>,
    #[serde(default)]
    revised_prompt: Option<String>,
}

// --- MCP tool parameter types ---

#[derive(Debug, Deserialize, JsonSchema)]
struct GenerateImageParams {
    #[schemars(description = "Text description of the desired image")]
    prompt: String,
    #[schemars(description = "Number of images to generate (1-10, default 1)")]
    n: Option<u8>,
    #[schemars(description = "Output format: \"url\" (default, temporary) or \"b64_json\"")]
    response_format: Option<String>,
    #[schemars(
        description = "Aspect ratio. Options: 1:1, 16:9, 9:16, 4:3, 3:4, 3:2, 2:3, 2:1, 1:2, 19.5:9, 9:19.5, 20:9, 9:20, auto"
    )]
    aspect_ratio: Option<String>,
    #[schemars(
        description = "Output resolution. Options: \"1k\" (~1024px, default), \"2k\" (~2048px)."
    )]
    resolution: Option<String>,
    #[schemars(
        description = "Model to use. Default: \"grok-imagine-image\" (1k/2k resolution, $0.02/image, 300 RPM)."
    )]
    model: Option<String>,
    #[schemars(
        description = "Optional style name. Use list_styles to see available options. When set, your prompt is wrapped in the style's template — avoid including style language in the prompt itself to prevent conflicts."
    )]
    style: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EditImageParams {
    #[schemars(
        description = "URL, base64 data URI, or local file path of the source image. Mutually exclusive with `images`."
    )]
    image_url: Option<String>,
    #[schemars(
        description = "Up to 3 source images (URLs, base64 data URIs, or local file paths) for multi-image editing. Reference them in your prompt as <IMAGE_0>, <IMAGE_1>, etc. Mutually exclusive with `image_url`."
    )]
    images: Option<Vec<String>>,
    #[schemars(description = "Natural language edit instructions")]
    prompt: String,
    #[schemars(description = "Number of image variations to generate (1-10, default 1)")]
    n: Option<u8>,
    #[schemars(description = "Output format: \"url\" (default, temporary) or \"b64_json\"")]
    response_format: Option<String>,
    #[schemars(
        description = "Aspect ratio. Options: 1:1, 16:9, 9:16, 4:3, 3:4, 3:2, 2:3, 2:1, 1:2, 19.5:9, 9:19.5, 20:9, 9:20, auto"
    )]
    aspect_ratio: Option<String>,
    #[schemars(
        description = "Output resolution. Options: \"1k\" (~1024px, default), \"2k\" (~2048px)."
    )]
    resolution: Option<String>,
    #[schemars(
        description = "Model to use. Default: \"grok-imagine-image\" (1k/2k resolution, $0.02/image, 300 RPM)."
    )]
    model: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct HeadshotParams {
    #[schemars(
        description = "Source portrait: local path, http(s) URL, or data: URI. Never cropped — full image is letterboxed with white (Gemini-style expand prep)."
    )]
    image: String,
    #[schemars(
        description = "Optional clothing description for filling missing shoulders only (e.g. \"navy blazer over open white shirt\")."
    )]
    clothing: Option<String>,
    #[schemars(
        description = "Optional features that must stay exact (glasses, logo text, beard, earrings, …)."
    )]
    notes: Option<String>,
    #[schemars(
        description = "Pronoun for expand prompt: \"his\", \"her\", or \"their\" (default \"their\")."
    )]
    pronoun: Option<String>,
    #[schemars(
        description = "Letterbox gravity: North (default), NorthWest, NorthEast, Center, West, East, South, SouthWest, SouthEast."
    )]
    gravity: Option<String>,
    #[schemars(
        description = "Resize source to this width (px) before side-padding. Default 550 (matches original Gemini pipeline)."
    )]
    content_width: Option<u32>,
    #[schemars(
        description = "Padded canvas width (px). Default 780. Height follows the resized source (not forced 3:2)."
    )]
    canvas_width: Option<u32>,
    #[schemars(
        description = "Output resolution: \"1k\" or \"2k\" (default \"2k\")."
    )]
    resolution: Option<String>,
    #[schemars(
        description = "Optional absolute path for the final image (also saved under save_dir)."
    )]
    output_path: Option<String>,
    #[schemars(description = "Number of variations (1-10, default 1)")]
    n: Option<u8>,
    #[schemars(
        description = "Model. Default: \"grok-imagine-image-quality\" (new Imagine quality model; closer fidelity for edits)."
    )]
    model: Option<String>,
}

// --- Input validation ---

fn validate_common_params(
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
            return Err(format!("resolution must be \"1k\" or \"2k\", got \"{res}\""));
        }
    }
    Ok(())
}

fn validate_aspect_ratio(aspect_ratio: Option<&str>) -> Result<(), String> {
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

// --- Helpers ---

/// Detect MIME type from base64-encoded image data by inspecting magic bytes.
fn detect_mime_type(b64: &str) -> &'static str {
    if b64.starts_with("iVBOR") {
        "image/png"
    } else if b64.starts_with("/9j/") {
        "image/jpeg"
    } else if b64.starts_with("R0lGOD") {
        "image/gif"
    } else if b64.starts_with("UklGR") {
        "image/webp"
    } else {
        "image/png" // default
    }
}

/// Normalize a user-supplied image source to a value the API accepts.
/// HTTP(S) URLs and `data:` URIs pass through; anything else is treated as a local file path.
fn resolve_image_source(src: &str) -> Result<String, String> {
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
        Ok(src.to_string())
    } else {
        info!(path = src, "Encoding local file as data URI for image source");
        local_file_to_data_uri(src)
    }
}

/// Read a local file and convert to a data: URI for the API.
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
    } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.starts_with(b"GIF") {
        "image/gif"
    } else if data.len() >= 12 && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

fn bytes_to_data_uri(data: &[u8], mime: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    format!("data:{mime};base64,{b64}")
}

/// Decode a `data:` URI into raw bytes.
fn decode_data_uri(src: &str) -> Result<Vec<u8>, String> {
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

/// Gemini-equivalent prep: resize full image (no crop), then letterbox with white
/// so the model only expands into the white gutters. No cutout / no rembg.
///
///   convert src -resize {content_width}x resized
///   convert resized -gravity North -background white -extent {canvas_width}x padded
fn pad_to_headshot_canvas(
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

fn gravity_offset(
    gravity: &str,
    cw: u32,
    ch: u32,
    nw: u32,
    nh: u32,
) -> Result<(u32, u32), String> {
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
fn build_headshot_prompt(clothing: Option<&str>, notes: Option<&str>, pronoun: &str) -> String {
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

fn normalize_pronoun(raw: Option<&str>) -> Result<String, String> {
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

// --- MCP Server ---

#[derive(Clone)]
pub struct GrokImageServer {
    api_key: String,
    save_dir: PathBuf,
    http: reqwest::Client,
    counter: Arc<AtomicU64>,
    styles: Arc<Vec<Style>>,
    tool_router: ToolRouter<Self>,
}

impl GrokImageServer {
    async fn call_grok_api<R: Serialize>(
        &self,
        url: &str,
        request: &R,
    ) -> Result<GrokImageResponse, String> {
        debug!(url, "Sending request to Grok API");

        let response = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(request)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        let status = response.status();
        debug!(%status, "Grok API response received");

        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "failed to read response body".to_string());
            warn!(%status, body = body, "Grok API error");
            return Err(format!("Grok API error ({status}): {body}"));
        }

        response
            .json::<GrokImageResponse>()
            .await
            .map_err(|e| format!("Failed to parse Grok response: {e}"))
    }

    fn save_image(&self, b64: &str) -> Option<PathBuf> {
        let bytes = match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to decode base64 for saving: {e}");
                return None;
            }
        };

        // Detect extension from decoded bytes
        let ext = if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            "png"
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            "jpg"
        } else if bytes.starts_with(b"GIF") {
            "gif"
        } else if bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
            "webp"
        } else {
            "png"
        };

        if let Err(e) = std::fs::create_dir_all(&self.save_dir) {
            warn!(dir = %self.save_dir.display(), "Failed to create save directory: {e}");
            return None;
        }

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let seq = self.counter.fetch_add(1, Ordering::Relaxed);
        let filename = format!("{ts}_{seq}.{ext}");
        let path = self.save_dir.join(&filename);

        match std::fs::write(&path, &bytes) {
            Ok(_) => {
                info!(path = %path.display(), size = bytes.len(), "Image saved to disk");
                Some(path)
            }
            Err(e) => {
                warn!(path = %path.display(), "Failed to save image: {e}");
                None
            }
        }
    }

    fn format_response(&self, data: &[GrokImageData]) -> Vec<Content> {
        self.format_response_with_output(data, None)
    }

    fn format_response_with_output(
        &self,
        data: &[GrokImageData],
        output_path: Option<&Path>,
    ) -> Vec<Content> {
        let mut contents = Vec::new();

        for (i, img) in data.iter().enumerate() {
            let label = if data.len() > 1 {
                format!("Image {}:", i + 1)
            } else {
                String::new()
            };

            // Return base64 images as proper MCP image content
            if let Some(b64) = &img.b64_json {
                let mime = detect_mime_type(b64);
                contents.push(Content::image(b64.as_str(), mime));

                // Save to disk and report path
                let mut text_parts = Vec::new();
                if !label.is_empty() {
                    text_parts.push(label.clone());
                }
                if let Some(path) = self.save_image(b64) {
                    text_parts.push(format!("Saved: {}", path.display()));
                }
                // First variation only for explicit output_path
                if i == 0 {
                    if let Some(out) = output_path {
                        if let Err(e) = self.write_b64_to_path(b64, out) {
                            text_parts.push(format!("Failed to write output_path: {e}"));
                        } else {
                            text_parts.push(format!("Output: {}", out.display()));
                        }
                    }
                }
                if let Some(revised) = &img.revised_prompt {
                    text_parts.push(format!("Revised prompt: {revised}"));
                }
                if !text_parts.is_empty() {
                    contents.push(Content::text(text_parts.join("\n")));
                }
            } else if let Some(url) = &img.url {
                // URL-only response
                let mut text_parts = Vec::new();
                if !label.is_empty() {
                    text_parts.push(label);
                }
                text_parts.push(format!("URL: {url}"));
                if let Some(revised) = &img.revised_prompt {
                    text_parts.push(format!("Revised prompt: {revised}"));
                }
                contents.push(Content::text(text_parts.join("\n")));
            }
        }

        if contents.is_empty() {
            contents.push(Content::text("No image data returned by API."));
        }

        contents
    }

    fn write_b64_to_path(&self, b64: &str, path: &Path) -> Result<(), String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("Failed to decode base64: {e}"))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent dir {}: {e}", parent.display()))?;
            }
        }
        std::fs::write(path, &bytes)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        info!(path = %path.display(), size = bytes.len(), "Wrote headshot to output_path");
        Ok(())
    }

    async fn load_image_bytes(&self, src: &str) -> Result<Vec<u8>, String> {
        if src.starts_with("data:") {
            decode_data_uri(src)
        } else if src.starts_with("http://") || src.starts_with("https://") {
            info!(url = src, "Downloading image for headshot pad");
            let resp = self
                .http
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
}

#[tool_router]
impl GrokImageServer {
    fn new(api_key: String, save_dir: PathBuf, styles: Vec<Style>) -> Self {
        Self {
            api_key,
            save_dir,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            counter: Arc::new(AtomicU64::new(0)),
            styles: Arc::new(styles),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List available image styles that can be used with generate_image's style parameter")]
    async fn list_styles(&self) -> Result<CallToolResult, McpError> {
        let mut lines = Vec::new();
        for s in self.styles.iter() {
            lines.push(format!(
                "- **{}**: {}\n  Template: `{}`",
                s.name, s.description, s.template
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(lines.join("\n\n"))]))
    }

    #[tool(description = "Generate an image from a text prompt using Grok's image generation API")]
    async fn generate_image(
        &self,
        Parameters(params): Parameters<GenerateImageParams>,
    ) -> Result<CallToolResult, McpError> {
        // Validate inputs
        if let Err(e) = validate_common_params(
            params.n,
            params.response_format.as_deref(),
            params.resolution.as_deref(),
        ) {
            return Ok(CallToolResult::error(vec![Content::text(e)]));
        }
        if let Err(e) = validate_aspect_ratio(params.aspect_ratio.as_deref()) {
            return Ok(CallToolResult::error(vec![Content::text(e)]));
        }

        let model = params
            .model
            .unwrap_or_else(|| "grok-imagine-image".to_string());

        // Resolve style
        let (prompt, style_applied) = if let Some(ref style_name) = params.style {
            if let Some(style) = self.styles.iter().find(|s| s.name == *style_name) {
                let styled = style.template.replace("{prompt}", &params.prompt);
                (styled, Some(style_name.as_str()))
            } else {
                let valid: Vec<&str> = self.styles.iter().map(|s| s.name.as_str()).collect();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Unknown style \"{style_name}\". Available styles: {}",
                    valid.join(", ")
                ))]));
            }
        } else {
            (params.prompt, None)
        };

        info!(model, prompt, style = ?style_applied, "generate_image called");

        let request = GrokGenerateRequest {
            model,
            prompt: prompt.clone(),
            n: params.n,
            response_format: params.response_format,
            aspect_ratio: params.aspect_ratio,
            resolution: params.resolution,
        };

        match self.call_grok_api(GROK_GENERATE_URL, &request).await {
            Ok(resp) => {
                let mut contents = Vec::new();
                if let Some(style_name) = style_applied {
                    contents.push(Content::text(format!(
                        "Style applied: {style_name}\nPrompt sent: {prompt}"
                    )));
                }
                contents.extend(self.format_response(&resp.data));
                Ok(CallToolResult::success(contents))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Edit an existing image (or compose up to 3 images) using natural language instructions via Grok's image-to-image API. For multi-image edits, pass `images` and reference each one as <IMAGE_0>, <IMAGE_1>, ..."
    )]
    async fn edit_image(
        &self,
        Parameters(params): Parameters<EditImageParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = validate_common_params(
            params.n,
            params.response_format.as_deref(),
            params.resolution.as_deref(),
        ) {
            return Ok(CallToolResult::error(vec![Content::text(e)]));
        }
        if let Err(e) = validate_aspect_ratio(params.aspect_ratio.as_deref()) {
            return Ok(CallToolResult::error(vec![Content::text(e)]));
        }

        let (image_field, images_field) = match (params.image_url, params.images) {
            (Some(_), Some(_)) => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Provide either image_url or images, not both.".to_string(),
                )]));
            }
            (None, None) => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Provide either image_url or images.".to_string(),
                )]));
            }
            (Some(src), None) => {
                let url = match resolve_image_source(&src) {
                    Ok(u) => u,
                    Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
                };
                (Some(GrokImageRef { url }), None)
            }
            (None, Some(list)) => {
                if list.is_empty() {
                    return Ok(CallToolResult::error(vec![Content::text(
                        "images must contain at least one source image.".to_string(),
                    )]));
                }
                if list.len() > 3 {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "images supports at most 3 entries, got {}.",
                        list.len()
                    ))]));
                }
                let mut refs = Vec::with_capacity(list.len());
                for src in list {
                    let url = match resolve_image_source(&src) {
                        Ok(u) => u,
                        Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
                    };
                    refs.push(GrokImageRef { url });
                }
                (None, Some(refs))
            }
        };

        let model = params
            .model
            .unwrap_or_else(|| "grok-imagine-image".to_string());

        info!(
            model,
            prompt = params.prompt,
            multi = images_field.is_some(),
            "edit_image called"
        );

        let request = GrokEditRequest {
            model,
            prompt: params.prompt,
            image: image_field,
            images: images_field,
            n: params.n,
            response_format: params.response_format,
            aspect_ratio: params.aspect_ratio,
            resolution: params.resolution,
        };

        match self.call_grok_api(GROK_EDIT_URL, &request).await {
            Ok(resp) => {
                let contents = self.format_response(&resp.data);
                Ok(CallToolResult::success(contents))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Expand a portrait only where cropped (Gemini-style). Letterboxes the full original with white side padding (never crops or reframes), then calls grok-imagine-image-quality to complete cut-off shoulders if needed and put the scene on a clean solid white background. Keeps pose, face, hair, clothing, and logos unchanged. No cutout / no transparent alpha."
    )]
    async fn headshot(
        &self,
        Parameters(params): Parameters<HeadshotParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = validate_common_params(params.n, Some("b64_json"), params.resolution.as_deref())
        {
            return Ok(CallToolResult::error(vec![Content::text(e)]));
        }

        let pronoun = match normalize_pronoun(params.pronoun.as_deref()) {
            Ok(p) => p,
            Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
        };

        let gravity = params
            .gravity
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("North");
        let content_width = params.content_width.unwrap_or(550);
        let canvas_width = params.canvas_width.unwrap_or(780);
        let resolution = params
            .resolution
            .unwrap_or_else(|| "2k".to_string());
        // New Imagine quality model — better edit fidelity than grok-imagine-image
        let model = params
            .model
            .unwrap_or_else(|| "grok-imagine-image-quality".to_string());

        let prompt = build_headshot_prompt(
            params.clothing.as_deref(),
            params.notes.as_deref(),
            &pronoun,
        );

        info!(
            model,
            gravity,
            content_width,
            canvas_width,
            resolution,
            "headshot called"
        );

        // 1) Load source → 2) letterbox pad (no crop) → 3) expand-only edit
        let bytes = match self.load_image_bytes(&params.image).await {
            Ok(b) => b,
            Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
        };
        let dyn_img = match image::load_from_memory(&bytes) {
            Ok(i) => i,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to decode source image: {e}"
                ))]));
            }
        };
        let padded = match pad_to_headshot_canvas(&dyn_img, content_width, canvas_width, gravity)
        {
            Ok(p) => p,
            Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
        };
        let (pad_w, pad_h) = padded.dimensions();
        let mut jpeg_buf = Vec::new();
        if let Err(e) = DynamicImage::ImageRgb8(padded).write_to(
            &mut Cursor::new(&mut jpeg_buf),
            ImageFormat::Jpeg,
        ) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to encode padded JPEG: {e}"
            ))]));
        }
        let data_uri = bytes_to_data_uri(&jpeg_buf, "image/jpeg");

        let _ = std::fs::create_dir_all(&self.save_dir);
        let pad_path = self.save_dir.join(format!(
            "headshot-padded_{}_{}.jpg",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            self.counter.fetch_add(1, Ordering::Relaxed)
        ));
        if let Err(e) = std::fs::write(&pad_path, &jpeg_buf) {
            warn!(path = %pad_path.display(), "Failed to save padded intermediate: {e}");
        } else {
            debug!(path = %pad_path.display(), "Saved padded headshot intermediate");
        }

        let request = GrokEditRequest {
            model,
            prompt: prompt.clone(),
            image: Some(GrokImageRef { url: data_uri }),
            images: None,
            n: params.n,
            response_format: Some("b64_json".to_string()),
            aspect_ratio: Some("3:2".to_string()),
            resolution: Some(resolution),
        };

        let output_path = params.output_path.as_ref().map(PathBuf::from);

        match self.call_grok_api(GROK_EDIT_URL, &request).await {
            Ok(resp) => {
                let mut contents = vec![Content::text(format!(
                    "Headshot pipeline: white letterbox {content_width}→{pad_w}×{pad_h} (gravity {gravity}) → expand-only (no cutout)\nPrompt: {prompt}"
                ))];
                contents.extend(self.format_response_with_output(
                    &resp.data,
                    output_path.as_deref(),
                ));
                Ok(CallToolResult::success(contents))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }
}

#[tool_handler]
impl ServerHandler for GrokImageServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "mcp-server-grok-image",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Grok image generation server. Use generate_image for text-to-image; edit_image for \
                 general natural-language edits (multi-image: pass `images`, reference <IMAGE_0>, …); \
                 headshot only to expand cropped portraits onto a clean white background \
                 (Gemini-style letterbox + expand-only, no cutout; defaults to grok-imagine-image-quality).",
            )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to stderr (stdout is reserved for MCP JSON-RPC)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cfg = load_config()?;
    let save_dir = PathBuf::from(&cfg.save_dir);
    let styles = build_styles(&cfg.styles);
    info!(
        save_dir = %save_dir.display(),
        style_count = styles.len(),
        "Starting mcp-server-grok-image"
    );

    let server = GrokImageServer::new(cfg.api_key, save_dir, styles);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
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
}
