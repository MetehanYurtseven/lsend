use crate::discovery::Discovery;
use crate::identity;
use crate::server::{self, Server};

/// The daemon's lifecycle: identity, server and discovery, from start to a
/// clean shutdown.
pub struct Daemon {
    server: Server,
    discovery: Discovery,
}

impl Daemon {
    pub async fn start(alias: String, port: u16) -> anyhow::Result<Self> {
        let cert = identity::generate()?;
        println!("Generated identity, fingerprint: {}", cert.fingerprint);

        let server = Server::start(&cert, &alias, port).await?;
        println!("HTTP server listening on port {port}");

        let discovery = Discovery::start(&cert, alias, port).await?;
        println!("Announcing on the network...");
        discovery.announce().await;

        Ok(Self { server, discovery })
    }

    /// Runs the event loop until Ctrl+C is pressed.
    pub async fn run(&mut self) {
        println!("Running. Press Ctrl+C to stop.");
        loop {
            tokio::select! {
                Some(event) = self.server.events.recv() => server::handle_event(event),
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
