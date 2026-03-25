use niri_tools_common::config::UiConfig;

/// Generate CSS from a resolved `UiConfig`.
///
/// CSS classes:
/// - `.mode-container` - the horizontal container for mode binds
/// - `.mode-column` - a vertical column of bind entries
/// - `.mode-key` - the key label (e.g., "b")
/// - `.mode-sep` - the separator between key and description
/// - `.mode-desc` - the description label
/// - `.mode-desc-mode` - accent class for switch-mode entries
pub fn generate_css(config: &UiConfig) -> String {
    let font = config
        .font
        .as_deref()
        .or(config.modes.font.as_deref())
        .unwrap_or("monospace 12");

    let bg = config
        .background_color
        .as_deref()
        .or(config.modes.background_color.as_deref())
        .unwrap_or("#2F2A4C");

    let fg = config
        .color
        .as_deref()
        .or(config.modes.color.as_deref())
        .unwrap_or("#DFD9FB");

    let radius = config
        .corner_radius
        .or(config.modes.corner_radius)
        .unwrap_or(2.0);

    let padding = config.modes.padding.unwrap_or(4.0);

    format!(
        r#"window {{
    background-color: transparent;
}}

.mode-container {{
    background-color: {bg};
    border-radius: {radius}px;
    padding: {padding}px;
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
    color: #8ec07c;
}}

.state-visible {{
    color: #8ec07c;
}}

.state-floating {{
    color: #8ec07c;
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
        let css = generate_css(&config);
        assert!(css.contains("background-color: transparent"));
        assert!(css.contains("#2F2A4C"));
        assert!(css.contains("#DFD9FB"));
        assert!(css.contains("monospace 12"));
    }

    #[test]
    fn generate_css_respects_config() {
        let mut config = UiConfig::default();
        config.background_color = Some("#000000".to_string());
        config.color = Some("#ffffff".to_string());
        config.font = Some("Mono 14".to_string());
        config.corner_radius = Some(8.0);
        let css = generate_css(&config);
        assert!(css.contains("#000000"));
        assert!(css.contains("#ffffff"));
        assert!(css.contains("Mono 14"));
        assert!(css.contains("8px"));
    }
}
