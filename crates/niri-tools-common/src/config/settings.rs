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
    fn notify_level_variants() {
        assert_ne!(NotifyLevel::None, NotifyLevel::Error);
        assert_ne!(NotifyLevel::Error, NotifyLevel::Warning);
        assert_ne!(NotifyLevel::Warning, NotifyLevel::All);
        assert_ne!(NotifyLevel::None, NotifyLevel::All);
    }
}
