use clap::{Parser, Subcommand};
use ipc::{Request, Response, read_message, socket_path, write_message};
use tokio::io::BufReader;
use tokio::net::UnixStream;

#[derive(Parser)]
#[command(name = "lsendctl")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the daemon's identity and status.
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let request = match cli.command {
        Command::Status => Request::Status,
    };

    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path).await.map_err(|err| {
        anyhow::anyhow!("Could not connect to lsendd at {}: {err}", path.display())
    })?;
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);

    write_message(&mut write_half, &request).await?;
    let response = read_message::<_, Response>(&mut reader)
        .await?
        .ok_or_else(|| anyhow::anyhow!("lsendd closed the connection without responding"))?;

    match response {
        Response::Status(status) => {
            println!("alias: {}", status.alias);
            println!("fingerprint: {}", status.fingerprint);
            println!("port: {}", status.port);
        }
        Response::Error { message } => anyhow::bail!("lsendd error: {message}"),
    }

    Ok(())
}
