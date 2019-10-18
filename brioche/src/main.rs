use std::path;
use std::fs;
use serde_json;
use ducc;
use ducc_serde;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    recipe: path::PathBuf,
}

fn main() {
    let opt = Opt::from_args();
    let js = fs::read_to_string(&opt.recipe).expect("Failed to read JS file");

    let ducc = ducc::Ducc::new();
    let result: ducc::Value = ducc.exec(
        &js,
        Some("stdin"),
        ducc::ExecSettings::default()
    ).expect("Execution failed");
    let result: serde_json::Value = ducc_serde::from_value(result).unwrap();

    println!("{:?}", result);
}
