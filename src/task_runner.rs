//! Cancellable subprocesses with bounded, latest-only live output.
//!
//! Commands receive an explicit working directory and argument vector: this
//! module does not interpret shell syntax. On Unix every command gets its own
//! process group, which is killed on cancellation, timeout, completion, or task
//! abortion so descendants cannot keep pipes open. On other platforms Tokio's
//! child-only cleanup is used; descendant cleanup is not yet guaranteed there.
//! Cancellation and timeout retain the bounded output already read; unread pipe
//! bytes can be discarded during cleanup, so these snapshots are not transcripts.

use std::{path::PathBuf, process::Stdio, time::Duration};

use tokio::{
    io::AsyncReadExt,
    process::Command,
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, interval, sleep, timeout},
};

/// Retain the most recent output per pipe, not an unbounded command transcript.
pub const OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
const UPDATE_INTERVAL: Duration = Duration::from_millis(100);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(pub u64);

#[derive(Clone, Debug)]
pub struct TaskSpec {
    pub id: TaskId,
    pub cwd: PathBuf,
    pub program: String,
    pub args: Vec<String>,
    /// Applied to this child only, after the default plain-output environment.
    pub environment: Vec<(String, String)>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskOutcome {
    Succeeded,
    Failed { code: Option<i32> },
    Cancelled,
    TimedOut,
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// Total raw bytes read from both pipes, before UTF-8 decoding or retention.
    pub output_bytes: usize,
    pub outcome: Option<TaskOutcome>,
}

pub struct TaskHandle {
    pub updates: watch::Receiver<TaskSnapshot>,
    cancellation: watch::Sender<bool>,
    worker: Option<JoinHandle<()>>,
}

impl TaskHandle {
    pub fn cancel(&self) {
        self.cancellation.send_replace(true);
    }

    /// Cancel and wait for bounded cleanup before closing the application.
    /// Returns the final retained output for reporting and audit records. If a
    /// worker stops without publishing an outcome, return an explicit error.
    pub async fn shutdown(mut self) -> TaskSnapshot {
        self.cancel();
        let mut worker_error = None;
        if let Some(mut worker) = self.worker.take() {
            match timeout(CLEANUP_TIMEOUT + Duration::from_secs(1), &mut worker).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    worker_error = Some(format!(
                        "Command worker stopped before reporting its outcome: {error}"
                    ));
                }
                Err(_) => {
                    // The process-group guard and kill_on_drop remain active if
                    // cleanup itself stalls or this task is interrupted.
                    worker.abort();
                    let _ = worker.await;
                    worker_error =
                        Some("Command worker did not finish within the cleanup deadline".into());
                }
            }
        }
        let mut snapshot = self.updates.borrow().clone();
        if snapshot.outcome.is_none() {
            snapshot.outcome = Some(TaskOutcome::Error(worker_error.unwrap_or_else(|| {
                "Command worker closed without reporting its outcome".into()
            })));
        }
        snapshot
    }
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        // Dropping a JoinHandle detaches it; explicitly signal the worker so
        // dropping a UI task never silently leaves its command running.
        self.cancel();
    }
}

pub fn spawn(spec: TaskSpec) -> TaskHandle {
    let initial = TaskSnapshot {
        id: spec.id,
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        output_bytes: 0,
        outcome: None,
    };
    let (updates, receiver) = watch::channel(initial);
    let (cancellation, cancelled) = watch::channel(false);
    let worker = tokio::spawn(run(spec, updates, cancelled));
    TaskHandle {
        updates: receiver,
        cancellation,
        worker: Some(worker),
    }
}

/// This guard is deliberately independent of asynchronous cleanup: aborting
/// the worker or shutting down the runtime must still kill its Unix group.
struct ProcessGroup {
    #[cfg(unix)]
    id: Option<i32>,
}

