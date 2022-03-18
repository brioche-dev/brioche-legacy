use std::path::PathBuf;

use clap::Parser as _;

mod bake;
mod bootstrap_env;
mod hash;
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

    let mut recipe_set = recipe::ResolvedRecipeSet::new();
    let resolved_recipe = recipe::resolve_recipe(&state, &*repo, &recipe, &mut recipe_set).await?;
    let baked_recipe = bake::get_baked_recipe(&state, &recipe_set, &resolved_recipe).await?;

    let recipe = recipe_set.get(&resolved_recipe);

    println!(
        "Built {} {} to {}",
        recipe.name,
        recipe.version,
        baked_recipe.prefix_path.display()
    );

    match state.persist_lockfile().await? {
        true => {
            println!("Updated lockfile");
        }
        false => {
            println!("Lockfile already up to date");
        }
    }

    Ok(())
}
