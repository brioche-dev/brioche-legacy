use std::io::{self, Read as _};
use serde_json;
use ducc;
use ducc_serde;

fn main() {
    let mut js = String::new();

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    stdin.read_to_string(&mut js).expect("Failed to read JS from stdin");

    let ducc = ducc::Ducc::new();
    let result: ducc::Value = ducc.exec(
        &js,
        Some("stdin"),
        ducc::ExecSettings::default()
    ).expect("Execution failed");
    let result: serde_json::Value = ducc_serde::from_value(result).unwrap();

    println!("{:?}", result);
}
