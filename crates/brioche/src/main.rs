use std::{
    env::temp_dir,
    io::SeekFrom,
    path::{Path, PathBuf},
};

use brioche_common::RecipeSource;
use futures_util::StreamExt as _;
use hex_literal::hex;
use sha2::Digest as _;
use structopt::StructOpt;
use tokio::{
    fs,
    io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _, BufReader},
};
use url::Url;

#[derive(Debug, StructOpt)]
enum Opt {
    Build { path: PathBuf },
}

#[tokio::main]
async fn main() {
    let result = run().await;
    match result {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{:#}", error);
            std::process::exit(1);
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let opt = Opt::from_args();

    let Opt::Build { path } = opt;

    let recipe = brioche_common::eval_recipe(path).await?;

    println!("{:#?}", recipe);

    let temp_dir = temp_dir().join("brioche");
    fs::create_dir_all(&temp_dir).await?;

    let download_dir = temp_dir.join("downloads");
    fs::create_dir_all(&download_dir).await?;

    let alpine_tar_gz = download(
        &download_dir,
        "https://dl-cdn.alpinelinux.org/alpine/v3.15/releases/x86_64/alpine-minirootfs-3.15.0-x86_64.tar.gz".parse()?,
        &hex!("ec7ec80a96500f13c189a6125f2dbe8600ef593b87fc4670fe959dc02db727a2"),
    ).await?;
    let alpine_tar_gz = BufReader::new(alpine_tar_gz);

    let roots_dir = temp_dir.join("roots");
    let _ = fs::remove_dir_all(&roots_dir).await;
    fs::create_dir(&roots_dir).await?;
    let alpine_tar = async_compression::tokio::bufread::GzipDecoder::new(alpine_tar_gz);

    let alpine_root_dir = roots_dir.join("alpine-3.15");
    fs::create_dir(&alpine_root_dir).await?;

    let mut alpine_tar = tokio_tar::Archive::new(alpine_tar);
    alpine_tar.unpack(&alpine_root_dir).await?;

    println!(
        "Unzipped Alpine minirootfs to {}",
        alpine_root_dir.display()
    );

    let overlay_system = roots_dir.join("overlay-system");
    fs::create_dir(&overlay_system).await?;

    let overlay_system_etc = overlay_system.join("etc");
    fs::create_dir(&overlay_system_etc).await?;

    fs::copy("/etc/resolv.conf", overlay_system_etc.join("resolv.conf")).await?;

    let source_dir = overlay_system.join("usr").join("src");
    fs::create_dir_all(&source_dir).await?;

    match &recipe.source {
        RecipeSource::Git { git: repo, git_ref } => {
            let mut git_command = tokio::process::Command::new("git");
            git_command.arg("clone");
            git_command.arg("--branch").arg(git_ref);
            git_command.arg("--depth").arg("1");
            git_command.arg("--").arg(repo).arg(&source_dir);
            let git_result = git_command.status().await?;

            if !git_result.success() {
                anyhow::bail!("git clone failed with exit code {}", git_result);
            }
        }
    }

    let overlay_workdir = roots_dir.join("overlay-workdir");
    fs::create_dir(&overlay_workdir).await?;

    let output_dir = roots_dir.join("output");
    fs::create_dir(&output_dir).await?;

    let overlay_dir = roots_dir.join("overlay");
    fs::create_dir(&overlay_dir).await?;

    let mut command = unshare::Command::new("/bin/sh");
    command.reset_fds();
    command.env_clear();
    command.chroot_dir(&overlay_dir);
    command.current_dir("/usr/src");
    command.unshare([
        &unshare::Namespace::Ipc,
        &unshare::Namespace::Mount,
        &unshare::Namespace::Pid,
        &unshare::Namespace::User,
    ]);

    let current_uid = nix::unistd::Uid::current().as_raw();
    let current_gid = nix::unistd::Gid::current().as_raw();
    let newuidmap = which::which("newuidmap")?;
    let newgidmap = which::which("newgidmap")?;
    command.set_id_map_commands(&newuidmap, &newgidmap);
    command.set_id_maps(
        vec![unshare::UidMap {
            outside_uid: current_uid,
            inside_uid: 0,
            count: 1,
        }],
        vec![unshare::GidMap {
            outside_gid: current_gid,
            inside_gid: 0,
            count: 1,
        }],
    );
    command.uid(0);
    command.gid(0);

    command.before_chroot(move || {
        let alpine_root_dir = alpine_root_dir.clone();
        let overlay_system = overlay_system.clone();
        let output_dir = output_dir.clone();
        let overlay_workdir = overlay_workdir.clone();
        let overlay_dir = overlay_dir.clone();
        let setup_env = move || -> anyhow::Result<()> {
            // NOTE: This doesn't seem to work in WSL, possibly because the
            // WSL kernel doesn't have the patch to enable overlayfs in user
            // namespaces.
            // libmount::Overlay::writable(
            //     [&*alpine_root_dir, &*overlay_system].into_iter(),
            //     &output_dir,
            //     &overlay_workdir,
            //     &overlay_dir,
            // )
            // .mount()
            // .map_err(|error| anyhow::anyhow!("{}", error))?;

            let mut overlayfs_command = std::process::Command::new("fuse-overlayfs");
            overlayfs_command.arg("-o").arg(format!(
                "lowerdir={}:{}",
                alpine_root_dir.display(),
                overlay_system.display()
            ));
            overlayfs_command
                .arg("-o")
                .arg(format!("upperdir={}", output_dir.display()));
            overlayfs_command
                .arg("-o")
                .arg(format!("workdir={}", overlay_workdir.display()));
            overlayfs_command.arg(&overlay_dir);

            let overlayfs_status = overlayfs_command.status()?;
            if !overlayfs_status.success() {
                anyhow::bail!(
                    "mounting overlayfs failed with exit code {}",
                    overlayfs_status
                );
            }

            libmount::BindMount::new("/proc", overlay_dir.join("proc"))
                .mount()
                .map_err(|error| anyhow::anyhow!("{}", error))?;
            libmount::BindMount::new("/sys", overlay_dir.join("sys"))
                .mount()
                .map_err(|error| anyhow::anyhow!("{}", error))?;
            libmount::BindMount::new("/dev", overlay_dir.join("dev"))
                .mount()
                .map_err(|error| anyhow::anyhow!("{}", error))?;

            Ok(())
        };

        let result = setup_env();
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                eprintln!("failed to set up system mounts: {}", error);
                Err(std::io::Error::new(std::io::ErrorKind::Other, error))
            }
        }
    });

    let mut child = command
        .spawn()
        .map_err(|error| anyhow::anyhow!("failed to spawn child process: {}", error))?;

    let child_task = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let exit_status = child.wait()?;

        match exit_status {
            unshare::ExitStatus::Exited(0) => Ok(()),
            unshare::ExitStatus::Exited(exit_code) => {
                anyhow::bail!("process exited with code {}", exit_code);
            }
            unshare::ExitStatus::Signaled(signal, _) => {
                anyhow::bail!("process exited with signal {}", signal.as_str());
            }
        }
    });

    let () = child_task.await??;

    Ok(())
}

async fn download(
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
