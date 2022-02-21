use futures_util::StreamExt as _;
use sha2::Digest as _;
use std::io::SeekFrom;
use tokio::{
    fs,
    io::{AsyncSeekExt as _, AsyncWriteExt as _},
};
use url::Url;
use uuid::Uuid;

use crate::state::State;

pub async fn download(state: &State, url: Url, sha_hash: &[u8; 32]) -> anyhow::Result<fs::File> {
    let final_file_path = state.downloads_dir.join(hex::encode(&sha_hash));
    let final_file = fs::File::open(&final_file_path).await;
    if let Ok(file) = final_file {
        return Ok(file);
    };

    let download_id = Uuid::new_v4();
    let temp_file_path = state.temp_downloads_dir.join(download_id.to_string());
    let mut download_file = fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create_new(true)
        .open(&temp_file_path)
        .await?;

    let response = reqwest::get(url.clone()).await?;
    dbg!(response.error_for_status_ref())?;
    let mut file_hash = sha2::Sha256::new();

    let mut response_body_stream = response.bytes_stream();
    while let Some(chunk) = response_body_stream.next().await {
        let chunk = chunk?;
        download_file.write_all(&chunk).await?;
        file_hash.update(&chunk);
    }

    let file_hash = file_hash.finalize();
    if &*file_hash == &*sha_hash {
        let rename_result = fs::rename(&temp_file_path, &final_file_path).await;
        match rename_result {
            Ok(()) => {
                println!("Downloaded URL {} -> {}", url, final_file_path.display());
            }
            Err(error) => {
                eprintln!(
                    "Downloaded URL {} -> {} (failed to rename: {})",
                    url,
                    temp_file_path.display(),
                    error
                );
            }
        }
    } else {
        anyhow::bail!(
            "File hash did not match for {} (expected {}, got {})",
            url,
            hex::encode(sha_hash),
            hex::encode(file_hash),
        );
    }

    download_file.seek(SeekFrom::Start(0)).await?;

    Ok(download_file)
}
