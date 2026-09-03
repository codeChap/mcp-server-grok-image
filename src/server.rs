use crate::grok::{
    DEFAULT_MODEL, GROK_EDIT_URL, GROK_GENERATE_URL, GrokEditRequest, GrokGenerateRequest,
    GrokImageData, GrokImageRef, GrokImageResponse,
};
use crate::headshot::{
    build_headshot_prompt, normalize_pronoun, prepare_padded_jpeg, resolve_headshot_quality,
};
use crate::image_io::{
    bytes_to_data_uri, detect_mime_type, image_ext_from_bytes, load_image_bytes,
    resolve_edit_image_fields,
};
use crate::params::{
    EditImageParams, GenerateImageParams, HeadshotParams, ensure_quality_supported,
    validate_common_params, validate_image_params, validate_quality,
};
use crate::styles::Style;
use base64::Engine;
use rmcp::{
    ErrorData as McpError, ServerHandler, handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters, model::*, tool, tool_handler, tool_router,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct GrokImageServer {
    api_key: String,
    save_dir: PathBuf,
    http: reqwest::Client,
    counter: Arc<AtomicU64>,
    styles: Arc<Vec<Style>>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

fn tool_err(msg: impl Into<String>) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![Content::text(msg.into())]))
}

impl GrokImageServer {
    async fn call_grok_api<R: serde::Serialize>(
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

    fn next_save_path(&self, stem: &str, ext: &str) -> PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let seq = self.counter.fetch_add(1, Ordering::Relaxed);
        self.save_dir.join(format!("{stem}{ts}_{seq}.{ext}"))
    }

    fn save_image(&self, b64: &str) -> Option<PathBuf> {
        let bytes = match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to decode base64 for saving: {e}");
                return None;
            }
        };

        let ext = image_ext_from_bytes(&bytes);

        if let Err(e) = std::fs::create_dir_all(&self.save_dir) {
            warn!(dir = %self.save_dir.display(), "Failed to create save directory: {e}");
            return None;
        }

        let path = self.next_save_path("", ext);

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

    /// Assemble MCP text/image content for every API image (b64 and URL shapes).
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

            if let Some(b64) = &img.b64_json {
                let mime = detect_mime_type(b64);
                contents.push(Content::image(b64.as_str(), mime));

                let mut text_parts = Vec::new();
                if !label.is_empty() {
                    text_parts.push(label.clone());
                }
                if let Some(path) = self.save_image(b64) {
                    text_parts.push(format!("Saved: {}", path.display()));
                }
                if i == 0 {
                    if let Some(out) = output_path {
                        if let Err(e) = write_b64_to_path(b64, out) {
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

    fn save_padded_intermediate(&self, jpeg: &[u8]) {
        let _ = std::fs::create_dir_all(&self.save_dir);
        let pad_path = self.next_save_path("headshot-padded_", "jpg");
        if let Err(e) = std::fs::write(&pad_path, jpeg) {
            warn!(path = %pad_path.display(), "Failed to save padded intermediate: {e}");
        } else {
            debug!(path = %pad_path.display(), "Saved padded headshot intermediate");
        }
    }
}

fn write_b64_to_path(b64: &str, path: &Path) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("Failed to decode base64: {e}"))?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent dir {}: {e}", parent.display()))?;
        }
    }
    std::fs::write(path, &bytes).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    info!(path = %path.display(), size = bytes.len(), "Wrote headshot to output_path");
    Ok(())
}

