use err_derive::Error;

pub struct JsExec {
    ducc: ducc::Ducc,
}

impl JsExec {
    pub fn new() -> Result<Self, JsError> {
        let ducc = ducc::Ducc::new();

        Self::add_global_scope_vars(&ducc, &ducc.globals())?;

        Ok(JsExec { ducc })
    }

    pub fn eval_module<'a>(&'a self, js: &str, name: Option<&str>)
        -> Result<ducc::Object<'a>, JsError>
    {
        self.ducc.with_new_thread_with_new_global_env(|thread| {
            JsExec::add_global_scope_vars(&self.ducc, &thread.globals())?;
            let module = JsExec::add_module_scope_vars(&self.ducc, &thread.globals())?;

            thread.exec(js, name, Default::default())?;

            Ok(module)
        })
    }

    fn add_global_scope_vars(ducc: &ducc::Ducc, global: &ducc::Object)
        -> Result<(), JsError>
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
        -> Result<ducc::Object<'ducc>, JsError>
    {
        let module = ducc.create_object();
        let exports = ducc.create_object();

        module.set("exports", exports)?;
        scope.set("module", module.clone())?;

        Ok(module)
    }

    pub fn get_default_export_from_module(module: ducc::Object)
        -> Result<ducc::Value, JsError>
    {
        let exports: ducc::Object = module.get("exports")?;
        let default: ducc::Value = exports.get("default")?;

        Ok(default)
    }
}

#[derive(Debug, Error)]
pub enum JsError {
    #[error(display = "IO error: {}", _0)]
    IoError(#[cause] std::io::Error),

    #[error(display = "Duktape error: {}", _0)]
    DuktapeError(#[cause] ducc::Error),
}
