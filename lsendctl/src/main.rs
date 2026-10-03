use clap::{Parser, Subcommand};
use ipc::{
    DeviceEntry, PendingEntry, Request, Response, SendPayload, read_message, socket_path,
    write_message,
};
use std::io::Read;
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
    /// Send files, directories, or a text message to a device, by alias or
    /// IP address.
    Send {
        /// Destination alias or IP address.
        #[arg(long = "to")]
        to: String,
        /// Files or directories (sent recursively) to send.
        #[arg(required_unless_present = "text", conflicts_with = "text")]
        paths: Vec<PathBuf>,
        /// Send a text message instead of files. Use `-` to read it from
        /// stdin, unchanged.
        #[arg(long)]
        text: Option<String>,
    },
    /// Show the incoming request waiting for a decision: alias, address,
    /// fingerprint, content (tab-separated). Prints nothing when there is
    /// none.
    Pending,
    /// Accept the pending request. A text message is printed to stdout.
    Accept {
        /// Only accept if the request comes from this fingerprint.
        #[arg(long)]
        from: Option<String>,
    },
    /// Decline the pending request.
    Decline {
        /// Only decline if the request comes from this fingerprint.
        #[arg(long)]
        from: Option<String>,
    },
    /// Add a device to the known senders, whose requests `--accept known`
    /// accepts without asking.
    Trust {
        /// Alias, IP address, or fingerprint of the device.
        target: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    let (request, show_fingerprint) = match cli.command {
        Command::Status => (Request::Status, false),
        Command::List { fingerprint } => (Request::List, fingerprint),
        Command::Send { to, paths, text } => {
            let payload = match text {
                Some(text) => SendPayload::Text {
                    text: resolve_text(text)?,
                },
                // The daemon has its own working directory, so resolve
                // relative paths against ours.
                None => SendPayload::Files {
                    paths: paths
                        .into_iter()
                        .map(std::path::absolute)
                        .collect::<Result<_, _>>()?,
                },
            };
            (
                Request::Send {
                    target: to,
                    payload,
                },
                false,
            )
        }
        Command::Pending => (Request::Pending, false),
        Command::Accept { from } => (Request::Accept { from }, false),
        Command::Decline { from } => (Request::Decline { from }, false),
        Command::Trust { target } => (Request::Trust { target }, false),
    };

    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path).await.map_err(|err| {
        anyhow::anyhow!("Could not connect to lsendd at {}: {err}", path.display())
    })?;
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);

    write_message(&mut write_half, &request).await?;
    // The daemon only answers once the receiver decided and the upload is
    // done. stderr keeps stdout clean for scripts.
    if let Request::Send {
        target,
        payload: SendPayload::Files { .. },
    } = &request
        && !json
    {
        eprintln!("Waiting for {target} to accept...");
    }
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
        // A text message has no files to count, success is the exit code.
        Response::Send { .. }
            if matches!(
                request,
                Request::Send {
                    payload: SendPayload::Text { .. },
                    ..
                }
            ) => {}
        Response::Send { sent_files } => println!("Sent {sent_files} file(s)"),
        Response::Pending { request } => {
            if let Some(request) = request {
                print_pending(&request);
            }
        }
        // Unchanged, so `accept | wl-copy` copies exactly the text.
        Response::Accept { text } => print!("{}", text.unwrap_or_default()),
        Response::Decline => {}
        Response::Trust { alias, fingerprint } => match alias {
            Some(alias) => println!("Trusted {alias} ({fingerprint})"),
            None => println!("Trusted {fingerprint}"),
        },
    }

    Ok(())
}

/// Resolves `--text`'s value: `-` reads stdin fully, anything else is used
/// as-is.
fn resolve_text(text: String) -> anyhow::Result<String> {
    if text != "-" {
        return Ok(text);
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

fn print_pending(request: &PendingEntry) {
    println!(
        "{}\t{}\t{}\t{}",
        request.alias, request.address, request.fingerprint, request.content
    );
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
