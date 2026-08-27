use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::Write,
    path::Path,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Error, Result, SimcIdentity, cancel_after, run_with_timeout, validate_supported_binary,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactContract {
    pub exit_code: i32,
    pub json_bytes: u64,
    pub html_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContractReport {
    pub identity: SimcIdentity,
    pub quick: ArtifactContract,
    pub profileset: ArtifactContract,
    pub invalid_input_exit_code: i32,
    pub cancel_exit_code: Option<i32>,
    pub cancel_elapsed_millis: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkSample {
    pub threads: usize,
    pub profileset_work_threads: usize,
    pub repetition: usize,
    pub elapsed_millis: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub identity: SimcIdentity,
    pub logical_cpus: usize,
    pub iterations: usize,
    pub samples: Vec<BenchmarkSample>,
    pub median_millis: BTreeMap<String, u128>,
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn run_report(
    executable: &Path,
    fixture: &Path,
    output_directory: &Path,
    stem: &str,
) -> Result<(ArtifactContract, Value)> {
    let json_path = output_directory.join(format!("{stem}.json"));
    let html_path = output_directory.join(format!("{stem}.html"));
    let arguments = [
        fixture.as_os_str().to_owned(),
        OsString::from(format!("json2={}", json_path.display())),
        OsString::from(format!("html={}", html_path.display())),
        OsString::from("report_progress=0"),
    ];
    let output = run_with_timeout(
        executable,
        arguments,
        output_directory,
        Duration::from_secs(60),
    )?;
    write_bytes(
        &output_directory.join(format!("{stem}.stdout.log")),
        output.stdout.as_bytes(),
    )?;
    write_bytes(
        &output_directory.join(format!("{stem}.stderr.log")),
        output.stderr.as_bytes(),
    )?;
    if !output.status.success() {
        return Err(Error::Contract(format!(
            "{stem} fixture exited with {}",
            output.status
        )));
    }
    let json_bytes = fs::read(&json_path)?;
    let html_bytes = fs::read(&html_path)?;
    if !html_bytes.starts_with(b"<!DOCTYPE html") {
        return Err(Error::Contract(format!(
            "{stem} HTML does not have the expected document prefix"
        )));
    }
    let document = serde_json::from_slice(&json_bytes)?;
    Ok((
        ArtifactContract {
            exit_code: output.status.code().unwrap_or_default(),
            json_bytes: json_bytes.len() as u64,
            html_bytes: html_bytes.len() as u64,
        },
        document,
    ))
}

fn object_keys(value: &Value) -> Result<Vec<String>> {
    let mut keys = value
        .as_object()
        .ok_or_else(|| Error::Contract("expected a JSON object".into()))?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    Ok(keys)
}

fn quick_projection(document: &Value) -> Result<Value> {
    Ok(json!({
        "version": document.pointer("/version"),
        "git_revision": document.pointer("/git_revision"),
        "report_version": document.pointer("/report_version"),
        "options": {
            "iterations": document.pointer("/sim/options/iterations"),
            "threads": document.pointer("/sim/options/threads"),
            "seed": document.pointer("/sim/options/seed"),
            "version_used": document.pointer("/sim/options/dbc/version_used"),
        },
        "player": {
            "name": document.pointer("/sim/players/0/name"),
            "level": document.pointer("/sim/players/0/level"),
            "role": document.pointer("/sim/players/0/role"),
            "specialization": document.pointer("/sim/players/0/specialization"),
            "collected_data_keys": object_keys(
                document.pointer("/sim/players/0/collected_data")
                    .ok_or_else(|| Error::Contract("quick collected_data is missing".into()))?
            )?,
        }
    }))
}

fn profileset_projection(document: &Value) -> Result<Value> {
    let source = document
        .pointer("/sim/profilesets/results")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Contract("profileset results are missing".into()))?;
    let mut results = source
        .iter()
        .map(|result| {
            Ok(json!({
                "name": result.get("name"),
                "iterations": result.get("iterations"),
                "keys": object_keys(result)?,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    results.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    Ok(json!({
        "version": document.pointer("/version"),
        "git_revision": document.pointer("/git_revision"),
        "metric": document.pointer("/sim/profilesets/metric"),
        "results": results,
    }))
}

fn verify_golden(actual: &Value, golden: &Path, label: &str) -> Result<()> {
    let expected: Value = serde_json::from_slice(&fs::read(golden)?)?;
    if actual != &expected {
        return Err(Error::Contract(format!(
            "{label} JSON contract differs from {}\nexpected: {}\nactual: {}",
            golden.display(),
            serde_json::to_string_pretty(&expected)?,
            serde_json::to_string_pretty(actual)?
        )));
    }
    Ok(())
}

pub fn run_executable_contract(
    executable: &Path,
    quick_fixture: &Path,
    profileset_fixture: &Path,
    quick_golden: &Path,
    profileset_golden: &Path,
    output_directory: &Path,
) -> Result<ContractReport> {
    fs::create_dir_all(output_directory)?;
    let identity = validate_supported_binary(executable)?;
    let quick_fixture = fs::canonicalize(quick_fixture)?;
    let profileset_fixture = fs::canonicalize(profileset_fixture)?;

    let (quick, quick_document) =
        run_report(executable, &quick_fixture, output_directory, "quick")?;
    verify_golden(&quick_projection(&quick_document)?, quick_golden, "quick")?;

    let (profileset, profileset_document) = run_report(
        executable,
        &profileset_fixture,
        output_directory,
        "profileset",
    )?;
    verify_golden(
        &profileset_projection(&profileset_document)?,
        profileset_golden,
        "profileset",
    )?;

    let missing = output_directory.join("intentionally-missing.simc");
    let invalid = run_with_timeout(
        executable,
        [missing.as_os_str()],
        output_directory,
        Duration::from_secs(10),
    )?;
    let invalid_input_exit_code = invalid
        .status
        .code()
        .ok_or_else(|| Error::Contract("invalid input exited by signal".into()))?;
    if invalid_input_exit_code != 60 {
        return Err(Error::Contract(format!(
            "missing input exit code changed from 60 to {invalid_input_exit_code}"
        )));
    }
    write_bytes(
        &output_directory.join("invalid.stdout.log"),
        invalid.stdout.as_bytes(),
    )?;
    write_bytes(
        &output_directory.join("invalid.stderr.log"),
        invalid.stderr.as_bytes(),
    )?;

    let cancel_arguments = [
        quick_fixture.as_os_str().to_owned(),
        OsString::from("iterations=100000000"),
        OsString::from("max_time=300"),
        OsString::from("report_progress=0"),
        OsString::from("report_details=0"),
    ];
    let cancel = cancel_after(
        executable,
        cancel_arguments,
        output_directory,
        Duration::from_millis(100),
        Duration::from_secs(5),
    )?;

    let report = ContractReport {
        identity,
        quick,
        profileset,
        invalid_input_exit_code,
        cancel_exit_code: cancel.exit_code,
        cancel_elapsed_millis: cancel.elapsed_millis,
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report)?;
    report_bytes.push(b'\n');
    write_bytes(
        &output_directory.join("contract-report.json"),
        &report_bytes,
    )?;
    Ok(report)
}

pub fn run_benchmark(
    executable: &Path,
    profileset_fixture: &Path,
    output_path: &Path,
    iterations: usize,
    repetitions: usize,
) -> Result<BenchmarkReport> {
    if iterations == 0 || repetitions == 0 {
        return Err(Error::Contract(
            "benchmark iterations and repetitions must be positive".into(),
        ));
    }
    let identity = validate_supported_binary(executable)?;
    let profileset_fixture = fs::canonicalize(profileset_fixture)?;
    let logical_cpus = std::thread::available_parallelism()?.get();
    let candidates = [(1, 0), (2, 0), (2, 1), (4, 1), (4, 2)];
    let working_directory = tempfile::Builder::new()
        .prefix("simshredder-benchmark-")
        .tempdir_in("/private/tmp")?;
    let mut samples = Vec::new();

    for (threads, profileset_work_threads) in candidates {
        if threads > logical_cpus {
            continue;
        }
        for repetition in 1..=repetitions {
            let json_path = working_directory.path().join(format!(
                "t{threads}-w{profileset_work_threads}-r{repetition}.json"
            ));
            let arguments = [
                profileset_fixture.as_os_str().to_owned(),
                OsString::from(format!("iterations={iterations}")),
                OsString::from(format!("threads={threads}")),
                OsString::from(format!("profileset_work_threads={profileset_work_threads}")),
                OsString::from("report_progress=0"),
                OsString::from("report_details=0"),
                OsString::from(format!("json2={}", json_path.display())),
            ];
            let output = run_with_timeout(
                executable,
                arguments,
                working_directory.path(),
                Duration::from_secs(5 * 60),
            )?;
            if !output.status.success() {
                return Err(Error::Contract(format!(
                    "benchmark t{threads}/w{profileset_work_threads} exited with {}",
                    output.status
                )));
            }
            let document: Value = serde_json::from_slice(&fs::read(json_path)?)?;
            let count = document
                .pointer("/sim/profilesets/results")
                .and_then(Value::as_array)
                .map(Vec::len);
            if count != Some(2) {
                return Err(Error::Contract(
                    "benchmark profileset result count changed".into(),
                ));
            }
            samples.push(BenchmarkSample {
                threads,
                profileset_work_threads,
                repetition,
                elapsed_millis: output.elapsed.as_millis(),
            });
        }
    }

    let mut grouped: BTreeMap<String, Vec<u128>> = BTreeMap::new();
    for sample in &samples {
        grouped
            .entry(format!(
                "threads={}/profileset_work_threads={}",
                sample.threads, sample.profileset_work_threads
            ))
            .or_default()
            .push(sample.elapsed_millis);
    }
    let median_millis = grouped
        .into_iter()
        .map(|(key, mut values)| {
            values.sort_unstable();
            (key, values[values.len() / 2])
        })
        .collect();
    let report = BenchmarkReport {
        identity,
        logical_cpus,
        iterations,
        samples,
        median_millis,
    };
    let mut bytes = serde_json::to_vec_pretty(&report)?;
    bytes.push(b'\n');
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_bytes(output_path, &bytes)?;
    Ok(report)
}
