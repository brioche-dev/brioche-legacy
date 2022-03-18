use std::path::PathBuf;

use tokio::fs;

use crate::{
    recipe::{ResolvedRecipeRef, ResolvedRecipeSet},
    state::State,
};

pub struct BakedRecipe {
    pub recipe_ref: ResolvedRecipeRef,
    pub prefix_path: PathBuf,
}

#[async_recursion::async_recursion]
pub async fn get_baked_recipe(
    state: &State,
    recipe_set: &ResolvedRecipeSet,
    recipe_ref: &ResolvedRecipeRef,
) -> anyhow::Result<BakedRecipe> {
    let recipe = recipe_set.get(recipe_ref);
    if let Some(prefix_path) = state.get_recipe_output(recipe_ref)? {
        println!("Recipe {} {} already baked", recipe.name, recipe.version);
        return Ok(BakedRecipe {
            recipe_ref: *recipe_ref,
            prefix_path,
        });
    }

    let bootstrap_env = crate::bootstrap_env::BootstrapEnv::new(&state).await?;
    let recipe_prefix = bootstrap_env.recipe_prefix_path();

    state.persist_lockfile().await?;
    println!("Persisted lockfile");

    for dependency_ref in &recipe.dependencies {
        let dependency_recipe = get_baked_recipe(state, recipe_set, dependency_ref).await?;

        // Copy each entry from the recipe into the prefix path

        let mut cp_command = tokio::process::Command::new("cp");
        cp_command.arg("-a");
        cp_command.arg("-r");

        let mut entries = fs::read_dir(&dependency_recipe.prefix_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            cp_command.arg(&entry.path());
        }

        cp_command.arg(&recipe_prefix.host_input_path);

        let cp_result = cp_command.spawn()?.wait().await?;
        if !cp_result.success() {
            anyhow::bail!(
                "failed to copy dependency {} from {} to {}",
                dependency_ref,
                dependency_recipe.prefix_path.display(),
                recipe_prefix.host_input_path.display(),
            );
        }
    }

    let host_source_path = bootstrap_env.host_source_path();
    let source = recipe_set.get_source(&recipe.source);
    match &source {
        crate::recipe::ResolvedRecipeSource::Git(git_checkout) => {
            let mut cp_command = tokio::process::Command::new("cp");
            cp_command.arg("-a");
            cp_command.arg("-r");
            cp_command.arg(&git_checkout.checkout_path);
            cp_command.arg(&host_source_path);

            let cp_result = cp_command.spawn()?.wait().await?;
            if !cp_result.success() {
                anyhow::bail!(
                    "failed to copy git source from {} to {}",
                    git_checkout.checkout_path.display(),
                    host_source_path.display(),
                );
            }
        }
        crate::recipe::ResolvedRecipeSource::Tarball(content_file) => {
            let mut content_file = content_file.try_clone().await?;
            state
                .unpack_to(&mut content_file, bootstrap_env.host_source_path())
                .await?;
        }
    }

    let mut command = crate::bootstrap_env::Command::new("/bin/sh");
    command.current_dir(bootstrap_env.container_source_path());
    command.env("BRIOCHE_PREFIX", &recipe_prefix.container_path);
    command.env("BRIOCHE_BOOTSTRAP_TARGET", bootstrap_env.bootstrap_target());

    let mut child = bootstrap_env.spawn(&command)?;
    let child_stdin = child.take_stdin();
    let child_stdout = child.take_stdout();
    let child_stderr = child.take_stderr();

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
    let child_stdout_task = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let mut child_stdout = match child_stdout {
            Some(child_stdout) => child_stdout,
            None => {
                return Ok(());
            }
        };

        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();
        std::io::copy(&mut child_stdout, &mut stdout)?;

        Ok(())
    });
    let child_stderr_task = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let mut child_stderr = match child_stderr {
            Some(child_stderr) => child_stderr,
            None => {
                return Ok(());
            }
        };

        let stderr = std::io::stderr();
        let mut stderr = stderr.lock();
        std::io::copy(&mut child_stderr, &mut stderr)?;

        Ok(())
    });

    let (child_task, child_stdin_task, child_stdout_task, child_stderr_task) = tokio::try_join!(
        child_task,
        child_stdin_task,
        child_stdout_task,
        child_stderr_task
    )?;

    let () = child_task?;
    let () = child_stdin_task?;
    let () = child_stdout_task?;
    let () = child_stderr_task?;

    let prefix_path = state
        .save_recipe_output(&recipe_ref, &recipe_prefix.host_output_path)
        .await?;

    state.persist_lockfile().await?;
    println!("Persisted lockfile");

    Ok(BakedRecipe {
        recipe_ref: *recipe_ref,
        prefix_path,
    })
}
