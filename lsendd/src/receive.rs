use localsend::discovery::{DeviceChannel, DiscoveredDevice, DiscoveryHandle, HttpChannel};
use localsend::http::dto_v2::RegisterDtoV2;
use localsend::http::server::PeerIp;
use localsend::http::server::common::save::FileUploadTarget;
use localsend::http::server::v2::{PrepareUploadDecisionV2, ServerEventV2};
use localsend::util::filename::{self, Rules};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::oneshot;

/// Handles incoming server events: accepts every upload request and writes
/// files into the XDG download directory, resolving name collisions.
pub struct Receiver {
    /// Alias of the sender for each active session, for logging.
    sessions: HashMap<String, String>,
    discovery: Arc<DiscoveryHandle>,
    own_fingerprint: String,
}

impl Receiver {
    pub fn new(discovery: Arc<DiscoveryHandle>, own_fingerprint: String) -> Self {
        Self {
            sessions: HashMap::new(),
            discovery,
            own_fingerprint,
        }
    }

    pub fn handle_event(&mut self, event: ServerEventV2) {
        match event {
            ServerEventV2::Register { ip, info } => {
                println!("Register from {ip}: {} ({})", info.alias, info.fingerprint);
                self.device_confirmed(ip, info);
            }
            ServerEventV2::PrepareUpload {
                session_id,
                ip,
                info,
                files,
                decision_tx,
                ..
            } => {
                println!(
                    "PrepareUpload from {ip} ({}): accepting {} file(s)",
                    info.alias,
                    files.len()
                );
                // The sender is clearly reachable.
                self.device_confirmed(ip, info.clone());
                self.sessions.insert(session_id, info.alias);
                let ids: HashSet<String> = files.keys().cloned().collect();
                let _ = decision_tx.send(PrepareUploadDecisionV2::Accept(ids));
            }
            ServerEventV2::FileUpload {
                file, target_tx, ..
            } => {
                let path = unique_path(&download_dir(), &file.file_name);
                println!("Receiving {}", path.display());
                let (result_tx, result_rx) = oneshot::channel();
                let reserved_path = path.clone();
                tokio::spawn(async move {
                    // A dropped channel means the upload never completed.
                    let result = result_rx
                        .await
                        .unwrap_or_else(|_| Err("Upload aborted".to_string()));
                    if let Err(err) = result {
                        eprintln!("Failed to save {}: {err}", reserved_path.display());
                        // Remove the reserved, possibly partial file.
                        let _ = tokio::fs::remove_file(&reserved_path).await;
                    }
                });
                let _ = target_tx.send(FileUploadTarget::Path {
                    path,
                    result_tx,
                    progress_tx: None,
                });
            }
            ServerEventV2::SessionEnd { session_id, reason } => {
                let alias = self.sessions.remove(&session_id).unwrap_or_default();
                println!("SessionEnd {alias} ({session_id}): {reason:?}");
            }
            ServerEventV2::PrepareUploadAborted { session_id } => {
                self.sessions.remove(&session_id);
                println!("PrepareUploadAborted {session_id}");
            }
            ServerEventV2::CancelReceived { ip, session_id } => {
                println!("CancelReceived from {ip}: {session_id}");
            }
            ServerEventV2::ListenerFailed { error } => {
                eprintln!("Server listener failed: {error}");
            }
        }
    }

    /// Feeds a device confirmed outside of discovery (it registered with the
    /// HTTP server or sent a transfer request) into the discovery store. The
    /// server does not do this itself, it has no access to the store.
    fn device_confirmed(&self, ip: PeerIp, info: RegisterDtoV2) {
        if info.fingerprint == self.own_fingerprint {
            return;
        }
        let device = DiscoveredDevice {
            alias: info.alias,
            version: info.version,
            device_model: info.device_model,
            device_type: info.device_type,
            fingerprint: info.fingerprint,
            channel: DeviceChannel::Http(HttpChannel {
                host: ip.to_string(),
                port: info.port,
                protocol: info.protocol,
            }),
            download: info.download,
        };
        let discovery = self.discovery.clone();
        tokio::spawn(async move {
            discovery.add_device(device).await;
        });
    }
}

/// The XDG download directory, falling back to the current directory when it
/// cannot be determined.
fn download_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// A path in `dir` for `file_name` that did not exist yet, appending
/// ` (1)`, ` (2)`, ... before the extension on collisions. The file is
/// created right away to reserve the name, so two uploads with the same name
/// cannot end up on the same path.
///
/// `file_name` may be a relative path (`photos/2024/a.jpg`) when a folder is
/// sent; its directories are recreated below `dir`. It comes from the sender
/// and is untrusted, so `.` and `..` segments are dropped and every segment
/// is sanitized, keeping the result inside `dir`.
fn unique_path(dir: &Path, file_name: &str) -> PathBuf {
    let rules = Rules::current();
    let mut segments: Vec<String> = file_name
        .split(['/', '\\'])
        .filter(|segment| !matches!(*segment, "" | "." | ".."))
        .map(|segment| filename::sanitize(segment, rules))
        .collect();
    let name = segments
        .pop()
        .unwrap_or_else(|| filename::sanitize("", rules));
    let dir = segments
        .iter()
        .fold(dir.to_path_buf(), |dir, segment| dir.join(segment));
    // A failure surfaces when the core crate opens the file.
    let _ = std::fs::create_dir_all(&dir);

    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name.as_str(), String::new()),
    };

    std::iter::once(dir.join(&name))
        .chain((1..).map(|i| dir.join(format!("{stem} ({i}){extension}"))))
        .find(|candidate| {
            // Any error other than a collision is left for the core crate to
            // report when it opens the file.
            !matches!(
                OpenOptions::new().write(true).create_new(true).open(candidate),
                Err(err) if err.kind() == ErrorKind::AlreadyExists
            )
        })
        .unwrap()
}
