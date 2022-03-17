use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Display,
    path::{Path, PathBuf},
};

use tokio::{fs::File, io::AsyncReadExt as _};
use url::Url;

use crate::{hash::Hash, state::State};

#[async_recursion::async_recursion]
pub async fn resolve_recipe(
    state: &State,
    repo: &Path,
    name: &str,
    recipe_set: &mut ResolvedRecipeSet,
) -> anyhow::Result<ResolvedRecipeRef> {
    let recipe = eval_recipe(repo.join(name)).await?;

    let resolved_source = match &recipe.source {
        crate::recipe::RecipeSource::Git { git: repo, git_ref } => {
            let repo: Url = repo.parse()?;
            let git_checkout_req = crate::state::GitCheckoutRequest::new(repo, git_ref);
            let git_checkout = state.git_checkout(git_checkout_req).await?;

            ResolvedRecipeSource::Git(git_checkout)
        }
        crate::recipe::RecipeSource::Tarball { tarball } => {
            let source_content_req = crate::state::ContentRequest::new(tarball.parse()?);
            let source_content = state.download(source_content_req).await?;

            ResolvedRecipeSource::Tarball(source_content)
        }
    };
    let resolved_source_ref = recipe_set.insert_source(resolved_source);

    let mut resolved_dependencies = BTreeSet::new();
    for (dependency_name, _dependency_version) in &recipe.dependencies {
        // TODO: Use dependency version to resolve dependency
        let resolved_dependency = resolve_recipe(state, repo, dependency_name, recipe_set).await?;
        resolved_dependencies.insert(resolved_dependency);
    }

    let resolved_recipe = ResolvedRecipe {
        name: recipe.name,
        version: recipe.version,
        source: resolved_source_ref,
        dependencies: resolved_dependencies,
        build: recipe.build,
    };

    Ok(recipe_set.insert(resolved_recipe))
}

async fn eval_recipe(path: impl AsRef<Path>) -> anyhow::Result<RecipeDefinition> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct ResolvedRecipeRef {
    hash: Hash,
}

impl ResolvedRecipeRef {
    pub fn to_path_component(&self) -> PathBuf {
        self.hash.to_path_component()
    }
}

impl Display for ResolvedRecipeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.hash.fmt(f)
    }
}

#[derive(Debug)]
pub struct ResolvedRecipeSet {
    definitions: BTreeMap<ResolvedRecipeRef, ResolvedRecipe>,
    sources: BTreeMap<ResolvedRecipeSourceRef, ResolvedRecipeSource>,
}

impl ResolvedRecipeSet {
    pub fn new() -> Self {
        Self {
            definitions: BTreeMap::new(),
            sources: BTreeMap::new(),
        }
    }

    pub fn get(&self, recipe_ref: &ResolvedRecipeRef) -> &ResolvedRecipe {
        self.definitions
            .get(recipe_ref)
            .expect("Recipe reference not found in recipe set")
    }

    pub fn get_source(&self, source_ref: &ResolvedRecipeSourceRef) -> &ResolvedRecipeSource {
        self.sources
            .get(source_ref)
            .expect("Recipe source reference not found in recipe set")
    }

    fn insert(&mut self, recipe: ResolvedRecipe) -> ResolvedRecipeRef {
        use sha2::Digest as _;

        let cjson_bytes = cjson::to_vec(&recipe).expect("Failed to canonicalize JSON");

        let mut cjson_hash = sha2::Sha256::new();
        cjson_hash.update(&cjson_bytes);
        let hash = Hash::from_digest(cjson_hash);

        let recipe_ref = ResolvedRecipeRef { hash };

        self.definitions.insert(recipe_ref, recipe);

        recipe_ref
    }

    fn insert_source(&mut self, source: ResolvedRecipeSource) -> ResolvedRecipeSourceRef {
        let source_ref = match source {
            ResolvedRecipeSource::Git(ref git_checkout) => ResolvedRecipeSourceRef::Git {
                commit: git_checkout.commit.to_string(),
            },
            ResolvedRecipeSource::Tarball(ref tarball_file) => ResolvedRecipeSourceRef::Tarball {
                hash: tarball_file.content_hash,
            },
        };

        self.sources.insert(source_ref.clone(), source);

        source_ref
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedRecipe {
    pub name: String,
    pub version: String,
    pub source: ResolvedRecipeSourceRef,
    pub dependencies: BTreeSet<ResolvedRecipeRef>,
    pub build: RecipeBuildScript,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum ResolvedRecipeSourceRef {
    Git { commit: String },
    Tarball { hash: Hash },
}

#[derive(Debug)]
pub enum ResolvedRecipeSource {
    Git(crate::state::GitCheckout),
    Tarball(crate::state::ContentFile),
}
