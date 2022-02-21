use futures_util::StreamExt as _;
use sha2::Digest as _;
use std::{io::SeekFrom, path::Path};
use tokio::{
    fs,
    io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _},
};
use url::Url;

pub async fn download(
    download_dir: impl AsRef<Path>,
    url: Url,
    sha_hash: &[u8; 32],
) -> anyhow::Result<fs::File> {
    let download_dir = download_dir.as_ref();
    let download_path = download_dir.join(hex::encode(&sha_hash));

    let mut file = fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(&download_path)
        .await?;

    let mut file_hash = sha2::Sha256::new();
    let metadata = file.metadata().await?;
    if metadata.len() == 0 {
        let response = reqwest::get(url).await?;
        response.error_for_status_ref()?;

        let mut response_body_stream = response.bytes_stream();
        while let Some(chunk) = response_body_stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            file_hash.update(&chunk);
        }

        println!("Downloaded file {}", download_path.display());
    } else {
        file.seek(SeekFrom::Start(0)).await?;

        let mut buf = [0u8; 4096];
        loop {
            let len = file.read(&mut buf).await?;
            if len == 0 {
                break;
            }

            let buf = &buf[0..len];
            file_hash.update(buf);
        }

        println!("Read file {}", download_path.display());
    }

    let file_hash = file_hash.finalize();
    if &*file_hash != &*sha_hash {
        anyhow::bail!(
            "File hash did not match (expected {}, got {})",
            hex::encode(sha_hash),
            hex::encode(file_hash),
        );
    }

    file.seek(SeekFrom::Start(0)).await?;

    Ok(file)
}
