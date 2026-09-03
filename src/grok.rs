use serde::{Deserialize, Serialize};

pub const GROK_GENERATE_URL: &str = "https://api.x.ai/v1/images/generations";
pub const GROK_EDIT_URL: &str = "https://api.x.ai/v1/images/edits";

pub const DEFAULT_MODEL: &str = "grok-imagine-image-2.0";
pub const DEFAULT_HEADSHOT_QUALITY: &str = "medium";
pub const MAX_EDIT_IMAGES: usize = 5;

#[derive(Serialize)]
pub struct GrokGenerateRequest {
    pub model: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GrokImageRef {
    pub url: String,
}

#[derive(Serialize)]
pub struct GrokEditRequest {
    pub model: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<GrokImageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<GrokImageRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
}

#[derive(Deserialize)]
pub struct GrokImageResponse {
    pub data: Vec<GrokImageData>,
}

#[derive(Deserialize)]
pub struct GrokImageData {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub b64_json: Option<String>,
    #[serde(default)]
    pub revised_prompt: Option<String>,
}

pub fn quality_supported(model: &str) -> bool {
    model.contains("2.0") || model.contains("2-0")
}
