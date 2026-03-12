use niri_tools_common::config::NotifyLevel;
use niri_tools_common::traits::Notifier;

pub struct RealNotifier {
    level: NotifyLevel,
    has_dms: bool,
}

impl RealNotifier {
    pub fn new(level: NotifyLevel) -> Self {
        // Check if `dms` is available on PATH
        let has_dms = std::process::Command::new("which")
            .arg("dms")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());

        Self { level, has_dms }
    }

    fn send_notification(
        &self,
        title: &str,
        message: &str,
        urgency: &str,
        timeout_ms: Option<u32>,
        dms_level: Option<&str>,
    ) {
        if self.has_dms {
            if let Some(dms_level) = dms_level {
                let _ = std::process::Command::new("dms")
                    .args([
                        "ipc",
                        "toast",
                        dms_level,
                        &format!("niri-tools: {title}"),
                        message,
                        "",
                        "",
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                return;
            }
        }

        let mut cmd = std::process::Command::new("notify-send");
        cmd.args(["-a", "niri-tools", "-u", urgency]);
        if let Some(t) = timeout_ms {
            cmd.args(["-t", &t.to_string()]);
        }
        cmd.args([title, message]);
        let _ = cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

impl Notifier for RealNotifier {
    fn notify_error(&self, title: &str, message: &str) {
        if self.level >= NotifyLevel::Error {
            self.send_notification(title, message, "critical", None, Some("errorWith"));
        }
    }

    fn notify_warning(&self, title: &str, message: &str) {
        if self.level >= NotifyLevel::Warning {
            self.send_notification(title, message, "normal", Some(5000), Some("warnWith"));
        }
    }

    fn notify_info(&self, title: &str, message: &str) {
        if self.level >= NotifyLevel::All {
            self.send_notification(title, message, "low", Some(2000), Some("infoWith"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_level_ordering() {
        assert!(NotifyLevel::None < NotifyLevel::Error);
        assert!(NotifyLevel::Error < NotifyLevel::Warning);
        assert!(NotifyLevel::Warning < NotifyLevel::All);
    }

    #[test]
    fn notify_level_gte_comparison() {
        // All >= Error
        assert!(NotifyLevel::All >= NotifyLevel::Error);
        // Error >= Error
        assert!(NotifyLevel::Error >= NotifyLevel::Error);
        // None < Error
        assert!(!(NotifyLevel::None >= NotifyLevel::Error));
    }

    /// Test that RealNotifier respects level thresholds by verifying
    /// the level field is stored correctly.
    #[test]
    fn real_notifier_stores_level() {
        let notifier = RealNotifier {
            level: NotifyLevel::Warning,
            has_dms: false,
        };
        assert_eq!(notifier.level, NotifyLevel::Warning);
    }

    /// Verify notify_error does not panic at any level.
    #[test]
    fn notify_methods_do_not_panic() {
        // Use level None so no external commands are actually called
        let notifier = RealNotifier {
            level: NotifyLevel::None,
            has_dms: false,
        };
        notifier.notify_error("test", "msg");
        notifier.notify_warning("test", "msg");
        notifier.notify_info("test", "msg");
    }

    /// Verify that at NotifyLevel::Error, only errors pass the filter.
    #[test]
    fn level_error_filters_correctly() {
        // We can't easily test that commands are spawned without mocking,
        // but we can verify the comparison logic inline.
        let level = NotifyLevel::Error;
        assert!(level >= NotifyLevel::Error); // error: pass
        assert!(!(level >= NotifyLevel::Warning)); // warning: block
        assert!(!(level >= NotifyLevel::All)); // info: block
    }

    /// Verify that at NotifyLevel::Warning, errors and warnings pass.
    #[test]
    fn level_warning_filters_correctly() {
        let level = NotifyLevel::Warning;
        assert!(level >= NotifyLevel::Error); // error: pass
        assert!(level >= NotifyLevel::Warning); // warning: pass
        assert!(!(level >= NotifyLevel::All)); // info: block
    }

    /// Verify that at NotifyLevel::All, everything passes.
    #[test]
    fn level_all_filters_correctly() {
        let level = NotifyLevel::All;
        assert!(level >= NotifyLevel::Error); // error: pass
        assert!(level >= NotifyLevel::Warning); // warning: pass
        assert!(level >= NotifyLevel::All); // info: pass
    }

    /// Verify that at NotifyLevel::None, nothing passes.
    #[test]
    fn level_none_filters_correctly() {
        let level = NotifyLevel::None;
        assert!(!(level >= NotifyLevel::Error)); // error: block
        assert!(!(level >= NotifyLevel::Warning)); // warning: block
        assert!(!(level >= NotifyLevel::All)); // info: block
    }
}
