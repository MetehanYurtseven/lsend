use localsend::http::server::common::save::FileUploadTarget;
use localsend::http::server::v2::{PrepareUploadDecisionV2, ServerEventV2};
use localsend::util::filename::{self, Rules};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;

/// Handles incoming server events: accepts every upload request and writes
/// files into the XDG download directory, resolving name collisions.
pub struct Receiver {
    /// Alias of the sender for each active session, for logging.
    sessions: HashMap<String, String>,
}

impl Receiver {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn handle_event(&mut self, event: ServerEventV2) {
        match event {
            ServerEventV2::Register { ip, info } => {
                println!("Register from {ip}: {} ({})", info.alias, info.fingerprint);
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
}

/// The XDG download directory, falling back to the current directory when it
/// cannot be determined.
fn download_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// A path in `dir` for `file_name` that did not exist yet, appending
/// ` (1)`, ` (2)`, ... before the extension on collisions. The file is
/// created right away to reserve the name, so two uploads with the same name
/// cannot end up on the same path. `file_name` comes from the sender and is
/// untrusted, so it is sanitized first.
fn unique_path(dir: &Path, file_name: &str) -> PathBuf {
    let name = filename::sanitize_path(file_name, Rules::current());
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
