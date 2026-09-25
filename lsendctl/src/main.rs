use clap::{Parser, Subcommand};
use ipc::{DeviceEntry, Request, Response, read_message, socket_path, write_message};
use std::path::PathBuf;
use tokio::io::BufReader;
use tokio::net::UnixStream;

#[derive(Parser)]
#[command(name = "lsendctl")]
struct Cli {
    /// Print the daemon's response as a single line of JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the daemon's identity and status.
    Status,
    /// List reachable devices, one per line: alias, address, type
    /// (tab-separated). Prints nothing when there are none.
    List {
        /// Append each device's fingerprint as a last column.
        #[arg(long)]
        fingerprint: bool,
    },
    /// Send files or directories to a device, by alias or IP address.
    Send {
        /// Destination alias or IP address.
        #[arg(long = "to")]
        to: String,
        /// Files or directories (sent recursively) to send.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    let (request, show_fingerprint) = match cli.command {
        Command::Status => (Request::Status, false),
        Command::List { fingerprint } => (Request::List, fingerprint),
        Command::Send { to, paths } => (Request::Send { target: to, paths }, false),
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
        Response::Error { message } => anyhow::bail!("lsendd error: {message}"),
        response if json => println!("{}", serde_json::to_string(&response)?),
        Response::Status(status) => {
            println!("alias: {}", status.alias);
            println!("fingerprint: {}", status.fingerprint);
            println!("port: {}", status.port);
        }
        Response::List { devices } => print_devices(&devices, show_fingerprint),
        Response::Send { sent_files } => println!("Sent {sent_files} file(s)"),
    }

    Ok(())
}

fn print_devices(devices: &[DeviceEntry], show_fingerprint: bool) {
    for device in devices {
        let device_type = device
            .device_type
            .as_ref()
            .map(|t| t.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        if show_fingerprint {
            println!(
                "{}\t{}\t{}\t{}",
                device.alias, device.address, device_type, device.fingerprint
            );
        } else {
            println!("{}\t{}\t{}", device.alias, device.address, device_type);
        }
    }
}
