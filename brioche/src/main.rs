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
    pub fn new() -> Result<Self, BriocheError> {
        let ducc = ducc::Ducc::new();

        Self::add_global_scope_vars(&ducc, &ducc.globals())?;

        Ok(JsExec { ducc })
    }

    pub fn eval_module<'a>(&'a self, js: &str, name: Option<&str>)
        -> Result<ducc::Object<'a>, BriocheError>
    {
        self.ducc.with_new_thread_with_new_global_env(|thread| {
            JsExec::add_global_scope_vars(&self.ducc, &thread.globals())?;
            let module = JsExec::add_module_scope_vars(&self.ducc, &thread.globals())?;

            thread.exec(js, name, Default::default())?;

            Ok(module)
        })
    }

    fn add_global_scope_vars(ducc: &ducc::Ducc, global: &ducc::Object)
        -> Result<(), BriocheError>
    {
        global.set("global", global.clone())?;
        global.set("require", ducc.create_function(|invocation| {
            match &invocation.args.into_vec()[..] {
                [ducc::Value::String(string)] => {
                    match string.to_string().as_deref() {
                        Ok("test-module") => Ok("Hello world!".to_string()),
                        _ => Err(ducc::Error::external("Unknown module")),
                    }
                }
                _ => {
                    Err(ducc::Error::external("Invalid arguments"))
                }
            }
        }))?;

        Ok(())
    }

    fn add_module_scope_vars<'ducc>(ducc: &'ducc ducc::Ducc, scope: &ducc::Object)
        -> Result<ducc::Object<'ducc>, BriocheError>
    {
        let module = ducc.create_object();
        let exports = ducc.create_object();

        module.set("exports", exports)?;
        scope.set("module", module.clone())?;

        Ok(module)
    }

    fn get_default_export_from_module(module: ducc::Object)
        -> Result<ducc::Value, BriocheError>
    {
        let exports: ducc::Object = module.get("exports")?;
        let default: ducc::Value = exports.get("default")?;

        Ok(default)
    }
}

fn run(opt: Opt) -> Result<(), BriocheError> {
    let js = fs::read_to_string(&opt.recipe)?;

    let js_exec = JsExec::new()?;
    let module = js_exec.eval_module(&js, opt.recipe.to_str().as_deref())?;
    let result = JsExec::get_default_export_from_module(module)?;
    let result: serde_json::Value = ducc_serde::from_value(result)?;

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
