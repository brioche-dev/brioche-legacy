use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use hex_literal::hex;
use joinery::JoinableIterator;
use tokio::{fs, io::BufReader};

use crate::{content, state::State};

pub struct BootstrapEnv {
    inputs_dir: PathBuf,
    source_relative_dir: PathBuf,
    chroot_config: ChrootConfig,
}

impl BootstrapEnv {
    pub async fn new(state: &State) -> anyhow::Result<Self> {
        let work_dir = state.new_temp_work_dir().await?;

        let alpine_root_dir = work_dir.join("layers").join("alpine-root");
        fs::create_dir_all(&alpine_root_dir).await?;

        let inputs_dir = work_dir.join("layers").join("inputs");
        fs::create_dir_all(&inputs_dir).await?;

        let overlayfs_work_dir = work_dir.join("layers").join("work-dir");
        fs::create_dir_all(&overlayfs_work_dir).await?;

        let outputs_dir = work_dir.join("layers").join("outputs");
        fs::create_dir_all(&outputs_dir).await?;

        let overlay_dir = work_dir.join("overlay");
        fs::create_dir_all(&overlay_dir).await?;

        let alpine_tar_gz = content::download(
            &state,
            "https://dl-cdn.alpinelinux.org/alpine/v3.15/releases/x86_64/alpine-minirootfs-3.15.0-x86_64.tar.gz".parse()?,
            &hex!("ec7ec80a96500f13c189a6125f2dbe8600ef593b87fc4670fe959dc02db727a2"),
        ).await?;
        let alpine_tar_gz = BufReader::new(alpine_tar_gz);
        let alpine_tar = async_compression::tokio::bufread::GzipDecoder::new(alpine_tar_gz);
        let mut alpine_archive = tokio_tar::Archive::new(alpine_tar);
        alpine_archive.unpack(&alpine_root_dir).await?;

        println!(
            "Unzipped Alpine minirootfs to {}",
            alpine_root_dir.display()
        );

        fs::create_dir_all(inputs_dir.join("etc")).await?;
        fs::copy(
            "/etc/resolv.conf",
            inputs_dir.join("etc").join("resolv.conf"),
        )
        .await?;

        let source_relative_dir = PathBuf::new().join("usr").join("src");
        fs::create_dir_all(inputs_dir.join(&source_relative_dir)).await?;

        let chroot_config = ChrootConfig {
            lower_dirs: vec![alpine_root_dir, inputs_dir.clone()],
            upper_dir: outputs_dir,
            work_dir: overlayfs_work_dir,
            target_dir: overlay_dir,
        };

        Ok(Self {
            inputs_dir,
            source_relative_dir,
            chroot_config,
        })
    }

    pub fn host_source_path(&self) -> PathBuf {
        self.inputs_dir.join(&self.source_relative_dir)
    }

    pub fn container_source_path(&self) -> PathBuf {
        PathBuf::from("/").join(&self.source_relative_dir)
    }

    pub fn spawn(&self, command: &Command) -> anyhow::Result<Child> {
        let mut spawn_cmd = unshare::Command::new(&command.program);
        spawn_cmd.reset_fds();
        spawn_cmd.env_clear();
        spawn_cmd.envs([
            (
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            ),
            ("HOME", "/root"),
        ]);
        spawn_cmd.chroot_dir(&self.chroot_config.target_dir);

        spawn_cmd.args(&command.args);
        spawn_cmd.envs(&command.env);
        if let Some(current_dir) = &command.current_dir {
            spawn_cmd.current_dir(current_dir);
        }

        spawn_cmd.unshare([
            &unshare::Namespace::Ipc,
            &unshare::Namespace::Mount,
            &unshare::Namespace::Pid,
            &unshare::Namespace::User,
        ]);
        spawn_cmd.stdin(unshare::Stdio::Pipe);

        let current_uid = nix::unistd::Uid::current().as_raw();
        let current_gid = nix::unistd::Gid::current().as_raw();
        let newuidmap = which::which("newuidmap")?;
        let newgidmap = which::which("newgidmap")?;
        spawn_cmd.set_id_map_commands(&newuidmap, &newgidmap);
        spawn_cmd.set_id_maps(
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
        spawn_cmd.uid(0);
        spawn_cmd.gid(0);

        let chroot_config = self.chroot_config.clone();
        spawn_cmd.before_chroot(move || {
            let chroot_config = chroot_config.clone();
            let mount_result = chroot_config.mount();
            match mount_result {
                Ok(()) => Ok(()),
                Err(error) => {
                    eprintln!("failed to set up system mounts: {}", error);
                    Err(std::io::Error::new(std::io::ErrorKind::Other, error))
                }
            }
        });

        let child = spawn_cmd
            .spawn()
            .map_err(|error| anyhow::anyhow!("failed to spawn child process: {}", error))?;

        Ok(Child { child })
    }
}

pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    env: HashMap<OsString, OsString>,
    current_dir: Option<PathBuf>,
}

impl Command {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: vec![],
            env: HashMap::new(),
            current_dir: None,
        }
    }

    // pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
    //     self.args.push(arg.as_ref().to_owned());
    //     self
    // }

    // pub fn env(&mut self, var: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
    //     self.env
    //         .insert(var.as_ref().to_owned(), value.as_ref().to_owned());
    //     self
    // }

    pub fn current_dir(&mut self, current_dir: impl AsRef<Path>) -> &mut Self {
        self.current_dir = Some(current_dir.as_ref().to_owned());
        self
    }
}

pub struct Child {
    child: unshare::Child,
}

impl Child {
    pub fn take_stdin(&mut self) -> Option<impl std::io::Write> {
        self.child.stdin.take()
    }

    pub fn wait(&mut self) -> anyhow::Result<unshare::ExitStatus> {
        let exit_status = self.child.wait()?;
        Ok(exit_status)
    }
}

#[derive(Debug, Clone)]
struct ChrootConfig {
    lower_dirs: Vec<PathBuf>,
    upper_dir: PathBuf,
    work_dir: PathBuf,
    target_dir: PathBuf,
}

impl ChrootConfig {
    fn mount(self) -> anyhow::Result<()> {
        let lower_dirs = self
            .lower_dirs
            .iter()
            .map(|dir| dir.display())
            .join_with(":");
        let mut overlayfs_command = std::process::Command::new("fuse-overlayfs");
        overlayfs_command
            .arg("-o")
            .arg(format!("lowerdir={}", lower_dirs));
        overlayfs_command
            .arg("-o")
            .arg(format!("upperdir={}", self.upper_dir.display()));
        overlayfs_command
            .arg("-o")
            .arg(format!("workdir={}", self.work_dir.display()));
        overlayfs_command.arg(&self.target_dir);

        let overlayfs_status = overlayfs_command.status()?;
        if !overlayfs_status.success() {
            anyhow::bail!(
                "mounting overlayfs failed with exit code {}",
                overlayfs_status
            );
        }

        libmount::BindMount::new("/proc", self.target_dir.join("proc"))
            .mount()
            .map_err(|error| anyhow::anyhow!("{}", error))?;
        libmount::BindMount::new("/sys", self.target_dir.join("sys"))
            .mount()
            .map_err(|error| anyhow::anyhow!("{}", error))?;
        libmount::BindMount::new("/dev", self.target_dir.join("dev"))
            .mount()
            .map_err(|error| anyhow::anyhow!("{}", error))?;

        Ok(())
    }
}