#[tool_router]
impl GrokImageServer {
    pub fn new(api_key: String, save_dir: PathBuf, styles: Vec<Style>) -> Self {
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

    #[tool(
        description = "List available image styles that can be used with generate_image's style parameter"
    )]
    async fn list_styles(&self) -> Result<CallToolResult, McpError> {
        let mut lines = Vec::new();
        for s in self.styles.iter() {
            lines.push(format!(
                "- **{}**: {}\n  Template: `{}`",
                s.name, s.description, s.template
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(
            lines.join("\n\n"),
        )]))
    }

    #[tool(description = "Generate an image from a text prompt using Grok's image generation API")]
    async fn generate_image(
        &self,
        Parameters(params): Parameters<GenerateImageParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = validate_image_params(
            params.n,
            params.response_format.as_deref(),
            params.resolution.as_deref(),
            params.aspect_ratio.as_deref(),
            params.quality.as_deref(),
        ) {
            return tool_err(e);
        }

        let model = params.model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        if let Err(e) = ensure_quality_supported(&model, params.quality.as_deref()) {
            return tool_err(e);
        }

        let (prompt, style_applied) = if let Some(ref style_name) = params.style {
            if let Some(style) = self.styles.iter().find(|s| s.name == *style_name) {
                let styled = style.template.replace("{prompt}", &params.prompt);
                (styled, Some(style_name.as_str()))
            } else {
                let valid: Vec<&str> = self.styles.iter().map(|s| s.name.as_str()).collect();
                return tool_err(format!(
                    "Unknown style \"{style_name}\". Available styles: {}",
                    valid.join(", ")
                ));
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
            quality: params.quality,
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
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Edit an existing image (or compose up to 5 images) using natural language instructions via Grok's image-to-image API. For multi-image edits, pass `images` and reference each one as <IMAGE_0>, <IMAGE_1>, ..."
    )]
    async fn edit_image(
        &self,
        Parameters(params): Parameters<EditImageParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = validate_image_params(
            params.n,
            params.response_format.as_deref(),
            params.resolution.as_deref(),
            params.aspect_ratio.as_deref(),
            params.quality.as_deref(),
        ) {
            return tool_err(e);
        }

        let (image_field, images_field) =
            match resolve_edit_image_fields(params.image_url, params.images) {
                Ok(fields) => fields,
                Err(e) => return tool_err(e),
            };

        let model = params.model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        if let Err(e) = ensure_quality_supported(&model, params.quality.as_deref()) {
            return tool_err(e);
        }

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
            quality: params.quality,
        };

        match self.call_grok_api(GROK_EDIT_URL, &request).await {
            Ok(resp) => Ok(CallToolResult::success(self.format_response(&resp.data))),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Expand a portrait only where cropped (Gemini-style). Letterboxes the full original with white side padding (never crops or reframes), then calls grok-imagine-image-2.0 (quality medium) to complete cut-off shoulders if needed and put the scene on a clean solid white background. Keeps pose, face, hair, clothing, and logos unchanged. No cutout / no transparent alpha."
    )]
    async fn headshot(
        &self,
        Parameters(params): Parameters<HeadshotParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) =
            validate_common_params(params.n, Some("b64_json"), params.resolution.as_deref())
        {
            return tool_err(e);
        }
        if let Err(e) = validate_quality(params.quality.as_deref()) {
            return tool_err(e);
        }

        let pronoun = match normalize_pronoun(params.pronoun.as_deref()) {
            Ok(p) => p,
            Err(e) => return tool_err(e),
        };

        let gravity = params
            .gravity
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("North");
        let content_width = params.content_width.unwrap_or(550);
        let canvas_width = params.canvas_width.unwrap_or(780);
        let resolution = params.resolution.unwrap_or_else(|| "2k".to_string());
        // 2.0 at medium is the edit-fidelity replacement for grok-imagine-image-quality
        // (that slug retires 2026-11-02 and redirects to 2.0 at quality low).
        let model = params.model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let quality = match resolve_headshot_quality(&model, params.quality) {
            Ok(q) => q,
            Err(e) => return tool_err(e),
        };

        let prompt = build_headshot_prompt(
            params.clothing.as_deref(),
            params.notes.as_deref(),
            &pronoun,
        );

        info!(
            model,
            gravity, content_width, canvas_width, resolution, "headshot called"
        );

        let bytes = match load_image_bytes(&self.http, &params.image).await {
            Ok(b) => b,
            Err(e) => return tool_err(e),
        };
        let padded = match prepare_padded_jpeg(&bytes, content_width, canvas_width, gravity) {
            Ok(p) => p,
            Err(e) => return tool_err(e),
        };
        let data_uri = bytes_to_data_uri(&padded.jpeg, "image/jpeg");
        self.save_padded_intermediate(&padded.jpeg);

        let request = GrokEditRequest {
            model,
            prompt: prompt.clone(),
            image: Some(GrokImageRef { url: data_uri }),
            images: None,
            n: params.n,
            response_format: Some("b64_json".to_string()),
            aspect_ratio: Some("3:2".to_string()),
            resolution: Some(resolution),
            quality,
        };

        let output_path = params.output_path.as_ref().map(PathBuf::from);

        match self.call_grok_api(GROK_EDIT_URL, &request).await {
            Ok(resp) => {
                let mut contents = vec![Content::text(format!(
                    "Headshot pipeline: white letterbox {content_width}→{}×{} (gravity {gravity}) → expand-only (no cutout)\nPrompt: {prompt}",
                    padded.width, padded.height
                ))];
                contents
                    .extend(self.format_response_with_output(&resp.data, output_path.as_deref()));
                Ok(CallToolResult::success(contents))
            }
            Err(e) => tool_err(e),
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
                 general natural-language edits (multi-image: pass `images` up to 5, reference <IMAGE_0>, …); \
                 headshot only to expand cropped portraits onto a clean white background \
                 (Gemini-style letterbox + expand-only, no cutout; defaults to grok-imagine-image-2.0 at quality medium).",
            )
    }
}
