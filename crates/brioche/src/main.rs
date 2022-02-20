use std::path::PathBuf;

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
enum Opt {
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
    let opt = Opt::from_args();

    let Opt::Build { path } = opt;

    let recipe = brioche_common::eval_recipe(path).await?;

    println!("{:#?}", recipe);

    Ok(())
}
