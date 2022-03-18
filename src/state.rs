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

use crate::{hash::Hash, recipe::ResolvedRecipeRef};

#[derive(Debug)]
pub struct State {
    project_dirs: directories::ProjectDirs,
    lockfile: Lockfile,
    pub checkouts_dir: PathBuf,
    pub downloads_dir: PathBuf,
    pub temp_checkouts_dir: PathBuf,
    pub temp_downloads_dir: PathBuf,
}

impl State {
    pub async fn new() -> anyhow::Result<Self> {
        let project_dirs = directories::ProjectDirs::from("dev.brioche", "Brioche", "brioche")
            .context("home directory not found")?;

        let data_dir = project_dirs.data_dir();
        fs::create_dir_all(&data_dir).await?;

        let checkouts_dir = data_dir.join("checkouts");
        fs::create_dir_all(&checkouts_dir).await?;

        let temp_checkouts_dir = checkouts_dir.join("_temp");
        fs::create_dir_all(&temp_checkouts_dir).await?;

        let downloads_dir = data_dir.join("downloads");
        fs::create_dir_all(&downloads_dir).await?;

        let temp_downloads_dir = downloads_dir.join("_temp");
        fs::create_dir_all(&temp_downloads_dir).await?;

        let lockfile_path = data_dir.join("lockfile.json");
        let lockfile = Lockfile::open(lockfile_path).await?;

        Ok(Self {
            project_dirs,
            lockfile,
            checkouts_dir,
            downloads_dir,
            temp_checkouts_dir,
            temp_downloads_dir,
        })
    }

