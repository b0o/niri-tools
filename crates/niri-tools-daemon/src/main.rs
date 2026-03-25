mod events;
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

        // Create the UI manager (owns GTK windows, starts hidden).
        let _ui_manager = ui::UiManager::new(app);

        // Spawn the daemon's tokio event loop on the background runtime.
        // UI commands will be forwarded to the GTK thread via glib channels (Phase 2.2+).
        runtime().spawn(async move {
            let niri_client: Box<dyn niri_tools_common::traits::NiriClient> =
                Box::new(RealNiriClient);
            let notifier: Box<dyn niri_tools_common::traits::Notifier> =
                Box::new(RealNotifier::new(NotifyLevel::All));

            let mut server = server::DaemonServer::new(niri_client, notifier);
            if let Err(e) = server.start().await {
                tracing::error!(%e, "daemon server error");
            }

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
