use crate::identity::Identity;
use crate::target::TargetSelector;
use localsend::discovery::DiscoveryHandle;
use localsend::http::client::v2::LsHttpClientV2;
use localsend::http::dto_v2::PrepareUploadRequestDtoV2;
use localsend::model::discovery::ProtocolType;
use localsend::model::transfer::{FileContent, FileDto, FileMetadata};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Resolves `target` without scanning the network and sends `paths` to it:
/// prepare-upload, then one upload request per accepted file. Returns the
/// number of files actually sent.
pub async fn send(
    identity: &Identity,
    discovery: &DiscoveryHandle,
    target: &str,
    paths: Vec<PathBuf>,
) -> Result<usize, String> {
    let target = TargetSelector::parse(target)?;
    let (files, file_paths) = collect_files(paths)?;
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
        return Err("All files were declined".to_string());
    };

    // Upload sequentially in a stable order.
    let mut file_ids: Vec<&String> = response.files.keys().collect();
    file_ids.sort_by_key(|file_id| &files[*file_id].file_name);

    let mut sent_files = 0usize;
    for file_id in file_ids {
        let token = &response.files[file_id];
        let path = file_paths[file_id].clone();
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
/// paths under the same IDs. Only individual files are supported; a path
/// that is not a regular file is an error.
fn collect_files(paths: Vec<PathBuf>) -> Result<CollectedFiles, String> {
    let mut files = HashMap::new();
    let mut file_paths = HashMap::new();
    for path in paths {
        let metadata =
            std::fs::metadata(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("{}: not a regular file", path.display()));
        }
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{}: has no file name", path.display()))?;

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
    Ok((files, file_paths))
}
