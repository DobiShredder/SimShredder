use std::{env, fs, path::PathBuf, process::ExitCode};

use simshredder_runtime_manager::{
    CatalogPayload, SignedCatalog, sign_catalog_payload, signing_key_from_pkcs8_pem,
};

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let payload_path = PathBuf::from(arguments.next().ok_or(
        "usage: sign-runtime-catalog <payload.json> <private-key.pem> <key-id> <output.json>",
    )?);
    let key_path = PathBuf::from(arguments.next().ok_or("missing private-key.pem")?);
    let key_id = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or("missing or non-UTF-8 key-id")?;
    let output_path = PathBuf::from(arguments.next().ok_or("missing output.json")?);
    if arguments.next().is_some() {
        return Err("too many arguments".into());
    }

    let payload: CatalogPayload =
        serde_json::from_slice(&fs::read(&payload_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let private_key_pem = fs::read_to_string(&key_path).map_err(|error| error.to_string())?;
    let signing_key =
        signing_key_from_pkcs8_pem(&private_key_pem).map_err(|error| error.to_string())?;
    let signature =
        sign_catalog_payload(&payload, &key_id, &signing_key).map_err(|error| error.to_string())?;
    let mut output = serde_json::to_vec_pretty(&SignedCatalog {
        payload,
        signatures: vec![signature],
    })
    .map_err(|error| error.to_string())?;
    output.push(b'\n');
    fs::write(output_path, output).map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
