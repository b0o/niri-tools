mod events;
mod niri;
mod notify;
mod scratchpad;
mod server;
mod state;

use niri::RealNiriClient;
use niri_tools_common::config::NotifyLevel;
use notify::RealNotifier;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "niri_tools_daemon=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("niri-tools-daemon starting");

    let niri_client: Box<dyn niri_tools_common::traits::NiriClient> =
        Box::new(RealNiriClient);
    let notifier: Box<dyn niri_tools_common::traits::Notifier> =
        Box::new(RealNotifier::new(NotifyLevel::All));

    let mut server = server::DaemonServer::new(niri_client, notifier);
    server.start().await?;

    Ok(())
}
