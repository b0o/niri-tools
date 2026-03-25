mod events;
mod mode;
mod niri;
mod notify;
mod scratchpad;
mod server;
mod state;
mod ui;

use std::sync::OnceLock;

use gtk4::glib;
use gtk4::prelude::*;
use tokio::runtime::Runtime;

use niri::RealNiriClient;
use niri_tools_common::config::NotifyLevel;
use niri_tools_common::config_parser::load_config;
use notify::RealNotifier;

/// Global tokio runtime, accessed from any thread.
pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create tokio runtime"))
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "niri_tools_daemon=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("niri-tools-daemon starting");

    let app = gtk4::Application::builder()
        .application_id("org.niri-tools.daemon")
        .build();

    app.connect_activate(move |app| {
        // Hold the application open (no visible windows at startup).
        // Leak the guard so the hold persists for the process lifetime.
        std::mem::forget(app.hold());

        // Load config for initial UI setup.
        let ui_config = load_config(None)
            .map(|c| c.ui_config)
            .unwrap_or_default();

        // Create channels:
        // ui_tx/ui_rx: tokio → GTK (UI commands like ModeShow)
        // daemon_tx/daemon_rx: GTK → tokio (commands from mode key actions)
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::channel::<ui::UiCommand>(64);
        let (daemon_tx, daemon_rx) =
            tokio::sync::mpsc::channel::<niri_tools_common::protocol::Command>(64);

        // Create the UI manager (owns GTK windows, starts hidden).
        let ui_manager = ui::UiManager::new(app, &ui_config, daemon_tx);

        // Bridge: receive UI commands from tokio and dispatch on the GTK thread.
        glib::spawn_future_local(async move {
            while let Some(cmd) = ui_rx.recv().await {
                ui_manager.handle_command(cmd);
            }
        });

        // Spawn the daemon's tokio event loop on the background runtime.
        runtime().spawn(async move {
            let niri_client: Box<dyn niri_tools_common::traits::NiriClient> =
                Box::new(RealNiriClient);
            let notifier: Box<dyn niri_tools_common::traits::Notifier> =
                Box::new(RealNotifier::new(NotifyLevel::All));

            let mut server = server::DaemonServer::new(niri_client, notifier, ui_tx);

            // Start the server and also listen for reverse commands from the GTK thread
            server.start_with_daemon_rx(daemon_rx).await;

            // Server exited (stop command received or error).
            // Quit the GTK application from the main thread.
            glib::idle_add_once(|| {
                if let Some(app) = gtk4::gio::Application::default() {
                    app.quit();
                }
            });
        });
    });

    let exit_code = app.run_with_args::<&str>(&[]);
    if exit_code != glib::ExitCode::SUCCESS {
        tracing::error!(?exit_code, "GTK application exited with error");
        std::process::exit(1);
    }
}
