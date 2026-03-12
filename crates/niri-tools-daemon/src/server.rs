use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

use niri_tools_common::config_parser::load_config;
use niri_tools_common::paths::socket_path;
use niri_tools_common::protocol::{Command, Response};
use niri_tools_common::traits::{NiriClient, Notifier};

use crate::events::{apply_event, EventAction};
use crate::scratchpad::ScratchpadManager;
use crate::state::DaemonState;

/// Maximum message size for client communication (16 MiB).
const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

pub struct DaemonServer {
    state: DaemonState,
    niri: Box<dyn NiriClient>,
    notifier: Box<dyn Notifier>,
    running: bool,
}

impl DaemonServer {
    pub fn new(niri: Box<dyn NiriClient>, notifier: Box<dyn Notifier>) -> Self {
        Self {
            state: DaemonState::default(),
            niri,
            notifier,
            running: false,
        }
    }

    /// Start the daemon: initialise state, load config, run the main loop.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        self.running = true;

        // Load initial config
        self.reload_config(false);

        // Populate initial state from niri
        self.initialize_state().await?;

        // Load persisted scratchpad state
        self.state.load_scratchpad_state();

        // Remove stale socket
        let sock_path = socket_path();
        if sock_path.exists() {
            let _ = std::fs::remove_file(&sock_path);
        }

        // Bind socket
        let listener = UnixListener::bind(&sock_path)?;

        // Main event loop
        self.run_loop(listener).await?;

        // Cleanup
        let _ = std::fs::remove_file(&sock_path);