impl ProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        #[cfg(unix)]
        {
            Self {
                id: pid
                    .and_then(|pid| i32::try_from(pid).ok())
                    .filter(|pid| *pid > 0),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            Self {}
        }
    }

    fn kill(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        if let Some(id) = self.id {
            // SAFETY: kill accepts an integer process-group id and accesses
            // no Rust memory. The positive id came from our own child, whose
            // process_group(0) created a new group before executing user code.
            let result = unsafe { libc::kill(-id, libc::SIGKILL) };
            if result != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    // Keep the guard armed so Drop can retry cleanup.
                    return Err(error);
                }
            }
            self.id = None;
        }
        Ok(())
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

async fn run(
    spec: TaskSpec,
    updates: watch::Sender<TaskSnapshot>,
    mut cancelled: watch::Receiver<bool>,
) {
    let mut stdout_capture = TailCapture::default();
    let mut stderr_capture = TailCapture::default();
    if *cancelled.borrow() {
        publish(
            &updates,
            spec.id,
            &stdout_capture,
            &stderr_capture,
            Some(TaskOutcome::Cancelled),
        );
        return;
    }

    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("TERM", "dumb")
        .envs(spec.environment.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            publish(
                &updates,
                spec.id,
                &stdout_capture,
                &stderr_capture,
                Some(TaskOutcome::Error(format!(
                    "Could not start {} in {}: {error}",
                    spec.program,
                    spec.cwd.display()
                ))),
            );
            return;
        }
    };
    let mut group = ProcessGroup::new(child.id());
    // Both handles must exist because the command builder explicitly pipes them.
    let mut stdout = child.stdout.take().expect("configured stdout pipe");
    let mut stderr = child.stderr.take().expect("configured stderr pipe");
    let mut stdout_buffer = [0_u8; 8192];
    let mut stderr_buffer = [0_u8; 8192];
    let mut stdout_closed = false;
    let mut stderr_closed = false;
    let mut exit_status = None;
    let mut dirty = false;
    let deadline = sleep(spec.timeout);
    tokio::pin!(deadline);
    let mut ticker = interval(UPDATE_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut outcome = loop {
        if let Some(status) = exit_status {
            if stdout_closed && stderr_closed {
                break if std::process::ExitStatus::success(&status) {
                    TaskOutcome::Succeeded
                } else {
                    TaskOutcome::Failed {
                        code: status.code(),
                    }
                };
            }
        }
        tokio::select! {
            _ = cancelled.changed() => {
                break TaskOutcome::Cancelled;
            }
            _ = &mut deadline => {
                break TaskOutcome::TimedOut;
            }
            result = stdout.read(&mut stdout_buffer), if !stdout_closed => {
                match result {
                    Ok(count) => {
                        stdout_closed = count == 0;
                        stdout_capture.push(&stdout_buffer[..count], stdout_closed);
                        dirty = true;
                    }
                    Err(error) => break TaskOutcome::Error(format!("Reading command output: {error}")),
                }
            }
            result = stderr.read(&mut stderr_buffer), if !stderr_closed => {
                match result {
                    Ok(count) => {
                        stderr_closed = count == 0;
                        stderr_capture.push(&stderr_buffer[..count], stderr_closed);
                        dirty = true;
                    }
                    Err(error) => break TaskOutcome::Error(format!("Reading command diagnostics: {error}")),
                }
            }
            result = child.wait(), if exit_status.is_none() => {
                match result {
                    Ok(status) => {
                        exit_status = Some(status);
                        // A command may exit while a background descendant
                        // retains its pipes. Stop the group now, then drain
                        // buffered output instead of waiting until the deadline.
                        if let Err(error) = group.kill() {
                            break TaskOutcome::Error(format!("Command exited but stopping its descendants failed: {error}"));
                        }
                    }
                    Err(error) => break TaskOutcome::Error(format!("Waiting for command: {error}")),
                }
            }
            _ = ticker.tick() => {
                if dirty {
                    publish(&updates, spec.id, &stdout_capture, &stderr_capture, None);
                    dirty = false;
                }
            }
        }
    };

    if let Err(error) = group.kill() {
        outcome = TaskOutcome::Error(format!(
            "{outcome:?}; process-group termination could not be confirmed: {error}"
        ));
    }
    if exit_status.is_none() {
        let _ = child.start_kill();
        match timeout(CLEANUP_TIMEOUT, child.wait()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                outcome = TaskOutcome::Error(format!(
                    "{outcome:?}; command termination could not be confirmed: {error}"
                ));
            }
            Err(_) => {
                outcome = TaskOutcome::Error(format!(
                    "{outcome:?}; command termination could not be confirmed within {} seconds",
                    CLEANUP_TIMEOUT.as_secs()
                ));
            }
        }
    }
    // Flush a code point interrupted by cancellation as a replacement character.
    stdout_capture.push(&[], true);
    stderr_capture.push(&[], true);
    publish(
        &updates,
        spec.id,
        &stdout_capture,
        &stderr_capture,
        Some(outcome),
    );
}

