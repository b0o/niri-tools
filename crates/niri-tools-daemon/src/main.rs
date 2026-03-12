#[allow(dead_code)]
mod events;
mod scratchpad;
#[allow(dead_code)]
mod server;
mod state;

#[allow(unreachable_code, unused_variables, clippy::diverging_sub_expression)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Phase 7 will provide real implementations.
    // For now, create stub implementations so the binary compiles.
    let niri: Box<dyn niri_tools_common::traits::NiriClient> = todo!("Phase 7: real NiriClient");
    let notifier: Box<dyn niri_tools_common::traits::Notifier> = todo!("Phase 7: real Notifier");

    let mut server = server::DaemonServer::new(niri, notifier);
    server.start().await?;

    Ok(())
}
