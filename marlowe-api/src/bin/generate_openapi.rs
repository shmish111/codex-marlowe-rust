use std::{fs, path::PathBuf};

fn main() {
    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("openapi.json");
    let json = marlowe_api::http::openapi_json().expect("openapi json generation failed");
    fs::write(&output_path, json).expect("failed to write openapi.json");
    println!("wrote {}", output_path.display());
}
