use std::path::{Path, PathBuf};

use crate::state::State;

pub struct BakedRecipe {
    pub recipe: crate::recipe::RecipeDefinition,
    pub prefix_path: PathBuf,
}

pub async fn get_baked_recipe(
    state: &State,
    repo: impl AsRef<Path>,
    recipe: &str,
) -> anyhow::Result<BakedRecipe> {
    let recipe_path = repo.as_ref().join(recipe);

    let recipe = crate::recipe::eval_recipe(&recipe_path).await?;
    println!("{:#?}", recipe);

    let recipe_hash = crate::recipe::recipe_definition_hash(&recipe)?;
    println!("Recipe hash: {}", hex::encode(recipe_hash));

    if let Some(prefix_path) = state.get_recipe_output(&recipe)? {
        println!("Recipe {} {} already baked", recipe.name, recipe.version);
        return Ok(BakedRecipe {
            prefix_path,
            recipe,
        });
    }

    let bootstrap_env = crate::bootstrap_env::BootstrapEnv::new(&state).await?;
    let recipe_prefix = bootstrap_env.recipe_prefix_path().await?;

    match &recipe.source {
        crate::recipe::RecipeSource::Git { git: repo, git_ref } => {
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
        crate::recipe::RecipeSource::Tarball { tarball } => {
            let source_content_req = crate::state::ContentRequest::new(tarball.parse()?);
            let mut source_content = state.download(source_content_req).await?;

            state
                .unpack_to(&mut source_content, bootstrap_env.host_source_path())
                .await?;
        }
    }

    state.persist_lockfile().await?;
    println!("Persisted lockfile");

    let mut command = crate::bootstrap_env::Command::new("/bin/sh");
    command.current_dir(bootstrap_env.container_source_path());
    command.env("BRIOCHE_PREFIX", &recipe_prefix.container_path);
    command.env("BRIOCHE_BOOTSTRAP_TARGET", bootstrap_env.bootstrap_target());
    command.env("BRIOCHE_BOOTSTRAP_SYSROOT", &recipe_prefix.container_path);

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

    let recipe_build = recipe.build.clone();
    let child_stdin_task = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let mut child_stdin = match child_stdin {
            Some(child_stdin) => child_stdin,
            None => {
                return Ok(());
            }
        };

        std::io::copy(&mut recipe_build.script.as_bytes(), &mut child_stdin)?;

        Ok(())
    });

    let (child_task, child_stdin_task) = tokio::try_join!(child_task, child_stdin_task)?;

    let () = child_task?;
    let () = child_stdin_task?;

    let prefix_path = state
        .save_recipe_output(&recipe, &recipe_prefix.host_output_path)
        .await?;

    state.persist_lockfile().await?;
    println!("Persisted lockfile");

    Ok(BakedRecipe {
        recipe,
        prefix_path,
    })
}
