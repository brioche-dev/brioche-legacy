use std::{
    env::temp_dir,
    io::SeekFrom,
    path::{Path, PathBuf},
};

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
    fs::create_dir_all(&roots_dir).await?;
    let alpine_tar = async_compression::tokio::bufread::GzipDecoder::new(alpine_tar_gz);

    let alpine_root_dir = roots_dir.join("alpine-3.15");
    let _ = fs::remove_dir_all(&alpine_root_dir).await;
    fs::create_dir(&alpine_root_dir).await?;

    let mut alpine_tar = tokio_tar::Archive::new(alpine_tar);
    alpine_tar.unpack(&alpine_root_dir).await?;

    println!(
        "Unzipped Alpine minirootfs to {}",
        alpine_root_dir.display()
    );

    let mut command = unshare::Command::new("/bin/sh");
    command.reset_fds();
    command.env_clear();
    command.chroot_dir(alpine_root_dir);
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
