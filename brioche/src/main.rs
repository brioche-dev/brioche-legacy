#![feature(inner_deref)]

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

struct JsExec {
    ducc: ducc::Ducc,
}

impl JsExec {
    fn new() -> Self {
        let ducc = ducc::Ducc::new();

        JsExec { ducc }
    }

    fn eval(&self, js: &str, name: Option<&str>) -> serde_json::Value {
        let result: ducc::Value = self.ducc.exec(
            js,
            name,
            ducc::ExecSettings::default()
        ).expect("JS execution failed");
        let result: serde_json::Value = ducc_serde::from_value(result).unwrap();

        result
    }
}

fn main() {
    let opt = Opt::from_args();
    let js = fs::read_to_string(&opt.recipe).expect("Failed to read JS file");

    let js_exec = JsExec::new();
    let result = js_exec.eval(&js, opt.recipe.to_str().as_deref());

    println!("{:?}", result);
}
