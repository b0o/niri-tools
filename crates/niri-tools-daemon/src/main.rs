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
    let niri_client: Box<dyn niri_tools_common::traits::NiriClient> =
        Box::new(RealNiriClient);
    let notifier: Box<dyn niri_tools_common::traits::Notifier> =
        Box::new(RealNotifier::new(NotifyLevel::All));

    let mut server = server::DaemonServer::new(niri_client, notifier);
    server.start().await?;

    Ok(())
}
