use std::{
    collections::HashMap,
    env,
    io::SeekFrom,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use futures_util::StreamExt as _;
use tokio::{
    fs,
    io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _},
    sync::RwLock,
};
use url::Url;
use uuid::Uuid;

use crate::hash::Hash;

#[derive(Debug)]
pub struct State {
    project_dirs: directories::ProjectDirs,
    lockfile: Lockfile,
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

        let lockfile_path = data_dir.join("lockfile.json");
        let lockfile = Lockfile::open(lockfile_path).await?;

        Ok(Self {
            project_dirs,
            lockfile,
            downloads_dir,
            temp_downloads_dir,
        })
    }

    pub async fn persist_lockfile(&self) -> anyhow::Result<()> {
        self.lockfile.persist().await?;
        Ok(())
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

    pub async fn get_existing_content_file(&self, req: &ContentRequest) -> Option<ContentFile> {
        let content_hash = req.content_hash?;

        let file_path = self.downloads_dir.join(content_hash.to_path_component());
        let file = fs::File::open(&file_path).await.ok()?;

        Some(ContentFile { file, content_hash })
    }

    pub async fn download(&self, mut req: ContentRequest) -> anyhow::Result<ContentFile> {
        use sha2::Digest as _;

        if req.content_hash.is_none() {
            req.content_hash = self.lockfile.request_hash(&req.url).await;
        }

        let existing_file = self.get_existing_content_file(&req).await;
        if let Some(existing) = existing_file {
            return Ok(existing);
        };

        let download_id = Uuid::new_v4();
        let temp_file_path = self.temp_downloads_dir.join(download_id.to_string());
        let mut download_file = fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create_new(true)
            .open(&temp_file_path)
            .await?;

        let response = reqwest::get(req.url.clone()).await?;
        response.error_for_status_ref()?;
        let mut file_hash = sha2::Sha256::new();

        let mut response_body_stream = response.bytes_stream();
        while let Some(chunk) = response_body_stream.next().await {
            let chunk = chunk?;
            download_file.write_all(&chunk).await?;
            file_hash.update(&chunk);
        }

        let downloaded_hash = Hash::from_digest(file_hash);
        if let Some(expected_hash) = req.content_hash {
            if expected_hash != downloaded_hash {
                anyhow::bail!(
                    "File hash did not match for {} (expected {}, got {})",
                    req.url,
                    expected_hash,
                    downloaded_hash,
                );
            }
        };

        let final_file_path = self.downloads_dir.join(downloaded_hash.to_path_component());
        let rename_result = fs::rename(&temp_file_path, &final_file_path).await;
        match rename_result {
            Ok(()) => {
                println!(
                    "Downloaded URL {} -> {}",
                    req.url,
                    final_file_path.display()
                );
            }
            Err(error) => {
                eprintln!(
                    "Downloaded URL {} -> {} (failed to rename: {})",
                    req.url,
                    temp_file_path.display(),
                    error
                );
            }
        }

        self.lockfile
            .set_request_hash(req.url, downloaded_hash)
            .await;

        download_file.seek(SeekFrom::Start(0)).await?;
        Ok(ContentFile {
            file: download_file,
            content_hash: downloaded_hash,
        })
    }

    pub async fn unpack(
        &self,
        archive_tar_gz: &mut ContentFile,
        _unpack_opts: UnpackOpts,
    ) -> anyhow::Result<PathBuf> {
        let archive_dir = self
            .project_dirs
            .data_dir()
            .join("unpack")
            .join(archive_tar_gz.content_hash.to_path_component());
        let temp_dir = archive_dir.join("temp");
        let unpacked_dir = archive_dir.join("unpacked");

        fs::create_dir_all(&archive_dir).await?;

        // If the temporary dir already exists, clear it out and recreate it
        let _ = fs::remove_dir_all(&temp_dir).await;

        if unpacked_dir.exists() {
            return Ok(unpacked_dir);
        }

        fs::create_dir(&temp_dir).await?;

        let unpack_id = Uuid::new_v4();
        let target_dir = temp_dir.join(unpack_id.to_string());
        fs::create_dir(&target_dir).await?;

        self.unpack_to(archive_tar_gz, &target_dir).await?;

        let rename_result = fs::rename(&target_dir, &unpacked_dir).await;

        match rename_result {
            Ok(()) => {
                println!(
                    "Unpacked {} -> {}",
                    archive_tar_gz.content_hash,
                    unpacked_dir.display()
                );
                Ok(unpacked_dir)
            }
            Err(error) => {
                eprintln!(
                    "Unpacked {} -> {} (failed to rename: {})",
                    archive_tar_gz.content_hash,
                    target_dir.display(),
                    error
                );
                Ok(target_dir)
            }
        }
    }

    pub async fn unpack_to(
        &self,
        archive_tar_gz: &mut ContentFile,
        target_dir: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let mut tar_command = tokio::process::Command::new("tar");
        tar_command.arg("-x");
        tar_command.arg("-z");
        tar_command.arg("-f").arg("-");
        tar_command.arg("-C").arg(target_dir.as_ref());

        tar_command.stdin(std::process::Stdio::piped());

        let mut tar_child = tar_command.spawn()?;

        if let Some(ref mut tar_stdin) = tar_child.stdin {
            tokio::io::copy(&mut archive_tar_gz.file, tar_stdin).await?;
        }

        let tar_result = tar_child.wait().await?;
        if !tar_result.success() {
            anyhow::bail!("tar exited with status {}", tar_result);
        }

        Ok(())
    }

    pub fn get_recipe_output(
        &self,
        recipe: &crate::recipe::RecipeDefinition,
    ) -> anyhow::Result<Option<PathBuf>> {
        let recipe_hash = crate::recipe::recipe_definition_hash(recipe)?;
        let recipe_prefix_dir = self
            .project_dirs
            .data_dir()
            .join("recipes")
            .join(recipe_hash.to_path_component())
            .join("prefix");

        if recipe_prefix_dir.is_dir() {
            Ok(Some(recipe_prefix_dir))
        } else {
            Ok(None)
        }
    }

    pub async fn save_recipe_output(
        &self,
        recipe: &crate::recipe::RecipeDefinition,
        output_dir: impl AsRef<Path>,
    ) -> anyhow::Result<PathBuf> {
        let recipe_hash = crate::recipe::recipe_definition_hash(recipe)?;
        let recipe_dir = self
            .project_dirs
            .data_dir()
            .join("recipes")
            .join(recipe_hash.to_path_component());

        let recipe_prefix_dir = recipe_dir.join("prefix");

        fs::create_dir_all(&recipe_prefix_dir).await?;

        let temp_id = Uuid::new_v4();
        let recipe_prefix_temp_dir = recipe_dir.join(format!("prefix-tmp.{}", temp_id));

        let mv_result = tokio::process::Command::new("mv")
            .arg(output_dir.as_ref())
            .arg(&recipe_prefix_temp_dir)
            .spawn()?
            .wait()
            .await?;
        if !mv_result.success() {
            anyhow::bail!("mv exited with status {}", mv_result);
        }

        fs::rename(&recipe_prefix_temp_dir, &recipe_prefix_dir).await?;

        Ok(recipe_prefix_dir)
    }
}

