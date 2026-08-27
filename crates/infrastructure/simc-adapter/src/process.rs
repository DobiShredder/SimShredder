use std::{
    ffi::{OsStr, OsString},
    io::Read,
    path::Path,
    process::ExitStatus,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use std::process::{Child as ManagedChild, Command, Stdio};
#[cfg(windows)]
use windows_spawn::{Child as ManagedChild, Command, DropPolicy, SpawnOptions, Stdio};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub elapsed: Duration,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

struct CapturedStream {
    bytes: Vec<u8>,
    truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogChunk {
    pub stream: ProcessStream,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct ProcessControl {
    cancel: Option<Arc<AtomicBool>>,
    log_observer: Option<Arc<dyn Fn(LogChunk) + Send + Sync>>,
}

impl ProcessControl {
    pub fn new(
        cancel: Option<Arc<AtomicBool>>,
        log_observer: Option<Arc<dyn Fn(LogChunk) + Send + Sync>>,
    ) -> Self {
        Self {
            cancel,
            log_observer,
        }
    }

    fn is_canceled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
    }

    fn emit(&self, chunk: LogChunk) {
        if let Some(observer) = &self.log_observer {
            observer(chunk);
        }
    }
}

fn read_bounded(
    mut reader: impl Read,
    limit: usize,
    stream: ProcessStream,
    sender: Option<mpsc::Sender<LogChunk>>,
) -> std::io::Result<CapturedStream> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        let retained = remaining.min(count);
        bytes.extend_from_slice(&buffer[..retained]);
        if retained > 0
            && let Some(sender) = &sender
        {
            let _ = sender.send(LogChunk {
                stream,
                bytes: buffer[..retained].to_vec(),
            });
        }
        truncated |= retained != count;
    }
    Ok(CapturedStream { bytes, truncated })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CancelOutput {
    pub exit_code: Option<i32>,
    pub elapsed_millis: u128,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimcIdentity {
    pub simc_version: String,
    pub game_version: String,
    pub channel: String,
    pub hotfix: Option<String>,
}

pub fn run_with_timeout<I, S>(
    executable: &Path,
    arguments: I,
    working_directory: &Path,
    timeout: Duration,
) -> Result<ProcessOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_with_control(
        executable,
        arguments,
        working_directory,
        timeout,
        ProcessControl::default(),
    )
}

pub fn run_with_control<I, S>(
    executable: &Path,
    arguments: I,
    working_directory: &Path,
    timeout: Duration,
    control: ProcessControl,
) -> Result<ProcessOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let started = Instant::now();
    let arguments: Vec<OsString> = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_os_string())
        .collect();
    let mut child = spawn_managed(executable, &arguments, working_directory, true)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Contract("failed to capture stdout".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Contract("failed to capture stderr".into()))?;
    let (sender, receiver) = mpsc::channel();
    let stdout_sender = sender.clone();
    let stdout_reader = thread::spawn(move || {
        read_bounded(
            &mut stdout,
            MAX_CAPTURE_BYTES,
            ProcessStream::Stdout,
            Some(stdout_sender),
        )
    });
    let stderr_reader = thread::spawn(move || {
        read_bounded(
            &mut stderr,
            MAX_CAPTURE_BYTES,
            ProcessStream::Stderr,
            Some(sender),
        )
    });

    loop {
        for chunk in receiver.try_iter() {
            control.emit(chunk);
        }
        if let Some(status) = child.try_wait()? {
            drop(child);
            let stdout = stdout_reader
                .join()
                .map_err(|_| Error::Contract("stdout reader panicked".into()))??;
            let stderr = stderr_reader
                .join()
                .map_err(|_| Error::Contract("stderr reader panicked".into()))??;
            for chunk in receiver.try_iter() {
                control.emit(chunk);
            }
            return Ok(ProcessOutput {
                status,
                stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
                stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
                elapsed: started.elapsed(),
                stdout_truncated: stdout.truncated,
                stderr_truncated: stderr.truncated,
            });
        }
        if control.is_canceled() {
            child.kill()?;
            let status = child.wait()?;
            drop(child);
            let stdout = stdout_reader
                .join()
                .map_err(|_| Error::Contract("stdout reader panicked".into()))??;
            let stderr = stderr_reader
                .join()
                .map_err(|_| Error::Contract("stderr reader panicked".into()))??;
            for chunk in receiver.try_iter() {
                control.emit(chunk);
            }
            return Err(Error::ProcessCanceled {
                duration: started.elapsed(),
                status: status.to_string(),
                exit_code: status.code(),
                stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
                stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
                stdout_truncated: stdout.truncated,
                stderr_truncated: stderr.truncated,
            });
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            let status = child.wait()?;
            drop(child);
            let stdout = stdout_reader
                .join()
                .map_err(|_| Error::Contract("stdout reader panicked".into()))??;
            let stderr = stderr_reader
                .join()
                .map_err(|_| Error::Contract("stderr reader panicked".into()))??;
            return Err(Error::ProcessTimedOut {
                duration: started.elapsed(),
                status: status.to_string(),
                exit_code: status.code(),
                stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
                stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
                stdout_truncated: stdout.truncated,
                stderr_truncated: stderr.truncated,
            });
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(not(windows))]
fn spawn_managed(
    executable: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    capture: bool,
) -> std::io::Result<ManagedChild> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(working_directory)
        .stdin(Stdio::null());
    if capture {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    command.spawn()
}

#[cfg(windows)]
fn spawn_managed(
    executable: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    capture: bool,
) -> std::io::Result<ManagedChild> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(working_directory)
        .stdin(Stdio::null());
    if capture {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    command.spawn_with(SpawnOptions::new().drop_policy(DropPolicy::KillTree))
}

pub fn parse_identity(output: &str) -> Result<SimcIdentity> {
    let pattern = Regex::new(
        r"SimulationCraft ([0-9]+-[0-9]+) for World of Warcraft ([0-9.]+) (Live|PTR|Beta)(?: \(hotfix ([^)]+)\))?",
    )
    .expect("constant regex must compile");
    let captures = pattern
        .captures(output)
        .ok_or_else(|| Error::Contract("version banner was not found".into()))?;
    Ok(SimcIdentity {
        simc_version: captures[1].to_owned(),
        game_version: captures[2].to_owned(),
        channel: captures[3].to_ascii_lowercase(),
        hotfix: captures.get(4).map(|value| value.as_str().to_owned()),
    })
}

pub fn cancel_after<I, S>(
    executable: &Path,
    arguments: I,
    working_directory: &Path,
    delay: Duration,
    deadline: Duration,
) -> Result<CancelOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let started = Instant::now();
    let arguments: Vec<OsString> = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_os_string())
        .collect();
    let mut child = spawn_managed(executable, &arguments, working_directory, false)?;
    thread::sleep(delay);
    if let Some(status) = child.try_wait()? {
        return Err(Error::Contract(format!(
            "cancel probe exited before cancellation with {status}"
        )));
    }
    child.kill()?;
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Err(Error::Contract(
                    "cancelled process reported successful completion".into(),
                ));
            }
            drop(child);
            return Ok(CancelOutput {
                exit_code: status.code(),
                elapsed_millis: started.elapsed().as_millis(),
            });
        }
        if started.elapsed() >= deadline {
            return Err(Error::Timeout(deadline));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_identity_with_hotfix() {
        assert_eq!(
            parse_identity(
                "Nothing to sim! SimulationCraft 1210-01 for World of Warcraft 12.1.0.69404 Live (hotfix 2026-08-22/69404)"
            )
            .unwrap(),
            SimcIdentity {
                simc_version: "1210-01".into(),
                game_version: "12.1.0.69404".into(),
                channel: "live".into(),
                hotfix: Some("2026-08-22/69404".into()),
            }
        );
    }

    #[test]
    fn refuses_unstructured_output() {
        assert!(parse_identity("SimulationCraft unknown").is_err());
    }

    #[test]
    fn drains_but_bounds_captured_output() {
        let captured = read_bounded(
            std::io::Cursor::new(vec![b'x'; 32]),
            8,
            ProcessStream::Stdout,
            None,
        )
        .unwrap();
        assert_eq!(captured.bytes, vec![b'x'; 8]);
        assert!(captured.truncated);
    }
}