    pub async fn persist_lockfile(&self) -> anyhow::Result<bool> {
        let result = self.lockfile.persist().await?;
        Ok(result)
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

    pub async fn git_checkout(&self, req: GitCheckoutRequest) -> anyhow::Result<GitCheckout> {
        let commit = self.lockfile.git_commit_hash(&req.repo, &req.git_ref).await;
        if let Some(ref commit) = commit {
            let existing_checkout_path = self.checkouts_dir.join(commit);
            if existing_checkout_path.is_dir() {
                return Ok(GitCheckout {
                    checkout_path: existing_checkout_path,
                    commit: commit.to_string(),
                });
            }
        }

        let checkout_id = Uuid::new_v4();
        let temp_checkout_path = self.temp_checkouts_dir.join(checkout_id.to_string());

        let mut git_clone_command = tokio::process::Command::new("git");
        git_clone_command.arg("clone");
        git_clone_command.arg("--branch").arg(&req.git_ref);
        git_clone_command.arg("--depth").arg("1");
        git_clone_command
            .arg("--")
            .arg(req.repo.to_string())
            .arg(&temp_checkout_path);
        let git_clone_result = git_clone_command.status().await?;

        if !git_clone_result.success() {
            anyhow::bail!("git clone failed with exit code {}", git_clone_result);
        }

        let mut git_rev_parse_command = tokio::process::Command::new("git");
        git_rev_parse_command.arg("rev-parse").arg("HEAD");
        git_rev_parse_command.current_dir(&temp_checkout_path);
        let git_rev_parse_output = git_rev_parse_command.output().await?;

        if !git_rev_parse_output.status.success() {
            println!(
                "rev-parse stdout: {}",
                String::from_utf8_lossy(&git_rev_parse_output.stdout)
            );
            eprintln!(
                "rev-parse stderr: {}",
                String::from_utf8_lossy(&git_rev_parse_output.stdout)
            );
            anyhow::bail!(
                "git rev-parse failed with exit code {})",
                git_rev_parse_output.status
            );
        }

        // Trim any whitespace and normalize the commit hash by decoding
        // and re-encoding it as hex
        let git_commit_hash = String::from_utf8_lossy(&git_rev_parse_output.stdout);
        let git_commit_hash = git_commit_hash.trim_end();
        let git_commit_hash = hex::decode(git_commit_hash)?;
        let git_commit_hash = hex::encode(&git_commit_hash);

        let final_checkout_path = self.checkouts_dir.join(&git_commit_hash);
        let _ = fs::remove_dir_all(&final_checkout_path).await;

        let rename_result = fs::rename(&temp_checkout_path, &final_checkout_path).await;
        match rename_result {
            Ok(()) => {
                println!(
                    "Checked out repo {} @ {} -> {}",
                    req.repo,
                    req.git_ref,
                    final_checkout_path.display(),
                );
            }
            Err(error) => {
                eprintln!(
                    "Checked out repo {} @ {} -> {} (failed to rename: {})",
                    req.repo,
                    req.git_ref,
                    final_checkout_path.display(),
                    error,
                );
            }
        }

        self.lockfile
            .set_git_commit_hash(&req.repo, &req.git_ref, &git_commit_hash)
            .await;

        Ok(GitCheckout {
            checkout_path: final_checkout_path,
            commit: git_commit_hash,
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
        recipe_ref: &ResolvedRecipeRef,
    ) -> anyhow::Result<Option<PathBuf>> {
        let recipe_prefix_dir = self
            .project_dirs
            .data_dir()
            .join("recipes")
            .join(recipe_ref.to_path_component())
            .join("prefix");

        if recipe_prefix_dir.is_dir() {
            Ok(Some(recipe_prefix_dir))
        } else {
            Ok(None)
        }
    }

    pub async fn save_recipe_output(
        &self,
        recipe_ref: &crate::recipe::ResolvedRecipeRef,
        output_dir: impl AsRef<Path>,
    ) -> anyhow::Result<PathBuf> {
        let recipe_dir = self
            .project_dirs
            .data_dir()
            .join("recipes")
            .join(recipe_ref.to_path_component());

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

    // pub async fn get_recipe_aux(
    //     &self,
    //     recipe_ref: &crate::recipe::ResolvedRecipeRef,
    // ) -> Option<RecipeAux> {
    //     self.lockfile.recipe_aux(recipe_ref).await
    // }

    pub async fn set_recipe_aux(
        &self,
        recipe_ref: &crate::recipe::ResolvedRecipeRef,
        recipe_aux: RecipeAux,
    ) {
        self.lockfile.set_recipe_aux(recipe_ref, recipe_aux).await;
    }
}

pub struct GitCheckoutRequest {
    repo: Url,
    git_ref: String,
}

impl GitCheckoutRequest {
    pub fn new(repo: Url, git_ref: &str) -> Self {
        Self {
            repo,
            git_ref: git_ref.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct GitCheckout {
    pub commit: String,
    pub checkout_path: PathBuf,
}

#[derive(Debug)]
pub struct ContentFile {
    file: tokio::fs::File,
    pub content_hash: Hash,
}

impl ContentFile {
    pub async fn try_clone(&self) -> anyhow::Result<Self> {
        let file = self.file.try_clone().await?;
        Ok(Self {
            file,
            content_hash: self.content_hash,
        })
    }
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
    persisted_value: RwLock<ContentLock>,
    current_value: RwLock<ContentLock>,
}

impl Lockfile {
    async fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = fs::File::open(path).await;

        let persisted_value = match file {
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
        let current_value = persisted_value.clone();

        Ok(Self {
            path: path.to_owned(),
            persisted_value: RwLock::new(persisted_value),
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

    async fn git_commit_hash(&self, repo: &Url, git_ref: &str) -> Option<String> {
        let lock = self.current_value.read().await;
        let repo_commits = lock.git_commits.get(repo)?;
        let commit = repo_commits.get(git_ref)?;
        Some(commit.to_string())
    }

    async fn set_git_commit_hash(&self, repo: &Url, git_ref: &str, commit: &str) {
        let mut lock = self.current_value.write().await;
        let repo_commits = lock.git_commits.entry(repo.clone()).or_default();
        repo_commits.insert(git_ref.to_string(), commit.to_string());
    }

    // async fn recipe_aux(&self, recipe_ref: &ResolvedRecipeRef) -> Option<RecipeAux> {
    //     let lock = self.current_value.read().await;
    //     let recipe_aux = lock.recipe_aux.get(recipe_ref);
    //     recipe_aux.cloned()
    // }

    async fn set_recipe_aux(&self, recipe_ref: &ResolvedRecipeRef, recipe_aux: RecipeAux) {
        let mut lock = self.current_value.write().await;
        lock.recipe_aux.insert(*recipe_ref, recipe_aux);
    }

    async fn persist(&self) -> anyhow::Result<bool> {
        let mut persisted_value = self.persisted_value.write().await;
        let current_value = self.current_value.read().await;

        if *persisted_value == *current_value {
            // File unchanged
            Ok(false)
        } else {
            let new_content = serde_json::to_vec_pretty(&*current_value)?;

            let mut file = fs::File::create(&self.path).await?;
            tokio::io::copy(&mut &new_content[..], &mut file).await?;

            *persisted_value = current_value.clone();
            Ok(true)
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ContentLock {
    request_hashes: HashMap<Url, Hash>,
    git_commits: HashMap<Url, HashMap<String, String>>,
    recipe_aux: HashMap<ResolvedRecipeRef, RecipeAux>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecipeAux {
    pub lines_stdout: u64,
    pub lines_stderr: u64,
}
