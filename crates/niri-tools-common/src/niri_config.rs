//! Parse niri's config.kdl to extract style properties for fallback values.
//!
//! This is a best-effort parser -- it reads a small subset of niri's config
//! to provide sensible defaults for the mode overlay and scratchpad picker.

use std::path::Path;

/// Style properties extracted from niri's config.
#[derive(Debug, Clone, Default)]
pub struct NiriStyleHints {
    /// Active border/ring color (from `layout.border.active-color` or gradient `from`)
    pub accent_color: Option<String>,
    /// Border width (from `layout.border.width`)
    pub border_width: Option<f64>,
}

/// Read niri's config.kdl and extract style hints.
///
/// Returns default hints if the config cannot be read or parsed.
pub fn read_niri_style_hints() -> NiriStyleHints {
    let config_path = niri_config_path();
    if !config_path.exists() {
        return NiriStyleHints::default();
    }

    match std::fs::read_to_string(&config_path) {
        Ok(content) => parse_style_hints(&content),
        Err(_) => NiriStyleHints::default(),
    }
}

fn niri_config_path() -> std::path::PathBuf {
    let config_home = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.config")
    });
    Path::new(&config_home).join("niri/config.kdl")
}

fn parse_style_hints(content: &str) -> NiriStyleHints {
    let doc: kdl::KdlDocument = match content.parse() {
        Ok(d) => d,
        Err(_) => return NiriStyleHints::default(),
    };

    let mut hints = NiriStyleHints::default();

    // Look for top-level `layout { ... }` node
    if let Some(layout) = doc.get("layout") {
        if let Some(children) = layout.children() {
            // Try border first, then focus-ring
            for block_name in &["border", "focus-ring"] {
                if let Some(block) = children.get(block_name) {
                    if let Some(block_children) = block.children() {
                        // Width
                        if hints.border_width.is_none() {
                            if let Some(width_val) = block_children.get_arg("width") {
                                if let Some(w) = width_val.as_i64() {
                                    hints.border_width = Some(w as f64);
                                } else if let Some(w) = width_val.as_f64() {
                                    hints.border_width = Some(w);
                                }
                            }
                        }

                        // Active color
                        if hints.accent_color.is_none() {
                            // Try active-color first
                            if let Some(color_val) = block_children.get_arg("active-color") {
                                if let Some(c) = color_val.as_string() {
                                    hints.accent_color = Some(c.to_string());
                                }
                            }

                            // Then try active-gradient (extract `from` color)
                            if hints.accent_color.is_none() {
                                if let Some(grad_node) = block_children.get("active-gradient") {
                                    if let Some(from_val) = grad_node.get("from") {
                                        if let Some(c) = from_val.value().as_string() {
                                            hints.accent_color = Some(c.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_border_with_active_color() {
        let content = r##"
layout {
    border {
        width 2
        active-color "#ff0000"
    }
}
"##;
        let hints = parse_style_hints(content);
        assert_eq!(hints.accent_color.as_deref(), Some("#ff0000"));
        assert_eq!(hints.border_width, Some(2.0));
    }

    #[test]
    fn parse_border_with_gradient() {
        let content = r##"
layout {
    border {
        width 1
        active-gradient from="#9074ff" to="#6627ff" angle=45
    }
}
"##;
        let hints = parse_style_hints(content);
        assert_eq!(hints.accent_color.as_deref(), Some("#9074ff"));
        assert_eq!(hints.border_width, Some(1.0));
    }

    #[test]
    fn parse_focus_ring_fallback() {
        let content = r##"
layout {
    focus-ring {
        width 3
        active-color "#00ff00"
    }
}
"##;
        let hints = parse_style_hints(content);
        assert_eq!(hints.accent_color.as_deref(), Some("#00ff00"));
        assert_eq!(hints.border_width, Some(3.0));
    }

    #[test]
    fn parse_empty_config() {
        let hints = parse_style_hints("");
        assert!(hints.accent_color.is_none());
        assert!(hints.border_width.is_none());
    }

    #[test]
    fn parse_no_layout() {
        let content = r#"input { keyboard { numlock; } }"#;
        let hints = parse_style_hints(content);
        assert!(hints.accent_color.is_none());
        assert!(hints.border_width.is_none());
    }
}
