use std::{collections::HashMap, path::Path};

use tokio::{fs::File, io::AsyncReadExt as _};

pub async fn eval_recipe(path: impl AsRef<Path>) -> anyhow::Result<RecipeDefinition> {
    let path = path.as_ref();
    let recipe_path = path.join("brioche.js");
    let mut recipe_file = File::open(&recipe_path).await?;
    let mut recipe_contents = vec![];
    recipe_file.read_to_end(&mut recipe_contents).await?;

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

pub fn recipe_definition_hash(recipe: &RecipeDefinition) -> anyhow::Result<[u8; 32]> {
    use sha2::Digest as _;

    let cjson_bytes = cjson::to_vec(&recipe)
        .map_err(|error| anyhow::anyhow!("faiiled to canonicalize json: {:?}", error))?;

    let mut cjson_hash = sha2::Sha256::new();
    cjson_hash.update(&cjson_bytes);
    let cjson_hash = cjson_hash.finalize();

    Ok(cjson_hash.try_into().expect("could not convert hash"))
}

#[derive(Debug, rquickjs::FromJs)]
struct Recipe<'js> {
    definition: rquickjs::Function<'js>,
}

#[derive(Debug, serde::Serialize, rquickjs::FromJs)]
pub struct RecipeDefinition {
    pub name: String,
    pub version: String,
    pub source: RecipeSource,
    pub dependencies: HashMap<String, String>,
    pub build: RecipeBuildScript,
}

#[derive(Debug, serde::Serialize, rquickjs::FromJs)]
#[quickjs(untagged)]
#[serde(untagged)]
pub enum RecipeSource {
    Git {
        git: String,
        #[quickjs(rename = "ref")]
        #[serde(rename = "ref")]
        git_ref: String,
    },
    Tarball {
        tarball: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, rquickjs::FromJs)]
#[quickjs(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct RecipeBuildScript {
    pub shell: String,
    pub script: String,
    pub env_vars: HashMap<String, String>,
}
