#![feature(inner_deref)]

use std::path;
use std::fs;
use structopt::StructOpt;
use err_derive::Error;

mod js;

fn main() {
    let opt = Opt::from_args();

    let result = run(opt);
    match result {
        Ok(()) => { },
        Err(err) => { eprintln!("{}", err); }
    }
}

#[derive(Debug, StructOpt)]
struct Opt {
    recipe: path::PathBuf,
}

fn run(opt: Opt) -> Result<(), BriocheError> {
    let js = fs::read_to_string(&opt.recipe)?;

    let js_exec = js::JsExec::new()?;
    let module = js_exec.eval_module(&js, opt.recipe.to_str().as_deref())?;
    let result = js::JsExec::get_default_export_from_module(module)?;
    let result: serde_json::Value = ducc_serde::from_value(result)
        .map_err(|error| BriocheError::JsModuleToJsonError { error })?;

    println!("{:?}", result);

    Ok(())
}

#[derive(Debug, Error)]
enum BriocheError {
    #[error(display = "IO error: {}", _0)]
    IoError(#[error(cause)] std::io::Error),

    #[error(display = "JS error: {}", _0)]
    JsError(#[error(cause)] js::JsError),

    #[error(display = "Error getting module as a JSON value: {}", error)]
    JsModuleToJsonError { #[error(cause, no_from)] error: ducc::Error },
}
