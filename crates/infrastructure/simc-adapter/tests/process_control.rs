#[cfg(target_os = "windows")]
use std::fs;
use std::{
    io::{self, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use simc_adapter::{Error, LogChunk, ProcessControl, ProcessStream, run_with_control};

#[test]
#[ignore = "subprocess helper for process control integration"]
fn controlled_child() {
    println!("first streamed line");
    io::stdout().flush().unwrap();
    eprintln!("diagnostic streamed line");
    io::stderr().flush().unwrap();
    thread::sleep(Duration::from_secs(30));
}

#[test]
#[ignore = "subprocess helper that creates a descendant process"]
#[cfg(target_os = "windows")]
fn controlled_tree_parent() {
    let marker = std::env::current_dir().unwrap().join("descendant-survived");
    let mut descendant = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "delayed_descendant_marker",
            "--nocapture",
        ])
        .env("SIMSHREDDER_DESCENDANT_MARKER", marker)
        .spawn()
        .unwrap();
    println!("descendant started");
    io::stdout().flush().unwrap();
    descendant.wait().unwrap();
    thread::sleep(Duration::from_secs(30));
}

#[test]
#[ignore = "subprocess helper that writes only if it survives cancellation"]
#[cfg(target_os = "windows")]
fn delayed_descendant_marker() {
    thread::sleep(Duration::from_millis(1_500));
    fs::write(
        std::env::var_os("SIMSHREDDER_DESCENDANT_MARKER")
            .expect("descendant marker path must be set"),
        b"survived",
    )
    .unwrap();
}

#[test]
fn cancellation_is_observed_without_a_shell_and_streams_bounded_chunks() {
    let cancel = Arc::new(AtomicBool::new(false));
    let chunks = Arc::new(Mutex::new(Vec::<LogChunk>::new()));
    let chunks_ref = chunks.clone();
    let control = ProcessControl::new(
        Some(cancel.clone()),
        Some(Arc::new(move |chunk| {
            chunks_ref.lock().unwrap().push(chunk)
        })),
    );
    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancel.store(true, Ordering::Release);
    });
    let executable = std::env::current_exe().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let error = run_with_control(
        &executable,
        ["--ignored", "--exact", "controlled_child", "--nocapture"],
        temporary.path(),
        Duration::from_secs(5),
        control,
    )
    .unwrap_err();
    cancel_thread.join().unwrap();
    assert!(matches!(error, Error::ProcessCanceled { .. }));
    let chunks = chunks.lock().unwrap();
    assert!(chunks.iter().any(|chunk| {
        chunk.stream == ProcessStream::Stdout
            && String::from_utf8_lossy(&chunk.bytes).contains("first streamed line")
    }));
    assert!(chunks.iter().any(|chunk| {
        chunk.stream == ProcessStream::Stderr
            && String::from_utf8_lossy(&chunk.bytes).contains("diagnostic streamed line")
    }));
}

#[test]
#[cfg(target_os = "windows")]
fn cancellation_terminates_the_entire_windows_job_tree() {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_ref = cancel.clone();
    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancel_ref.store(true, Ordering::Release);
    });
    let executable = std::env::current_exe().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let marker = temporary.path().join("descendant-survived");
    let error = run_with_control(
        &executable,
        [
            "--ignored",
            "--exact",
            "controlled_tree_parent",
            "--nocapture",
        ],
        temporary.path(),
        Duration::from_secs(5),
        ProcessControl::new(Some(cancel), None),
    )
    .unwrap_err();
    cancel_thread.join().unwrap();
    assert!(matches!(error, Error::ProcessCanceled { .. }));
    thread::sleep(Duration::from_secs(2));
    assert!(
        !marker.exists(),
        "a descendant escaped the Windows Job Object"
    );
}
