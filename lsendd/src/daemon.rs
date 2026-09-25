use crate::discovery::Discovery;
use crate::identity::Identity;
use crate::ipc_server::IpcServer;
use crate::receive::Receiver;
use crate::server::Server;
use localsend::discovery::DiscoveryHandle;
use std::sync::Arc;
use tokio::net::UnixStream;
use tokio::signal::unix::{Signal, SignalKind, signal};

/// The daemon's lifecycle: identity, server, discovery and IPC, from start
/// to a clean shutdown.
pub struct Daemon {
    identity: Arc<Identity>,
    server: Server,
    receiver: Receiver,
    discovery: Discovery,
    ipc: IpcServer,
    sigterm: Signal,
}

impl Daemon {
    pub async fn start(alias: String, port: u16) -> anyhow::Result<Self> {
        let identity = Arc::new(Identity::generate(alias, port)?);
        println!(
            "Generated identity, fingerprint: {}",
            identity.fingerprint()
        );

        let server = Server::start(&identity).await?;
        println!("HTTP server listening on port {port}");

        let discovery = Discovery::start(&identity).await?;
        println!("Announcing on the network...");
        discovery.announce().await;

        let ipc = IpcServer::bind().await?;
        println!("IPC socket listening at {}", ipc.path().display());

        Ok(Self {
            identity,
            server,
            receiver: Receiver::new(),
            discovery,
            ipc,
            sigterm: signal(SignalKind::terminate())?,
        })
    }

    /// Runs the event loop until SIGINT (Ctrl+C) or SIGTERM (e.g. systemd
    /// stopping the service) arrives.
    pub async fn run(&mut self) {
        println!("Running. Press Ctrl+C to stop.");
        loop {
            tokio::select! {
                Some(event) = self.server.events.recv() => self.receiver.handle_event(event),
                accept_result = self.ipc.accept() => match accept_result {
                    Ok(stream) => spawn_ipc_connection(
                        stream,
                        self.identity.clone(),
                        self.discovery.handle.clone(),
                    ),
                    Err(err) => eprintln!("IPC accept failed: {err:#}"),
                },
                _ = tokio::signal::ctrl_c() => break,
                _ = self.sigterm.recv() => break,
            }
        }
    }

    pub async fn shutdown(self) {
        println!("Shutting down...");
        self.server.shutdown().await;
        self.discovery.shutdown().await;
        println!("Stopped.");
    }
}

fn spawn_ipc_connection(
    stream: UnixStream,
    identity: Arc<Identity>,
    discovery: Arc<DiscoveryHandle>,
) {
    tokio::spawn(async move {
        if let Err(err) = crate::ipc_server::handle_connection(stream, identity, discovery).await {
            eprintln!("IPC connection error: {err:#}");
        }
    });
}
