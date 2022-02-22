use std::{env, path::PathBuf};

use anyhow::Context as _;
use tokio::fs;

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
}
