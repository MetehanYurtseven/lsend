use crate::discovery::Discovery;
use crate::identity::{self, Identity};
use crate::ipc_server::IpcServer;
use crate::receive::Receiver;
use crate::server::Server;
use ipc::StatusResponse;
use localsend::discovery::DiscoveryHandle;
use std::sync::Arc;
use tokio::net::UnixStream;

/// The daemon's lifecycle: identity, server, discovery and IPC, from start
/// to a clean shutdown.
pub struct Daemon {
    status: StatusResponse,
    identity: Arc<Identity>,
    server: Server,
    receiver: Receiver,
    discovery: Discovery,
    ipc: IpcServer,
}

impl Daemon {
    pub async fn start(alias: String, port: u16) -> anyhow::Result<Self> {
        let cert = identity::generate()?;
        println!("Generated identity, fingerprint: {}", cert.fingerprint);
        let identity = Arc::new(Identity::new(&cert, alias.clone(), port));

        let server = Server::start(&cert, &alias, port).await?;
        println!("HTTP server listening on port {port}");

        let discovery = Discovery::start(&cert, alias.clone(), port).await?;
        println!("Announcing on the network...");
        discovery.announce().await;

        let ipc = IpcServer::bind().await?;
        println!("IPC socket listening at {}", ipc.path().display());

        Ok(Self {
            status: StatusResponse {
                alias,
                fingerprint: cert.fingerprint,
                port,
            },
            identity,
            server,
            receiver: Receiver::new(),
            discovery,
            ipc,
        })
    }

    /// Runs the event loop until Ctrl+C is pressed.
    pub async fn run(&mut self) {
        println!("Running. Press Ctrl+C to stop.");
        loop {
            tokio::select! {
                Some(event) = self.server.events.recv() => self.receiver.handle_event(event),
                accept_result = self.ipc.accept() => match accept_result {
                    Ok(stream) => spawn_ipc_connection(
                        stream,
                        self.status.clone(),
                        self.identity.clone(),
                        self.discovery.handle.clone(),
                    ),
                    Err(err) => eprintln!("IPC accept failed: {err:#}"),
                },
                _ = tokio::signal::ctrl_c() => break,
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
    status: StatusResponse,
    identity: Arc<Identity>,
    discovery: Arc<DiscoveryHandle>,
) {
    tokio::spawn(async move {
        if let Err(err) =
            crate::ipc_server::handle_connection(stream, status, identity, discovery).await
        {
            eprintln!("IPC connection error: {err:#}");
        }
    });
}
