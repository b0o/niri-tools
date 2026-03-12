use std::path::PathBuf;

pub fn socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("NIRI_TOOLS_SOCKET") {
        return PathBuf::from(path);
    }
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("niri-tools.sock")
}

pub fn default_config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.config")
    });
    PathBuf::from(config_dir)
        .join("niri")
        .join("scratchpads.kdl")
}

pub fn state_file_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("niri-tools-state.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_uses_xdg_runtime_dir() {
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000") };
        let path = socket_path();
        assert_eq!(path, PathBuf::from("/run/user/1000/niri-tools.sock"));
    }

    #[test]
    fn socket_path_falls_back_to_tmp() {
        unsafe { std::env::remove_var("XDG_RUNTIME_DIR") };
        let path = socket_path();
        assert_eq!(path, PathBuf::from("/tmp/niri-tools.sock"));
    }

    #[test]
    fn state_file_path_uses_xdg_runtime_dir() {
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000") };
        let path = state_file_path();
        assert_eq!(path, PathBuf::from("/run/user/1000/niri-tools-state.json"));
    }

    #[test]
    fn state_file_path_falls_back_to_tmp() {
        unsafe { std::env::remove_var("XDG_RUNTIME_DIR") };
        let path = state_file_path();
        assert_eq!(path, PathBuf::from("/tmp/niri-tools-state.json"));
    }
}
