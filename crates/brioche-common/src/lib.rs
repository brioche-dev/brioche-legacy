use std::{collections::HashMap, fs::File, io::Read as _, path::Path};

pub fn eval_recipe(path: impl AsRef<Path>) -> anyhow::Result<RecipeDefinition> {
    let path = path.as_ref();
    let recipe_path = path.join("brioche.js");
    let mut recipe_file = File::open(&recipe_path)?;
    let mut recipe_contents = vec![];
    recipe_file.read_to_end(&mut recipe_contents)?;

    let runtime = rquickjs::Runtime::new()?;
    let context = rquickjs::Context::full(&runtime)?;
    let recipe_def = context.with(move |ctx| -> anyhow::Result<_> {
        let module_name = path.to_string_lossy();
        let module = rquickjs::Module::new(ctx, module_name.as_bytes(), recipe_contents)?;
        let module = module.eval()?;
        let recipe: Recipe = module.get("recipe")?;

        let recipe_def: RecipeDefinition = recipe.definition.call(())?;

        Ok(recipe_def)
    })?;

    Ok(recipe_def)
}

#[derive(Debug, rquickjs::FromJs)]
struct Recipe<'js> {
    definition: rquickjs::Function<'js>,
}

#[derive(Debug, rquickjs::FromJs)]
pub struct RecipeDefinition {
    pub name: String,
    pub version: String,
    pub source: RecipeSource,
    pub dependencies: HashMap<String, String>,
    pub build: RecipeBuildScript,
}

#[derive(Debug, rquickjs::FromJs)]
#[quickjs(untagged)]
pub enum RecipeSource {
    Git {
        git: String,
        #[quickjs(rename = "ref")]
        git_ref: String,
    },
}

#[derive(Debug, rquickjs::FromJs)]
#[quickjs(rename_all = "camelCase")]
pub struct RecipeBuildScript {
    pub shell: String,
    pub script: String,
    pub env_vars: HashMap<String, String>,
}
