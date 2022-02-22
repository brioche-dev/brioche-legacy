use std::{env, io::SeekFrom, path::PathBuf};

use anyhow::Context as _;
use futures_util::StreamExt as _;
use tokio::{
    fs,
    io::{AsyncSeekExt as _, AsyncWriteExt as _},
};
use url::Url;
use uuid::Uuid;

#[derive(Debug)]
pub struct State {
    pub downloads_dir: PathBuf,
    pub temp_downloads_dir: PathBuf,
}

impl State {
    pub async fn new() -> anyhow::Result<Self> {
        let project_dirs = directories::ProjectDirs::from("dev.brioche", "Brioche", "brioche")
            .context("home directory not found")?;

        let data_dir = project_dirs.data_dir();
        fs::create_dir_all(&data_dir).await?;

        let downloads_dir = data_dir.join("downloads");
        fs::create_dir_all(&downloads_dir).await?;

        let temp_downloads_dir = downloads_dir.join("_temp");
        fs::create_dir_all(&temp_downloads_dir).await?;

        Ok(Self {
            downloads_dir,
            temp_downloads_dir,
        })
    }

    pub async fn new_temp_work_dir(&self) -> anyhow::Result<PathBuf> {
        let uuid = uuid::Uuid::new_v4();
        let temp_dir = env::temp_dir();
        let work_dir = temp_dir
            .join("brioche")
            .join("work-dir")
            .join(uuid.to_string())
            .join("work");

        fs::create_dir_all(&work_dir).await?;

        Ok(work_dir)
    }

    pub async fn download(&self, url: Url, sha_hash: &[u8; 32]) -> anyhow::Result<fs::File> {
        use sha2::Digest as _;

        let final_file_path = self.downloads_dir.join(hex::encode(&sha_hash));
        let final_file = fs::File::open(&final_file_path).await;
        if let Ok(file) = final_file {
            return Ok(file);
        };

        let download_id = Uuid::new_v4();
        let temp_file_path = self.temp_downloads_dir.join(download_id.to_string());
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
}
