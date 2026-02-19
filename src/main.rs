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

const GROK_API_URL: &str = "https://api.x.ai/v1/images/generations";

// --- Config ---

#[derive(Deserialize)]
struct Config {
    api_key: String,
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let path = PathBuf::from(home)
        .join(".config")
        .join("mcp-server-grok-image")
        .join("config.toml");
    let content = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "Failed to read config file: {}\n\
             Create it with your xAI API key.\n\
             Example:\n\n\
             api_key = \"xai-...\"\n\n\
             Error: {e}",
            path.display()
        )
    })?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
    Ok(config)
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
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EditImageParams {
    #[schemars(description = "URL or base64 data URI of the source image to edit")]
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

// --- MCP Server ---

#[derive(Clone)]
pub struct GrokImageServer {
    api_key: String,
    http: reqwest::Client,
    tool_router: ToolRouter<Self>,
}

impl GrokImageServer {
    async fn call_grok_api(&self, request: &GrokImageRequest) -> Result<GrokImageResponse, String> {
        let response = self
            .http
            .post(GROK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(request)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "failed to read response body".to_string());
            return Err(format!("Grok API error ({status}): {body}"));
        }

        response
            .json::<GrokImageResponse>()
            .await
            .map_err(|e| format!("Failed to parse Grok response: {e}"))
    }

    fn format_response(data: &[GrokImageData]) -> String {
        let mut parts = Vec::new();
        for (i, img) in data.iter().enumerate() {
            let label = if data.len() > 1 {
                format!("Image {}:\n", i + 1)
            } else {
                String::new()
            };

            let mut section = label;

            if let Some(url) = &img.url {
                section.push_str(&format!("URL: {url}\n"));
            }
            if let Some(b64) = &img.b64_json {
                let preview = if b64.len() > 80 {
                    format!("{}...", &b64[..80])
                } else {
                    b64.clone()
                };
                section.push_str(&format!("Base64: {preview}\n"));
            }
            if let Some(revised) = &img.revised_prompt {
                section.push_str(&format!("Revised prompt: {revised}\n"));
            }

            parts.push(section);
        }
        parts.join("\n")
    }
}

#[tool_router]
impl GrokImageServer {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: reqwest::Client::new(),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Generate an image from a text prompt using Grok's image generation API")]
    async fn generate_image(
        &self,
        Parameters(params): Parameters<GenerateImageParams>,
    ) -> Result<CallToolResult, McpError> {
        let model = params.model.unwrap_or_else(|| "grok-imagine-image-pro".to_string());

        let request = GrokImageRequest {
            model,
            prompt: params.prompt,
            n: params.n,
            response_format: params.response_format,
            image_url: None,
            aspect_ratio: params.aspect_ratio,
            resolution: params.resolution,
        };

        match self.call_grok_api(&request).await {
            Ok(resp) => {
                let text = Self::format_response(&resp.data);
                Ok(CallToolResult::success(vec![Content::text(text)]))
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
        let model = params.model.unwrap_or_else(|| "grok-imagine-image-pro".to_string());

        let request = GrokImageRequest {
            model,
            prompt: params.prompt,
            n: params.n,
            response_format: params.response_format,
            image_url: Some(params.image_url),
            aspect_ratio: None,
            resolution: params.resolution,
        };

        match self.call_grok_api(&request).await {
            Ok(resp) => {
                let text = Self::format_response(&resp.data);
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }
}

#[tool_handler]
impl ServerHandler for GrokImageServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "mcp-server-grok-image".to_string(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Grok image generation server. Use generate_image to create images from text prompts, \
                 or edit_image to modify existing images with natural language instructions."
                    .to_string(),
            ),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_config()?;
    let server = GrokImageServer::new(cfg.api_key);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
