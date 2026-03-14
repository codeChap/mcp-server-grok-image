use base64::Engine;
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
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, info, warn};

const GROK_API_URL: &str = "https://api.x.ai/v1/images/generations";

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
struct GrokImageRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    n: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_url: Option<String>,
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
        description = "Output resolution. Options: \"1k\" (~1024px, default), \"2k\" (~2048px). Supported by grok-imagine-image-pro and grok-imagine-image. Not supported by grok-2-image-1212."
    )]
    resolution: Option<String>,
    #[schemars(
        description = "Model to use. Options: \"grok-imagine-image-pro\" (default, premium quality, 1k/2k resolution, $0.07/image, 30 RPM), \"grok-imagine-image\" (faster, 1k/2k resolution, $0.02/image, 300 RPM), \"grok-2-image-1212\" (legacy, text-only input, no aspect_ratio or resolution support, $0.07/image, 300 RPM)"
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
        description = "URL or base64 data URI of the source image to edit. Also accepts a local file path."
    )]
    image_url: String,
    #[schemars(description = "Natural language edit instructions")]
    prompt: String,
    #[schemars(description = "Number of image variations to generate (1-10, default 1)")]
    n: Option<u8>,
    #[schemars(description = "Output format: \"url\" (default, temporary) or \"b64_json\"")]
    response_format: Option<String>,
    #[schemars(
        description = "Output resolution. Options: \"1k\" (~1024px, default), \"2k\" (~2048px). Supported by grok-imagine-image-pro and grok-imagine-image. Not supported by grok-2-image-1212."
    )]
    resolution: Option<String>,
    #[schemars(
        description = "Model to use. Options: \"grok-imagine-image-pro\" (default, premium quality, 1k/2k resolution, $0.07/image, 30 RPM), \"grok-imagine-image\" (faster, 1k/2k resolution, $0.02/image, 300 RPM), \"grok-2-image-1212\" (legacy, text-only input, no aspect_ratio or resolution support, $0.07/image, 300 RPM)"
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

/// Read a local file and convert to a data: URI for the API.
fn local_file_to_data_uri(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("Failed to read file {path}: {e}"))?;

    let mime = if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".gif") {
        "image/gif"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else {
        // Try to detect from magic bytes
        if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
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
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    info!(path, mime, "Encoded local file as data URI");
    Ok(format!("data:{mime};base64,{b64}"))
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
    async fn call_grok_api(&self, request: &GrokImageRequest) -> Result<GrokImageResponse, String> {
        debug!(
            model = request.model,
            prompt = request.prompt,
            "Sending request to Grok API"
        );

        let response = self
            .http
            .post(GROK_API_URL)
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
}

#[tool_router]
impl GrokImageServer {
    fn new(api_key: String, save_dir: PathBuf, styles: Vec<Style>) -> Self {
        Self {
            api_key,
            save_dir,
            http: reqwest::Client::new(),
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
            .unwrap_or_else(|| "grok-imagine-image-pro".to_string());

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

        let request = GrokImageRequest {
            model,
            prompt: prompt.clone(),
            n: params.n,
            response_format: params.response_format,
            image_url: None,
            aspect_ratio: params.aspect_ratio,
            resolution: params.resolution,
        };

        match self.call_grok_api(&request).await {
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
        description = "Edit an existing image using natural language instructions via Grok's image API"
    )]
    async fn edit_image(
        &self,
        Parameters(params): Parameters<EditImageParams>,
    ) -> Result<CallToolResult, McpError> {
        // Validate inputs
        if let Err(e) = validate_common_params(
            params.n,
            params.response_format.as_deref(),
            params.resolution.as_deref(),
        ) {
            return Ok(CallToolResult::error(vec![Content::text(e)]));
        }

        let model = params
            .model
            .unwrap_or_else(|| "grok-imagine-image-pro".to_string());

        // Resolve image_url: local file paths get converted to data URIs
        let image_url = if params.image_url.starts_with("http://")
            || params.image_url.starts_with("https://")
            || params.image_url.starts_with("data:")
        {
            params.image_url
        } else {
            // Treat as local file path
            info!(path = params.image_url, "Reading local file for edit_image");
            match local_file_to_data_uri(&params.image_url) {
                Ok(uri) => uri,
                Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
            }
        };

        info!(model, prompt = params.prompt, "edit_image called");

        let request = GrokImageRequest {
            model,
            prompt: params.prompt,
            n: params.n,
            response_format: params.response_format,
            image_url: Some(image_url),
            aspect_ratio: None,
            resolution: params.resolution,
        };

        match self.call_grok_api(&request).await {
            Ok(resp) => {
                let contents = self.format_response(&resp.data);
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
                "Grok image generation server. Use generate_image to create images from text prompts, \
                 or edit_image to modify existing images with natural language instructions.",
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
