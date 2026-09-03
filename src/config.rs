use serde::Deserialize;
use std::path::PathBuf;
use tracing::info;

#[derive(Deserialize)]
pub struct StyleConfig {
    pub name: String,
    pub description: String,
    pub template: String,
}

#[derive(Deserialize)]
pub struct Config {
    pub api_key: String,
    #[serde(default = "default_save_dir")]
    pub save_dir: String,
    #[serde(default)]
    pub styles: Vec<StyleConfig>,
}

fn default_save_dir() -> String {
    "/tmp/grok-images".to_string()
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
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
        Err(_) => match std::env::var("XAI_API_KEY") {
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
        },
    }
}