        Ok(())
    }

    /// Populate initial state from niri queries.
    async fn initialize_state(&mut self) -> anyhow::Result<()> {
        // Get windows
        let windows = self.niri.get_windows().await?;
        for w in windows {
            if w.is_focused {
                self.state.focused_window_id = Some(w.id);
            }
            self.state.windows.insert(w.id, w);
        }

        // Get workspaces
        let workspaces = self.niri.get_workspaces().await?;
        for ws in workspaces {
            self.state.workspaces.insert(ws.id, ws);
        }

        // Get outputs
        self.state.outputs = self.niri.get_outputs().await?;

        // Get focused output
        if let Ok(output) = self.niri.get_focused_output().await {
            self.state.focused_output = Some(output);
        }

        Ok(())
    }

    /// Main event loop: wait for client connections, niri events, or signals.
    async fn run_loop(&mut self, listener: UnixListener) -> anyhow::Result<()> {
        use futures_core::Stream;
        use futures_util::StreamExt;
        use std::pin::Pin;
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;

        // Subscribe to niri events
        let mut event_stream: Option<
            Pin<Box<dyn Stream<Item = niri_tools_common::Result<niri_tools_common::types::NiriEvent>> + Send>>,
        > = match self.niri.subscribe_events().await {
            Ok(stream) => Some(stream),
            Err(e) => {
                self.notifier
                    .notify_warning("Event Stream", &format!("Failed to subscribe: {e}"));
                None
            }
        };

        while self.running {
            tokio::select! {
                // Accept client connection
                result = listener.accept() => {
                    match result {
                        Ok((stream, _addr)) => {
                            self.handle_client_connection(stream).await;
                        }
                        Err(e) => {
                            self.notifier.notify_error("Socket", &format!("Accept failed: {e}"));
                        }
                    }
                }

                // Niri event
                event = async {
                    if let Some(ref mut stream) = event_stream {
                        stream.next().await
                    } else {
                        // No stream, sleep forever
                        std::future::pending::<Option<niri_tools_common::Result<niri_tools_common::types::NiriEvent>>>().await
                    }
                } => {
                    match event {
                        Some(Ok(niri_event)) => {
                            self.handle_niri_event(&niri_event).await;
                        }
                        Some(Err(e)) => {
                            self.notifier.notify_warning("Event Stream", &format!("Event error: {e}"));
                        }
                        None => {
                            // Stream ended, try to reconnect
                            self.notifier.notify_warning("Event Stream", "Stream ended, attempting reconnect");
                            event_stream = match self.niri.subscribe_events().await {
                                Ok(stream) => Some(stream),
                                Err(e) => {
                                    self.notifier.notify_error("Event Stream", &format!("Reconnect failed: {e}"));
                                    None
                                }
                            };
                        }
                    }
                }

                // Signals
                _ = sigterm.recv() => {
                    self.running = false;
                }
                _ = sigint.recv() => {
                    self.running = false;
                }
            }
        }

        Ok(())
    }

    /// Handle a single client connection: read command, dispatch, respond.
    async fn handle_client_connection(&mut self, mut stream: tokio::net::UnixStream) {
        // Read length-prefixed command
        let command = match read_async_message::<Command>(&mut stream).await {
            Ok(cmd) => cmd,
            Err(e) => {
                let _ = write_async_message(
                    &mut stream,
                    &Response::Error(format!("Failed to read command: {e}")),
                )
                .await;
                return;
            }
        };

        let response = self.dispatch_command(command).await;

        let _ = write_async_message(&mut stream, &response).await;
    }

    /// Route a command to the appropriate handler.
    pub async fn dispatch_command(&mut self, command: Command) -> Response {
        match command {
            Command::DaemonStatus => self.get_status(),

            Command::DaemonStop => {
                self.running = false;
                Response::Ok
            }

            Command::DaemonRestart => {
                // Reload config
                self.reload_config(true);
                Response::Ok
            }

            Command::Toggle { name } => {
                let mut mgr = ScratchpadManager::new(&mut self.state, self.niri.as_ref());
                match name {
                    Some(ref n) => match mgr.toggle(n).await {
                        Ok(()) => Response::Ok,
                        Err(e) => Response::Error(e.to_string()),
                    },
                    None => match mgr.smart_toggle().await {
                        Ok(()) => Response::Ok,
                        Err(e) => Response::Error(e.to_string()),
                    },
                }
            }

            Command::Hide => {
                let mut mgr = ScratchpadManager::new(&mut self.state, self.niri.as_ref());
                match mgr.hide().await {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                }
            }

            Command::ToggleFloat { name } => {
                let mut mgr = ScratchpadManager::new(&mut self.state, self.niri.as_ref());
                match mgr.toggle_float(name.as_deref()).await {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                }
            }

            Command::Float { name } => {
                let mut mgr = ScratchpadManager::new(&mut self.state, self.niri.as_ref());
                match mgr.float_scratchpad(name.as_deref()).await {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                }
            }

            Command::Tile { name } => {
                let mut mgr = ScratchpadManager::new(&mut self.state, self.niri.as_ref());
                match mgr.tile_scratchpad(name.as_deref()).await {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                }
            }
        }
    }

    /// Handle a niri event: parse, apply, and perform follow-up actions.
    async fn handle_niri_event(&mut self, event: &niri_tools_common::types::NiriEvent) {
        let action = apply_event(&mut self.state, event);

        match action {
            EventAction::None => {}

            EventAction::WindowOpened(window) => {
                let mut mgr = ScratchpadManager::new(&mut self.state, self.niri.as_ref());
                if let Err(e) = mgr.handle_window_opened(&window).await {
                    self.notifier
                        .notify_warning("Scratchpad", &format!("Window opened error: {e}"));
                }
            }

            EventAction::Reconcile(window_ids) => {
                self.state.reconcile_with_windows(&window_ids);
                let _ = self.state.save_scratchpad_state();
            }

            EventAction::SaveState => {
                let _ = self.state.save_scratchpad_state();
            }

            EventAction::ReloadWorkspaces => {
                match self.niri.get_workspaces().await {
                    Ok(workspaces) => {
                        self.state.workspaces.clear();
                        for ws in workspaces {
                            self.state.workspaces.insert(ws.id, ws);
                        }
                    }
                    Err(e) => {
                        self.notifier.notify_warning(
                            "Workspaces",
                            &format!("Failed to reload: {e}"),
                        );
                    }
                }
            }
        }
    }

    /// Load (or reload) KDL config. Updates state with new scratchpad configs.
    pub fn reload_config(&mut self, is_reload: bool) {
        let loaded = match load_config(None) {
            Ok(cfg) => cfg,
            Err(e) => {
                let msg = format!("Config error: {e}");
                if is_reload {
                    self.notifier.notify_error("Config", &msg);
                }
                return;
            }
        };

        // Apply warnings
        for warning in &loaded.warnings {
            self.notifier.notify_warning("Config", warning);
        }

        // Update scratchpad configs
        self.state.scratchpad_configs = loaded.scratchpads;

        // Update config file list and watch setting
        self.state.config_files = loaded.config_files.into_iter().collect();
        self.state.watch_config = loaded.settings.watch_config;

        if is_reload {
            self.notifier.notify_info("Config", "Configuration reloaded");
        }
    }

    /// Build a status response with current process info.
    pub fn get_status(&self) -> Response {
        let pid = std::process::id();
        let cmdline = std::env::args().collect::<Vec<_>>().join(" ");

        // Get parent PID and cmdline via /proc
        let ppid = get_ppid(pid);
        let parent_cmdline = ppid
            .map(get_process_cmdline)
            .unwrap_or_default();

        Response::Status {
            pid,
            cmdline,
            ppid: ppid.unwrap_or(0),
            parent_cmdline,
            socket: socket_path().to_string_lossy().to_string(),
        }
    }
}

