use std::path::PathBuf;

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

    println!("{:?}", opt);

    Ok(())
}
