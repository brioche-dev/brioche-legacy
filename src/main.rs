use std::{env::temp_dir, path::PathBuf};

use clap::Parser as _;
use futures_util::TryStreamExt as _;
use hex_literal::hex;
use tokio::{fs, io::BufReader};

mod content;
mod recipe;
mod state;

#[derive(Debug, clap::Parser)]
enum Args {
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
    let opt = Args::parse();

    let Args::Build { path } = opt;

    let state = state::State::new().await?;

    let recipe = recipe::eval_recipe(path).await?;

    println!("{:#?}", recipe);

    let temp_dir = temp_dir().join("brioche");
    fs::create_dir_all(&temp_dir).await?;

    let download_dir = temp_dir.join("downloads");
    fs::create_dir_all(&download_dir).await?;

    let alpine_tar_gz = content::download(
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
        recipe::RecipeSource::Git { git: repo, git_ref } => {
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
        recipe::RecipeSource::Tarball { tarball } => {
            let response = reqwest::get(&*tarball).await?;
            let tar_gz_bytes = response
                .bytes_stream()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error));
            let tar_gz_bytes = tokio_util::io::StreamReader::new(tar_gz_bytes);

            let tar_bytes = async_compression::tokio::bufread::GzipDecoder::new(tar_gz_bytes);

            let mut tar = tokio_tar::Archive::new(tar_bytes);
            tar.unpack(&source_dir).await?;
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
    command.envs([
        (
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        ),
        ("HOME", "/root"),
    ]);
    command.chroot_dir(&overlay_dir);
    command.current_dir("/usr/src");
    command.unshare([
        &unshare::Namespace::Ipc,
        &unshare::Namespace::Mount,
        &unshare::Namespace::Pid,
        &unshare::Namespace::User,
    ]);
    command.stdin(unshare::Stdio::Pipe);

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

    let child_stdin = child.stdin.take();

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

    let child_stdin_task = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        use std::io::Write as _;

        let mut child_stdin = match child_stdin {
            Some(child_stdin) => child_stdin,
            None => {
                return Ok(());
            }
        };

        write!(child_stdin, "{}", recipe.build.script)?;

        Ok(())
    });

    let (child_task, child_stdin_task) = tokio::try_join!(child_task, child_stdin_task)?;

    let () = child_task?;
    let () = child_stdin_task?;

    Ok(())
}
