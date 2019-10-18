#![feature(inner_deref)]

use std::path;
use std::io;
use std::fs;
use serde_json;
use ducc;
use ducc_serde;
use structopt::StructOpt;
use err_derive::Error;

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

    fn eval(&self, js: &str, name: Option<&str>)
        -> Result<serde_json::Value, BriocheError>
    {
        let result: ducc::Value = self.ducc.exec(
            js,
            name,
            ducc::ExecSettings::default()
        )?;
        let result: serde_json::Value = ducc_serde::from_value(result)?;

        Ok(result)
    }
}

fn run(opt: Opt) -> Result<(), BriocheError> {
    let js = fs::read_to_string(&opt.recipe)?;

    let js_exec = JsExec::new();
    let result = js_exec.eval(&js, opt.recipe.to_str().as_deref());

    println!("{:?}", result);

    Ok(())
}

fn main() {
    let opt = Opt::from_args();

    let result = run(opt);
    match result {
        Ok(()) => { },
        Err(err) => { eprintln!("{}", err); }
    }
}

#[derive(Debug, Error)]
enum BriocheError {
    #[error(display = "IO error: {}", _0)]
    IoError(#[cause] io::Error),

    #[error(display = "Duktape error: {}", _0)]
    DuktapeError(#[cause] ducc::Error),
}
