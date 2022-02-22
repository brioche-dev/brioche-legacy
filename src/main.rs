use std::path::PathBuf;

use clap::Parser as _;

mod bake;
mod bootstrap_env;
mod recipe;
mod state;

#[derive(Debug, clap::Parser)]
enum Args {
    Build {
        #[clap(long)]
        repo: PathBuf,
        recipe: String,
    },
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

    let Args::Build { repo, recipe } = opt;

    let state = state::State::new().await?;
    let built_recipe = bake::get_baked_recipe(&state, repo, &recipe).await?;

    println!(
        "Built {} {} to {}",
        built_recipe.recipe.name,
        built_recipe.recipe.version,
        built_recipe.prefix_path.display()
    );

    state.persist_lockfile().await?;
    println!("Persisted lockfile");

    Ok(())
}
