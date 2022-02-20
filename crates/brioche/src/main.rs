use std::{collections::HashMap, ffi::OsString, fs::File, io::Read as _, path::PathBuf};

use rquickjs::FromJs;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
enum Opt {
    Build { path: PathBuf },
}

fn main() {
    let result = run();
    match result {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{:#}", error);
            std::process::exit(1);
        }
    }
}

fn run() -> anyhow::Result<()> {
    let opt = Opt::from_args();

    let Opt::Build { path } = opt;

    let recipe_path = path.join("brioche.js");
    let mut recipe_file = File::open(&recipe_path)?;
    let mut recipe_contents = vec![];
    recipe_file.read_to_end(&mut recipe_contents)?;

    let runtime = rquickjs::Runtime::new()?;
    let context = rquickjs::Context::full(&runtime)?;
    context.with(move |ctx| -> anyhow::Result<()> {
        let module_name = recipe_path.to_string_lossy();
        let module = rquickjs::Module::new(ctx, module_name.as_bytes(), recipe_contents)?;
        let module = module.eval()?;
        let recipe: Recipe = module.get("recipe")?;

        println!("{:#?}", recipe);

        let recipe_def: RecipeDefinition = recipe.definition.call(())?;

        println!("{:#?}", recipe_def);

        Ok(())
    })?;

    Ok(())
}

#[derive(Debug, rquickjs::FromJs)]
struct Recipe<'js> {
    options: RecipeOptions,
    definition: rquickjs::Function<'js>,
}

#[derive(Debug, rquickjs::FromJs)]
struct RecipeOptions {}

#[derive(Debug, rquickjs::FromJs)]
struct RecipeDefinition {
    name: String,
    version: String,
    source: RecipeSource,
    dependencies: HashMap<String, String>,
    build: RecipeBuildScript,
}

#[derive(Debug, rquickjs::FromJs)]
#[quickjs(untagged)]
enum RecipeSource {
    Git {
        git: String,
        #[quickjs(rename = "ref")]
        git_ref: String,
    },
}

#[derive(Debug, rquickjs::FromJs)]
#[quickjs(rename_all = "camelCase")]
struct RecipeBuildScript {
    shell: String,
    script: String,
    env_vars: HashMap<String, String>,
}
