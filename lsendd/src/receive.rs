use localsend::discovery::{DeviceChannel, DiscoveredDevice, DiscoveryHandle, HttpChannel};
use localsend::http::dto_v2::RegisterDtoV2;
use localsend::http::server::PeerIp;
use localsend::http::server::common::save::FileUploadTarget;
use localsend::http::server::v2::{PrepareUploadDecisionV2, ServerEventV2};
use localsend::model::transfer::FileDto;
use localsend::util::filename::{self, Rules};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::oneshot;

/// Handles incoming server events: accepts every upload request and writes
/// files into the XDG download directory, resolving name collisions. Text
/// messages are handed to the `on_text` command instead.
pub struct Receiver {
    /// Alias of the sender for each active session, for logging.
    sessions: HashMap<String, String>,
    discovery: Arc<DiscoveryHandle>,
    own_fingerprint: String,
    on_text: String,
}

impl Receiver {
    pub fn new(discovery: Arc<DiscoveryHandle>, own_fingerprint: String, on_text: String) -> Self {
        Self {
            sessions: HashMap::new(),
            discovery,
            own_fingerprint,
            on_text,
        }
    }

    /// Handles one server event. Fails when the server has stopped itself
    /// and can no longer receive anything.
    pub fn handle_event(&mut self, event: ServerEventV2) -> anyhow::Result<()> {
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
                // The sender is clearly reachable.
                self.device_confirmed(ip, info.clone());

                if let Some(message) = message_of(&files) {
                    // The text is the request itself: accepting no file ends
                    // it with 204, nothing is uploaded.
                    let _ = decision_tx.send(PrepareUploadDecisionV2::Accept(HashSet::new()));
                    run_on_text(self.on_text.clone(), message.to_string());
                    return Ok(());
                }

                println!(
                    "PrepareUpload from {ip} ({}): accepting {} file(s)",
                    info.alias,
                    files.len()
                );
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
                anyhow::bail!("Server listener failed: {error}");
            }
        }
        Ok(())
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

/// The text of a message request: a single text file whose content is
/// embedded in `preview`, as the official app sends it.
fn message_of(files: &HashMap<String, FileDto>) -> Option<&str> {
    let [file] = files.values().collect::<Vec<_>>()[..] else {
        return None;
    };
    if file.file_type != "text" && !file.file_type.starts_with("text/") {
        return None;
    }
    file.preview.as_deref()
}

/// Runs `command` through `sh -c` with `message` on its stdin, without
/// blocking the caller.
fn run_on_text(command: String, message: String) {
    tokio::spawn(async move {
        let mut child = match Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdin(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                eprintln!("Failed to run on-text command: {err}");
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            // A command that does not read its input closes the pipe early.
            let _ = stdin.write_all(message.as_bytes()).await;
        }
        match child.wait().await {
            Ok(status) if !status.success() => eprintln!("on-text command exited with {status}"),
            Ok(_) => {}
            Err(err) => eprintln!("Failed to wait for on-text command: {err}"),
        }
    });
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
