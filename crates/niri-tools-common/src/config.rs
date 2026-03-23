use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ScratchpadConfig {
    pub name: String,
    pub command: Option<Vec<String>>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub auto_adopt: bool,
    pub size: Option<SizeConfig>,
    pub position: Option<PositionConfig>,
    pub output_overrides: HashMap<String, OutputOverride>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SizeConfig {
    pub width: String,
    pub height: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionConfig {
    pub x: String,
    pub y: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutputOverride {
    pub size: Option<SizeConfig>,
    pub position: Option<PositionConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotifyLevel {
    None = 0,
    Error = 1,
    Warning = 2,
    All = 3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DaemonSettings {
    pub notify_level: NotifyLevel,
    pub watch_config: bool,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            notify_level: NotifyLevel::All,
            watch_config: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_settings_default_values() {
        let settings = DaemonSettings::default();
        assert_eq!(settings.notify_level, NotifyLevel::All);
        assert!(settings.watch_config);
    }

    #[test]
    fn size_config_construction_and_equality() {
        let a = SizeConfig {
            width: "60%".to_string(),
            height: "600".to_string(),
        };
        let b = SizeConfig {
            width: "60%".to_string(),
            height: "600".to_string(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn position_config_construction_and_equality() {
        let a = PositionConfig {
            x: "10%".to_string(),
            y: "200".to_string(),
        };
        let b = PositionConfig {
            x: "10%".to_string(),
            y: "200".to_string(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn output_override_default_is_empty() {
        let ov = OutputOverride::default();
        assert_eq!(ov.size, None);
        assert_eq!(ov.position, None);
    }

    #[test]
    fn scratchpad_config_construction() {
        let config = ScratchpadConfig {
            name: "term".to_string(),
            command: Some(vec!["foot".to_string()]),
            app_id: Some("foot".to_string()),
            title: None,
            auto_adopt: false,
            size: Some(SizeConfig {
                width: "60%".to_string(),
                height: "60%".to_string(),
            }),
            position: Some(PositionConfig {
                x: "20%".to_string(),
                y: "20%".to_string(),
            }),
            output_overrides: HashMap::new(),
        };
        assert_eq!(config.name, "term");
        assert!(config.command.is_some());
        assert_eq!(config.app_id.as_deref(), Some("foot"));
        assert!(config.title.is_none());
        assert!(config.size.is_some());
        assert!(config.position.is_some());
        assert!(config.output_overrides.is_empty());
    }

    #[test]
    fn scratchpad_config_with_output_overrides() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "eDP-1".to_string(),
            OutputOverride {
                size: Some(SizeConfig {
                    width: "800".to_string(),
                    height: "600".to_string(),
                }),
                position: None,
            },
        );

        let config = ScratchpadConfig {
            name: "browser".to_string(),
            command: None,
            app_id: Some("firefox".to_string()),
            title: None,
            auto_adopt: false,
            size: None,
            position: None,
            output_overrides: overrides,
        };

        assert_eq!(config.output_overrides.len(), 1);
        let edp = &config.output_overrides["eDP-1"];
        assert!(edp.size.is_some());
        assert!(edp.position.is_none());
    }

    #[test]
    fn notify_level_variants() {
        // Ensure all variants are distinct
        assert_ne!(NotifyLevel::None, NotifyLevel::Error);
        assert_ne!(NotifyLevel::Error, NotifyLevel::Warning);
        assert_ne!(NotifyLevel::Warning, NotifyLevel::All);
        assert_ne!(NotifyLevel::None, NotifyLevel::All);
    }
}
