use crate::identity::Identity;
use crate::target::TargetSelector;
use ipc::SendPayload;
use localsend::discovery::DiscoveryHandle;
use localsend::http::client::v2::LsHttpClientV2;
use localsend::http::dto_v2::PrepareUploadRequestDtoV2;
use localsend::model::discovery::ProtocolType;
use localsend::model::transfer::{FileContent, FileDto, FileMetadata};
use std::collections::HashMap;
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use walkdir::WalkDir;

/// Resolves `target` without scanning the network and sends `content` to it:
/// prepare-upload, then one upload request per accepted file. Returns the
/// number of files actually sent.
pub async fn send(
    identity: &Identity,
    discovery: &DiscoveryHandle,
    target: &str,
    content: SendPayload,
) -> Result<usize, String> {
    let target = TargetSelector::parse(target)?;
    let (files, file_paths) = match content {
        SendPayload::Files { paths } => collect_files(paths)?,
        SendPayload::Text { text } => collect_text(text),
    };
    if files.is_empty() {
        return Err("No files selected".to_string());
    }

    let device = target.resolve(discovery).await?;

    let http = device
        .get_best_channel()
        .and_then(|channel| channel.http())
        .ok_or_else(|| format!("{}: No dialable address", device.device.alias))?
        .clone();

    let expected_fingerprint = match http.protocol {
        ProtocolType::Https => Some(device.device.fingerprint.clone()),
        ProtocolType::Http => None,
    };
    let client = LsHttpClientV2::try_new(
        &identity.cert.private_key_pem,
        &identity.cert.certificate_pem,
        expected_fingerprint,
        None,
    )
    .map_err(|err| format!("Failed to create HTTP client: {err}"))?;

    let payload = PrepareUploadRequestDtoV2 {
        info: identity.register_dto(),
        files: files.clone(),
    };
    let cancel = CancellationToken::new();
    let prepared = client
        .prepare_upload(
            http.protocol,
            &http.host,
            http.port,
            None,
            payload,
            None,
            cancel.clone(),
        )
        .await
        .map_err(|err| format!("Failed to prepare upload: {err}"))?;

    let Some(response) = prepared.response else {
        // Only a text message has no paths. Its receiver reads it from the
        // request and answers 204 without asking for an upload.
        if file_paths.is_empty() {
            return Ok(0);
        }
        return Err("All files were declined".to_string());
    };

    // Upload sequentially in a stable order.
    let mut file_ids: Vec<&String> = response.files.keys().collect();
    file_ids.sort_by_key(|file_id| &files[*file_id].file_name);

    let mut sent_files = 0usize;
    for file_id in file_ids {
        let token = &response.files[file_id];
        // A text message has no path; skip it if a receiver asks for it anyway.
        let Some(path) = file_paths.get(file_id).cloned() else {
            continue;
        };
        let body = localsend::reqwest::Body::wrap_stream(FileContent::Path(path).into_stream());

        match client
            .upload(
                http.protocol,
                &http.host,
                http.port,
                None,
                &response.session_id,
                file_id,
                token,
                body,
                cancel.clone(),
            )
            .await
        {
            Ok(()) => sent_files += 1,
            Err(err) => {
                let _ = client
                    .cancel(http.protocol, &http.host, http.port, &response.session_id)
                    .await;
                return Err(format!(
                    "Failed to upload {}: {err}",
                    files[file_id].file_name
                ));
            }
        }
    }

    Ok(sent_files)
}

type CollectedFiles = (HashMap<String, FileDto>, HashMap<String, PathBuf>);

/// Stats `paths` into transfer metadata keyed by a fresh file ID, plus the
/// paths under the same IDs. Directories are expanded, see [`expand_path`].
fn collect_files(paths: Vec<PathBuf>) -> Result<CollectedFiles, String> {
    let mut files = HashMap::new();
    let mut file_paths = HashMap::new();
    for path in paths {
        for (path, metadata, file_name) in expand_path(&path)? {
            let id = Uuid::new_v4().to_string();
            files.insert(
                id.clone(),
                FileDto {
                    id: id.clone(),
                    file_name,
                    size: metadata.len(),
                    file_type: mime_guess::from_path(&path)
                        .first_or_octet_stream()
                        .to_string(),
                    sha256: None,
                    preview: None,
                    metadata: FileMetadata::from_fs_metadata(&metadata),
                },
            );
            file_paths.insert(id, path);
        }
    }
    Ok((files, file_paths))
}

/// A single text message as a transfer: one file whose content is embedded
/// in `preview`, matching how the official app sends a message. It has no
/// path, the receiver reads it from the request.
fn collect_text(text: String) -> CollectedFiles {
    let id = Uuid::new_v4().to_string();
    let file = FileDto {
        id: id.clone(),
        file_name: format!("{id}.txt"),
        size: text.len() as u64,
        file_type: "text/plain".to_string(),
        sha256: None,
        preview: Some(text),
        metadata: None,
    };
    (HashMap::from([(id, file)]), HashMap::new())
}

/// Expands `path` into the files to send, each with its protocol file name.
/// A directory is walked recursively and its files are named by their path
/// including the directory itself (`photos/2024/a.jpg`), so the receiver can
/// rebuild the tree, as the official app does.
fn expand_path(path: &Path) -> Result<Vec<(PathBuf, Metadata, String)>, String> {
    let metadata = std::fs::metadata(path).map_err(|err| format!("{}: {err}", path.display()))?;
    // `file_name` is `None` for paths like `..`, so fall back to the
    // resolved path.
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .or_else(|| {
            path.canonicalize()
                .ok()?
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .ok_or_else(|| format!("{}: has no file name", path.display()))?;

    if metadata.is_file() {
        return Ok(vec![(path.to_path_buf(), metadata, name)]);
    }
    if !metadata.is_dir() {
        return Err(format!(
            "{}: not a regular file or directory",
            path.display()
        ));
    }

    let mut expanded = Vec::new();
    for entry in WalkDir::new(path).min_depth(1).sort_by_file_name() {
        let entry = entry.map_err(|err| err.to_string())?;
        if !entry.file_type().is_file() {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|err| format!("{}: {err}", entry.path().display()))?;
        let relative = entry
            .path()
            .strip_prefix(path)
            .expect("walkdir yields paths below its root");
        let file_name = std::iter::once(name.clone())
            .chain(
                relative
                    .iter()
                    .map(|segment| segment.to_string_lossy().into_owned()),
            )
            .collect::<Vec<_>>()
            .join("/");
        expanded.push((entry.into_path(), metadata, file_name));
    }
    Ok(expanded)
}