fn publish(
    updates: &watch::Sender<TaskSnapshot>,
    id: TaskId,
    stdout: &TailCapture,
    stderr: &TailCapture,
    outcome: Option<TaskOutcome>,
) {
    // send_replace retains the final value even if all observers go away.
    updates.send_replace(TaskSnapshot {
        id,
        stdout: stdout.text.clone(),
        stderr: stderr.text.clone(),
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
        output_bytes: stdout.bytes.saturating_add(stderr.bytes),
        outcome,
    });
}

#[derive(Default)]
struct TailCapture {
    text: String,
    pending: Vec<u8>,
    truncated: bool,
    bytes: usize,
}

impl TailCapture {
    fn push(&mut self, bytes: &[u8], eof: bool) {
        self.bytes = self.bytes.saturating_add(bytes.len());
        self.pending.extend_from_slice(bytes);
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    self.text.push_str(valid);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    self.text
                        .push_str(std::str::from_utf8(&self.pending[..valid]).unwrap());
                    match error.error_len() {
                        Some(invalid) => {
                            self.text.push('\u{fffd}');
                            self.pending.drain(..valid + invalid);
                        }
                        None => {
                            self.pending.drain(..valid);
                            if eof {
                                self.text.push_str(&String::from_utf8_lossy(&self.pending));
                                self.pending.clear();
                            }
                            break;
                        }
                    }
                }
            }
        }
        if self.text.len() > OUTPUT_LIMIT_BYTES {
            let mut discard = self.text.len() - OUTPUT_LIMIT_BYTES;
            while !self.text.is_char_boundary(discard) {
                discard += 1;
            }
            self.text.drain(..discard);
            self.truncated = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_survives_every_split_and_invalid_bytes_are_replaced() {
        let text = "Hello 🦀 你好 café!";
        for split in 0..=text.len() {
            let mut capture = TailCapture::default();
            capture.push(&text.as_bytes()[..split], false);
            capture.push(&text.as_bytes()[split..], true);
            assert_eq!(capture.text, text, "split at {split}");
        }
        let mut capture = TailCapture::default();
        capture.push(&[b'a', 0xff, b'b', 0xf0, 0x9f], true);
        assert_eq!(capture.text, "a�b�");
        assert_eq!(capture.bytes, 5);
    }

    #[test]
    fn output_byte_count_saturates_without_overflow() {
        let mut capture = TailCapture {
            bytes: usize::MAX - 1,
            ..TailCapture::default()
        };
        capture.push(b"abc", true);
        assert_eq!(capture.bytes, usize::MAX);
    }

    #[test]
    fn bounded_tail_preserves_unicode_character_boundaries() {
        let mut capture = TailCapture::default();
        for _ in 0..20_000 {
            capture.push("🦀你好".as_bytes(), false);
        }
        capture.push(b"THE END", true);
        assert!(capture.truncated);
        assert!(capture.text.len() <= OUTPUT_LIMIT_BYTES);
        assert!(capture.text.ends_with("THE END"));
        assert!(!capture.text.contains('\u{fffd}'));
    }

    async fn finished(updates: &mut watch::Receiver<TaskSnapshot>) -> TaskSnapshot {
        timeout(Duration::from_secs(15), async {
            loop {
                let snapshot = updates.borrow_and_update().clone();
                if snapshot.outcome.is_some() {
                    return snapshot;
                }
                updates
                    .changed()
                    .await
                    .expect("worker must publish final state");
            }
        })
        .await
        .expect("runner should finish promptly")
    }

    #[tokio::test]
    async fn missing_program_is_a_final_error() {
        let mut handle = spawn(TaskSpec {
            id: TaskId(1),
            cwd: std::env::current_dir().unwrap(),
            program: "llm-cli-nonexistent-runner-test-program".into(),
            args: vec![],
            environment: vec![],
            timeout: Duration::from_secs(2),
        });
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.id, TaskId(1));
        assert!(matches!(result.outcome, Some(TaskOutcome::Error(_))));
        assert_eq!(handle.shutdown().await, result);
    }

    #[tokio::test]
    async fn missing_working_directory_is_a_final_error() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().join("does-not-exist");
        let mut handle = spawn(TaskSpec {
            id: TaskId(2),
            cwd: cwd.clone(),
            program: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            args: vec![],
            environment: vec![],
            timeout: Duration::from_secs(2),
        });
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.id, TaskId(2));
        match &result.outcome {
            Some(TaskOutcome::Error(message)) => {
                assert!(message.contains(&cwd.display().to_string()))
            }
            other => panic!("missing directory should fail gracefully, got {other:?}"),
        }
        assert_eq!(handle.shutdown().await, result);
    }

    #[tokio::test]
    async fn shutdown_reports_closed_worker_without_losing_snapshot() {
        let initial = TaskSnapshot {
            id: TaskId(42),
            stdout: "retained output".into(),
            stderr: "retained diagnostic".into(),
            stdout_truncated: true,
            stderr_truncated: false,
            output_bytes: OUTPUT_LIMIT_BYTES + 20,
            outcome: None,
        };
        let (updates, receiver) = watch::channel(initial.clone());
        let (cancellation, _cancelled) = watch::channel(false);
        drop(updates);
        let handle = TaskHandle {
            updates: receiver,
            cancellation,
            worker: Some(tokio::spawn(async {})),
        };
        let result = handle.shutdown().await;
        assert!(matches!(result.outcome, Some(TaskOutcome::Error(_))));
        assert_eq!(
            TaskSnapshot {
                outcome: None,
                ..result
            },
            initial
        );
    }

    #[cfg(unix)]
    fn shell(script: &str, limit: Duration) -> TaskHandle {
        spawn(TaskSpec {
            id: TaskId(7),
            cwd: std::env::current_dir().unwrap(),
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            environment: vec![],
            timeout: limit,
        })
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn output_is_visible_before_completion() {
        let directory = tempfile::tempdir().unwrap();
        let release = directory.path().join("release");
        let mut handle = spawn(TaskSpec {
            id: TaskId(9),
            cwd: directory.path().to_path_buf(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "printf 'started'; while [ ! -f \"$1\" ]; do sleep 0.02; done; printf ' finished'"
                    .into(),
                "test".into(),
                release.to_string_lossy().into_owned(),
            ],
            environment: vec![],
            timeout: Duration::from_secs(10),
        });
        timeout(Duration::from_secs(5), async {
            loop {
                handle.updates.changed().await.unwrap();
                let state = handle.updates.borrow_and_update().clone();
                if state.stdout.contains("started") {
                    assert_eq!(state.outcome, None);
                    break;
                }
            }
        })
        .await
        .unwrap();
        std::fs::write(release, b"release").unwrap();
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Succeeded));
        assert_eq!(result.stdout, "started finished");
        handle.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn environment_overrides_are_child_local() {
        const VARIABLE: &str = "LLM_CLI_RUNNER_CHILD_ENV_51D67583";
        let original = std::env::var_os(VARIABLE);
        let mut handle = spawn(TaskSpec {
            id: TaskId(10),
            cwd: std::env::current_dir().unwrap(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "printf '%s|%s|%s|%s' \"$LLM_CLI_RUNNER_CHILD_ENV_51D67583\" \"$TERM\" \"$NO_COLOR\" \"$CLICOLOR\"".into(),
            ],
            environment: vec![
                (VARIABLE.into(), "child-only".into()),
                ("TERM".into(), "explicit-terminal".into()),
                ("NO_COLOR".into(), "explicit-no-color".into()),
                ("CLICOLOR".into(), "explicit-cli-color".into()),
            ],
            timeout: Duration::from_secs(2),
        });
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Succeeded));
        assert_eq!(
            result.stdout,
            "child-only|explicit-terminal|explicit-no-color|explicit-cli-color"
        );
        assert_eq!(std::env::var_os(VARIABLE), original);
        assert_eq!(handle.shutdown().await, result);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failure_preserves_both_pipes_and_exit_code() {
        let mut handle = shell(
            "printf output; printf diagnostic >&2; exit 7",
            Duration::from_secs(2),
        );
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Failed { code: Some(7) }));
        assert_eq!(result.stdout, "output");
        assert_eq!(result.stderr, "diagnostic");
        handle.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn deadline_covers_silent_child_and_exit_after_closed_pipes() {
        for script in ["exec sleep 30", "exec 1>&-; exec 2>&-; exec sleep 30"] {
            let started = std::time::Instant::now();
            let mut handle = shell(script, Duration::from_millis(80));
            let result = finished(&mut handle.updates).await;
            assert_eq!(result.outcome, Some(TaskOutcome::TimedOut));
            assert!(started.elapsed() < Duration::from_secs(3));
            handle.shutdown().await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_preserves_output_already_read_from_both_pipes() {
        let mut handle = shell(
            "printf 'partial stdout'; printf 'partial stderr' >&2; exec sleep 30",
            Duration::from_secs(2),
        );
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::TimedOut));
        assert_eq!(result.stdout, "partial stdout");
        assert_eq!(result.stderr, "partial stderr");
        assert_eq!(handle.shutdown().await, result);
    }

    #[cfg(unix)]
    async fn assert_process_stopped(pid: &str) {
        timeout(Duration::from_secs(2), async {
            loop {
                // A killed orphan can remain a zombie briefly on Linux CI;
                // it cannot run or retain pipes and counts as stopped.
                let output = Command::new("ps")
                    .args(["-o", "stat=", "-p", pid.trim()])
                    .output()
                    .await
                    .unwrap();
                let status = String::from_utf8_lossy(&output.stdout);
                if status.trim().is_empty() || status.trim().starts_with('Z') {
                    return;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("descendant should no longer be running");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn parent_exit_cleans_descendants_that_keep_pipes_open() {
        let started = std::time::Instant::now();
        let mut handle = shell("sleep 30 & printf '%s' $!; exit 0", Duration::from_secs(5));
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Succeeded));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(!result.stdout.is_empty());
        assert_process_stopped(&result.stdout).await;
        handle.shutdown().await;
    }

    #[cfg(unix)]
    async fn wait_for_pid(handle: &mut TaskHandle) -> String {
        timeout(Duration::from_secs(2), async {
            loop {
                handle.updates.changed().await.unwrap();
                let state = handle.updates.borrow_and_update().clone();
                if !state.stdout.is_empty() {
                    assert!(state.stdout.trim().parse::<u32>().unwrap() > 0);
                    return state.stdout;
                }
            }
        })
        .await
        .unwrap()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_a_task_stops_its_descendant() {
        let mut handle = shell("sleep 30 & printf '%s' $!; wait", Duration::from_secs(10));
        let pid = wait_for_pid(&mut handle).await;
        handle.cancel();
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Cancelled));
        assert_process_stopped(&pid).await;
        handle.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_handle_cancels_instead_of_detaching_command() {
        let mut handle = shell("sleep 30 & printf '%s' $!; wait", Duration::from_secs(10));
        let pid = wait_for_pid(&mut handle).await;
        let mut observer = handle.updates.clone();
        drop(handle);
        let result = finished(&mut observer).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Cancelled));
        assert_process_stopped(&pid).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_returns_cancelled_state_and_retained_output() {
        let mut handle = shell("sleep 30 & printf '%s' $!; wait", Duration::from_secs(10));
        let pid = wait_for_pid(&mut handle).await;
        let result = handle.shutdown().await;
        assert_eq!(result.id, TaskId(7));
        assert_eq!(result.outcome, Some(TaskOutcome::Cancelled));
        assert_eq!(result.stdout, pid);
        assert_process_stopped(&pid).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aborting_worker_still_cleans_its_process_group() {
        let mut handle = shell("sleep 30 & printf '%s' $!; wait", Duration::from_secs(10));
        let pid = wait_for_pid(&mut handle).await;
        handle.worker.as_ref().unwrap().abort();
        let result = handle.shutdown().await;
        assert_eq!(result.id, TaskId(7));
        assert_eq!(result.stdout, pid);
        assert!(matches!(result.outcome, Some(TaskOutcome::Error(_))));
        assert_process_stopped(&pid).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn large_mixed_output_is_drained_but_retention_is_bounded() {
        let mut handle = shell(
            "i=0; while [ \"$i\" -lt 12000 ]; do printf '0123456789abcdef\\n'; printf 'stderr0123456789\\n' >&2; i=$((i + 1)); done; printf 'final stdout 🦀'; printf 'final stderr 你好' >&2",
            Duration::from_secs(10),
        );
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Succeeded));
        assert!(result.stdout_truncated && result.stderr_truncated);
        assert!(result.stdout.len() <= OUTPUT_LIMIT_BYTES);
        assert!(result.stderr.len() <= OUTPUT_LIMIT_BYTES);
        assert!(result.output_bytes > result.stdout.len() + result.stderr.len());
        assert!(result.stdout.ends_with("final stdout 🦀"));
        assert!(result.stderr.ends_with("final stderr 你好"));
        handle.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn split_utf8_is_reassembled_across_live_reads() {
        let mut handle = shell(
            "printf '\\360\\237'; sleep 0.2; printf '\\246\\200'",
            Duration::from_secs(3),
        );
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Succeeded));
        assert_eq!(result.stdout, "🦀");
        handle.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn working_directory_and_arguments_are_not_reinterpreted() {
        let directory = tempfile::tempdir().unwrap();
        let argument = "literal; $(printf not-a-command)";
        let mut handle = spawn(TaskSpec {
            id: TaskId(8),
            cwd: directory.path().to_path_buf(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "printf '%s\\n' \"$PWD\" \"$1\"; read ignored || printf 'stdin closed'".into(),
                "test".into(),
                argument.into(),
            ],
            environment: vec![],
            timeout: Duration::from_secs(2),
        });
        let result = finished(&mut handle.updates).await;
        assert_eq!(result.outcome, Some(TaskOutcome::Succeeded));
        let lines = result.stdout.lines().collect::<Vec<_>>();
        assert_eq!(
            PathBuf::from(lines[0]).canonicalize().unwrap(),
            directory.path().canonicalize().unwrap()
        );
        assert_eq!(lines[1], argument);
        assert_eq!(lines[2], "stdin closed");
        handle.shutdown().await;
    }
}