pub struct ContentFile {
    file: tokio::fs::File,
    content_hash: Hash,
}

pub struct ContentRequest {
    url: Url,
    content_hash: Option<Hash>,
}

impl ContentRequest {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            content_hash: None,
        }
    }

    pub fn maybe_hash(mut self, hash: Option<Hash>) -> Self {
        self.content_hash = hash;
        self
    }
}

pub enum UnpackOpts {
    Reusable,
}

#[derive(Debug)]
struct Lockfile {
    path: PathBuf,
    current_value: RwLock<ContentLock>,
}

impl Lockfile {
    async fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = fs::File::open(path).await;

        let current_value = match file {
            Ok(mut existing_file) => {
                let mut file_content = vec![];
                existing_file.read_to_end(&mut file_content).await?;
                let value = serde_json::from_slice(&file_content)?;
                value
            }
            Err(error) => {
                eprintln!("Failed to open lockfile: {}", error);
                ContentLock::default()
            }
        };

        Ok(Self {
            path: path.to_owned(),
            current_value: RwLock::new(current_value),
        })
    }

    async fn request_hash(&self, url: &Url) -> Option<Hash> {
        let lock = self.current_value.read().await;
        lock.request_hashes.get(url).cloned()
    }

    async fn set_request_hash(&self, url: Url, hash: Hash) {
        let mut lock = self.current_value.write().await;
        lock.request_hashes.insert(url, hash);
    }

    async fn persist(&self) -> anyhow::Result<()> {
        let current_value = self.current_value.read().await;
        let new_content = serde_json::to_vec_pretty(&*current_value)?;

        let mut file = fs::File::create(&self.path).await?;
        tokio::io::copy(&mut &new_content[..], &mut file).await?;

        Ok(())
    }
}

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
struct ContentLock {
    request_hashes: HashMap<Url, Hash>,
}
