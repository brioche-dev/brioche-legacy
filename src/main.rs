use std::path::PathBuf;

use clap::Parser as _;

mod bootstrap_env;
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

    let recipe_hash = recipe::recipe_definition_hash(&recipe)?;
    println!("Recipe hash: {}", hex::encode(recipe_hash));

    let bootstrap_env = bootstrap_env::BootstrapEnv::new(&state).await?;

    match &recipe.source {
        recipe::RecipeSource::Git { git: repo, git_ref } => {
            let mut git_command = tokio::process::Command::new("git");
            git_command.arg("clone");
            git_command.arg("--branch").arg(git_ref);
            git_command.arg("--depth").arg("1");
            git_command
                .arg("--")
                .arg(repo)
                .arg(bootstrap_env.host_source_path());
            let git_result = git_command.status().await?;

            if !git_result.success() {
                anyhow::bail!("git clone failed with exit code {}", git_result);
            }
        }
        recipe::RecipeSource::Tarball { tarball } => {
            let source_content_req = state::ContentRequest::new(tarball.parse()?);
            let mut source_content = state.download(source_content_req).await?;

            state
                .unpack_to(&mut source_content, bootstrap_env.host_source_path())
                .await?;
        }
    }

    state.persist_lockfile().await?;
    println!("Persisted lockfile");

    let mut command = bootstrap_env::Command::new("/bin/sh");
    command.current_dir(bootstrap_env.container_source_path());

    let mut child = bootstrap_env.spawn(&command)?;
    let child_stdin = child.take_stdin();

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
        let mut child_stdin = match child_stdin {
            Some(child_stdin) => child_stdin,
            None => {
                return Ok(());
            }
        };

        std::io::copy(&mut recipe.build.script.as_bytes(), &mut child_stdin)?;

        Ok(())
    });

    let (child_task, child_stdin_task) = tokio::try_join!(child_task, child_stdin_task)?;

    let () = child_task?;
    let () = child_stdin_task?;

    state.persist_lockfile().await?;
    println!("Persisted lockfile");

    Ok(())
}
