use crate::config::StyleConfig;
use tracing::{info, warn};

#[derive(Clone)]
pub struct Style {
    pub name: String,
    pub description: String,
    pub template: String,
}

const BUILTIN_STYLES: &[(&str, &str, &str)] = &[
    (
        "watercolor",
        "Watercolor painting style",
        "{prompt}, as a watercolor painting",
    ),
    (
        "oil-painting",
        "Oil painting with visible brushstrokes",
        "{prompt}, as an oil painting with visible brushstrokes",
    ),
    (
        "pencil-sketch",
        "Detailed pencil sketch",
        "{prompt}, as a detailed pencil sketch",
    ),
    (
        "pixel-art",
        "Retro pixel art",
        "{prompt}, as retro pixel art",
    ),
    (
        "anime",
        "Anime style illustration",
        "{prompt}, in anime style",
    ),
    (
        "pop-art",
        "Bold pop art style",
        "{prompt}, in bold pop art style",
    ),
    (
        "art-nouveau",
        "Art nouveau with flowing organic lines",
        "{prompt}, in art nouveau style with flowing organic lines",
    ),
    (
        "cinematic",
        "Cinematic photography with dramatic lighting",
        "{prompt}, cinematic photography with dramatic lighting",
    ),
    (
        "portrait",
        "Professional portrait photography",
        "{prompt}, professional portrait photography with shallow depth of field",
    ),
    (
        "macro",
        "Extreme macro photography",
        "{prompt}, extreme macro photography with sharp detail",
    ),
    (
        "aerial",
        "Aerial drone photography",
        "{prompt}, aerial drone photography",
    ),
    (
        "studio",
        "Studio photography on clean background",
        "{prompt}, studio photography on clean background with controlled lighting",
    ),
    (
        "noir",
        "Dark film noir style",
        "{prompt}, in dark film noir style with high contrast black and white",
    ),
    (
        "vintage",
        "Faded vintage photograph",
        "{prompt}, as a faded vintage photograph with warm tones",
    ),
];

pub fn build_styles(custom: &[StyleConfig]) -> Vec<Style> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(name: &str, description: &str, template: &str) -> StyleConfig {
        StyleConfig {
            name: name.into(),
            description: description.into(),
            template: template.into(),
        }
    }

    #[test]
    fn builtins_include_watercolor() {
        let styles = build_styles(&[]);
        assert!(styles.iter().any(|s| s.name == "watercolor"));
        assert_eq!(styles.len(), BUILTIN_STYLES.len());
    }

    #[test]
    fn custom_style_appends() {
        let styles = build_styles(&[custom(
            "my-style",
            "Custom look",
            "{prompt}, in my custom style",
        )]);
        let found = styles.iter().find(|s| s.name == "my-style").unwrap();
        assert_eq!(found.template, "{prompt}, in my custom style");
    }

    #[test]
    fn same_name_overrides_builtin() {
        let styles = build_styles(&[custom(
            "watercolor",
            "Loose watercolor",
            "{prompt}, as a loose watercolor",
        )]);
        let found = styles.iter().find(|s| s.name == "watercolor").unwrap();
        assert_eq!(found.description, "Loose watercolor");
        assert_eq!(found.template, "{prompt}, as a loose watercolor");
        assert_eq!(styles.iter().filter(|s| s.name == "watercolor").count(), 1);
    }

    #[test]
    fn missing_prompt_placeholder_is_skipped() {
        let styles = build_styles(&[custom("bad", "No placeholder", "just a string")]);
        assert!(styles.iter().all(|s| s.name != "bad"));
    }
}
