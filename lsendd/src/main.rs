mod daemon;
mod discovery;
mod identity;
mod ipc_server;
mod pending;
mod policy;
mod receive;
mod send;
mod server;
mod target;

use clap::Parser;
use daemon::Daemon;
use localsend::multicast::DEFAULT_PORT;
use policy::AcceptPolicy;

#[derive(Parser)]
#[command(name = "lsendd")]
struct Args {
    /// Shell command run for each received text message, with the text on
    /// its stdin. The default prints it, ending with a newline so the next
    /// log line starts on its own.
    #[arg(long, default_value = "cat; echo")]
    on_text: String,

    /// Which requests are accepted automatically. The others wait for
    /// `lsendctl accept` or `decline`, and are declined after 60 s. `known`
    /// accepts the fingerprints listed in $XDG_CONFIG_HOME/lsend/known, one
    /// per line.
    #[arg(long, value_enum, default_value_t = AcceptPolicy::Known)]
    accept: AcceptPolicy,
}

/// The hostname, used as the default device alias.
fn default_alias() -> String {
    gethostname::gethostname()
        .to_string_lossy()
        .trim_end_matches(".local")
        .to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut daemon =
        Daemon::start(default_alias(), DEFAULT_PORT, args.on_text, args.accept).await?;
    let result = daemon.run().await;
    daemon.shutdown().await;
    result
}
