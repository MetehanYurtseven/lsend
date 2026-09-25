mod daemon;
mod discovery;
mod identity;
mod server;

use daemon::Daemon;
use localsend::multicast::DEFAULT_PORT;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut daemon = Daemon::start("lsendd-test".to_string(), DEFAULT_PORT).await?;
    daemon.run().await;
    daemon.shutdown().await;
    Ok(())
}
