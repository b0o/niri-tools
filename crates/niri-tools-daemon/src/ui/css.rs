use niri_tools_common::config::UiConfig;
use niri_tools_common::niri_config::NiriStyleHints;

/// Generate CSS from a resolved `UiConfig` with niri style hints as fallbacks.
///
/// Resolution order: `ui.modes > ui (global) > niri config > built-in defaults`
///
/// CSS classes:
/// - `.mode-container` - the horizontal container for mode binds
/// - `.mode-column` - a vertical column of bind entries
/// - `.mode-key` - the key label (e.g., "b")
/// - `.mode-sep` - the separator between key and description
/// - `.mode-desc` - the description label
/// - `.mode-desc-mode` - accent class for switch-mode entries
pub fn generate_css(config: &UiConfig, hints: &NiriStyleHints) -> String {
    let font = config
        .modes
        .font
        .as_deref()
        .or(config.font.as_deref())
        .unwrap_or("monospace 12");

    let bg = config
        .modes
        .background_color
        .as_deref()
        .or(config.background_color.as_deref())
        .unwrap_or("#2F2A4C");

    let fg = config
        .modes
        .color
        .as_deref()
        .or(config.color.as_deref())
        .unwrap_or("#DFD9FB");

    let radius = config
        .modes
        .corner_radius
        .or(config.corner_radius)
        .unwrap_or(2.0);

    let padding = config.modes.padding.unwrap_or(4.0);

    // Accent color: modes ui > global ui > niri config > hardcoded default
    let accent = hints.accent_color.as_deref().unwrap_or("#8ec07c");

    let border_width = config
        .modes
        .border_width
        .or(hints.border_width)
        .unwrap_or(0.0);

    let border_css = if border_width > 0.0 {
        format!("border: {border_width}px solid {accent};")
    } else {
        String::new()
    };

    format!(
        r#"window {{
    background-color: transparent;
}}

.mode-container {{
    background-color: {bg};
    border-radius: {radius}px;
    padding: {padding}px;
    {border_css}
}}

.mode-column {{
    padding: 0;
}}

.mode-key {{
    font: {font};
    color: {fg};
    font-weight: bold;
}}

.mode-sep {{
    font: {font};
    color: {fg};
    opacity: 0.5;
}}

.mode-desc {{
    font: {font};
    color: {fg};
}}

.mode-desc-mode {{
    color: {accent};
}}

.state-visible {{
    color: {accent};
}}

.state-floating {{
    color: {accent};
}}

.state-unspawned {{
    opacity: 0.5;
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_css_with_defaults() {
        let config = UiConfig::default();
        let hints = NiriStyleHints::default();
        let css = generate_css(&config, &hints);
        assert!(css.contains("background-color: transparent"));
        assert!(css.contains("#2F2A4C"));
        assert!(css.contains("#DFD9FB"));
        assert!(css.contains("monospace 12"));
        assert!(css.contains("#8ec07c")); // default accent
    }

    #[test]
    fn generate_css_respects_config() {
        let mut config = UiConfig::default();
        config.background_color = Some("#000000".to_string());
        config.color = Some("#ffffff".to_string());
        config.font = Some("Mono 14".to_string());
        config.corner_radius = Some(8.0);
        let hints = NiriStyleHints::default();
        let css = generate_css(&config, &hints);
        assert!(css.contains("#000000"));
        assert!(css.contains("#ffffff"));
        assert!(css.contains("Mono 14"));
        assert!(css.contains("8px"));
    }

    #[test]
    fn generate_css_uses_niri_accent_color() {
        let config = UiConfig::default();
        let hints = NiriStyleHints {
            accent_color: Some("#ff00ff".to_string()),
            border_width: Some(2.0),
        };
        let css = generate_css(&config, &hints);
        assert!(css.contains("#ff00ff")); // accent used
        assert!(css.contains("border: 2px solid #ff00ff")); // border
    }

    #[test]
    fn generate_css_modes_override_global() {
        let mut config = UiConfig::default();
        config.font = Some("Global 10".to_string());
        config.modes.font = Some("ModeFont 14".to_string());
        let hints = NiriStyleHints::default();
        let css = generate_css(&config, &hints);
        assert!(css.contains("ModeFont 14"));
        assert!(!css.contains("Global 10"));
    }
}
