mod daemon;
mod discovery;
mod identity;
mod ipc_server;
mod receive;
mod send;
mod server;
mod target;

use daemon::Daemon;
use localsend::multicast::DEFAULT_PORT;

/// The hostname, used as the default device alias.
fn default_alias() -> String {
    gethostname::gethostname()
        .to_string_lossy()
        .trim_end_matches(".local")
        .to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut daemon = Daemon::start(default_alias(), DEFAULT_PORT).await?;
    daemon.run().await;
    daemon.shutdown().await;
    Ok(())
}