/// Read a length-prefixed bincode message from an async Unix stream.
async fn read_async_message<T: serde::de::DeserializeOwned>(
    stream: &mut tokio::net::UnixStream,
) -> anyhow::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_MESSAGE_SIZE {
        anyhow::bail!("message too large: {len}");
    }
    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload).await?;
    let (msg, _) = bincode::serde::decode_from_slice(&payload, bincode::config::standard())
        .map_err(|e| anyhow::anyhow!("decode error: {e}"))?;
    Ok(msg)
}

/// Write a length-prefixed bincode message to an async Unix stream.
async fn write_async_message<T: serde::Serialize>(
    stream: &mut tokio::net::UnixStream,
    msg: &T,
) -> anyhow::Result<()> {
    let payload = bincode::serde::encode_to_vec(msg, bincode::config::standard())
        .map_err(|e| anyhow::anyhow!("encode error: {e}"))?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&payload).await?;
    Ok(())
}

/// Get the parent PID of a process (Linux-specific, via /proc).
fn get_ppid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

/// Get the command line of a process (Linux-specific, via /proc).
fn get_process_cmdline(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .unwrap_or_default()
        .replace('\0', " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use futures_core::Stream;
    use niri_tools_common::types::{NiriEvent, OutputInfo, WindowInfo, WorkspaceInfo};

    // -- Mock NiriClient --

    #[derive(Debug, Default, Clone)]
    struct MockActions {
        calls: Vec<(String, Vec<String>)>,
    }

    struct MockNiriClient {
        actions: Arc<Mutex<MockActions>>,
        windows: Vec<WindowInfo>,
        workspaces: Vec<WorkspaceInfo>,
        outputs: HashMap<String, OutputInfo>,
        focused_output: String,
    }

    impl MockNiriClient {
        fn new() -> Self {
            Self {
                actions: Arc::new(Mutex::new(MockActions::default())),
                windows: vec![],
                workspaces: vec![],
                outputs: HashMap::new(),
                focused_output: "eDP-1".to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl NiriClient for MockNiriClient {
        async fn run_action(&self, action: &str, args: &[&str]) -> niri_tools_common::Result<()> {
            self.actions.lock().unwrap().calls.push((
                action.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
            Ok(())
        }

        async fn get_windows(&self) -> niri_tools_common::Result<Vec<WindowInfo>> {
            Ok(self.windows.clone())
        }

        async fn get_workspaces(&self) -> niri_tools_common::Result<Vec<WorkspaceInfo>> {
            Ok(self.workspaces.clone())
        }

        async fn get_outputs(&self) -> niri_tools_common::Result<HashMap<String, OutputInfo>> {
            Ok(self.outputs.clone())
        }

        async fn get_focused_output(&self) -> niri_tools_common::Result<String> {
            Ok(self.focused_output.clone())
        }

        async fn subscribe_events(
            &self,
        ) -> niri_tools_common::Result<
            Pin<Box<dyn Stream<Item = niri_tools_common::Result<NiriEvent>> + Send>>,
        > {
            unimplemented!("not needed for unit tests")
        }
    }

    // -- Mock Notifier --

    #[derive(Default)]
    struct MockNotifier {
        messages: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    impl Notifier for MockNotifier {
        fn notify_error(&self, title: &str, message: &str) {
            self.messages
                .lock()
                .unwrap()
                .push(("error".to_string(), title.to_string(), message.to_string()));
        }

        fn notify_warning(&self, title: &str, message: &str) {
            self.messages.lock().unwrap().push((
                "warning".to_string(),
                title.to_string(),
                message.to_string(),
            ));
        }

        fn notify_info(&self, title: &str, message: &str) {
            self.messages
                .lock()
                .unwrap()
                .push(("info".to_string(), title.to_string(), message.to_string()));
        }
    }

    fn make_server() -> DaemonServer {
        DaemonServer::new(
            Box::new(MockNiriClient::new()),
            Box::new(MockNotifier::default()),
        )
    }

    // -- dispatch_command tests --

    #[tokio::test]
    async fn dispatch_daemon_status_returns_status() {
        let mut server = make_server();
        let response = server.dispatch_command(Command::DaemonStatus).await;

        match response {
            Response::Status {
                pid,
                cmdline,
                ppid: _,
                parent_cmdline: _,
                socket,
            } => {
                assert_eq!(pid, std::process::id());
                assert!(!cmdline.is_empty());
                assert!(!socket.is_empty());
            }
            other => panic!("Expected Status, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_daemon_stop_sets_running_false() {
        let mut server = make_server();
        server.running = true;

        let response = server.dispatch_command(Command::DaemonStop).await;
        assert_eq!(response, Response::Ok);
        assert!(!server.running);
    }

    #[tokio::test]
    async fn dispatch_daemon_restart_reloads_config() {
        let mut server = make_server();
        let response = server.dispatch_command(Command::DaemonRestart).await;
        assert_eq!(response, Response::Ok);
    }

    #[tokio::test]
    async fn dispatch_toggle_with_no_config_returns_error() {
        let mut server = make_server();
        let response = server
            .dispatch_command(Command::Toggle {
                name: Some("nonexistent".to_string()),
            })
            .await;

        match response {
            Response::Error(msg) => {
                assert!(msg.contains("nonexistent"));
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_hide_returns_ok() {
        let mut server = make_server();
        // No focused window, so hide is a no-op
        let response = server.dispatch_command(Command::Hide).await;
        assert_eq!(response, Response::Ok);
    }

    // -- get_status tests --

    #[test]
    fn get_status_returns_valid_pid() {
        let server = make_server();
        match server.get_status() {
            Response::Status { pid, .. } => {
                assert_eq!(pid, std::process::id());
            }
            other => panic!("Expected Status, got {other:?}"),
        }
    }

    // -- get_ppid tests --

    #[test]
    fn get_ppid_for_current_process() {
        let pid = std::process::id();
        let ppid = get_ppid(pid);
        // Current process should have a parent
        assert!(ppid.is_some());
        assert!(ppid.unwrap() > 0);
    }

    #[test]
    fn get_ppid_for_nonexistent_process() {
        let ppid = get_ppid(u32::MAX);
        assert!(ppid.is_none());
    }

    // -- get_process_cmdline tests --

    #[test]
    fn get_process_cmdline_for_current() {
        let pid = std::process::id();
        let cmdline = get_process_cmdline(pid);
        // Should contain something (the test runner binary)
        assert!(!cmdline.is_empty());
    }
}
