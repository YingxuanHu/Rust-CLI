use std::{
    error::Error,
    io::{self, Read},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};

/// Run a non-interactive command with a wall-clock timeout while draining both
/// output streams. Draining in dedicated readers avoids a verbose child
/// process blocking on a full stdout or stderr pipe.
pub fn run_command_with_timeout(
    cwd: &std::path::Path,
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    let output = run_process_with_timeout(
        &mut command,
        &format!("{program} {args:?}"),
        timeout,
    )?;

    if !output.status.success() {
        // Ripgrep uses exit code 1 to indicate that it found no matches.
        if program == "rg" && output.status.code() == Some(1) {
            return Ok(output.stdout.trim().to_string());
        }
        bail!(
            "{program} {:?} exited with {}.\nstdout:\n{}\nstderr:\n{}",
            args,
            output.status,
            output.stdout,
            output.stderr
        );
    }

    Ok(output.stdout.trim().to_string())
}

/// Shell-mode counterpart to [`run_command_with_timeout`].
pub fn run_shell_command_with_timeout(
    cwd: &std::path::Path,
    cmd: &str,
    timeout: Duration,
) -> Result<String> {
    if is_potentially_interactive(cmd) {
        bail!(
            "Command appears to require interactive input: {}\n\
             Hint: For git commit, use 'git commit -m \"message\"' instead of bare 'git commit'",
            cmd
        );
    }

    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd).current_dir(cwd);
    let output = run_process_with_timeout(&mut command, &format!("sh -c {cmd}"), timeout)?;

    if !output.status.success() {
        if !output.stderr.trim().is_empty() {
            bail!("{}", output.stderr.trim());
        }
        bail!("command exited with {}", output.status);
    }

    let mut result = output.stdout;
    if !output.stderr.trim().is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&output.stderr);
    }
    Ok(result.trim().to_string())
}

struct TimedOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_process_with_timeout(
    command: &mut Command,
    description: &str,
    timeout: Duration,
) -> Result<TimedOutput> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            anyhow!("command not found in PATH")
        } else {
            error.into()
        }
    })
    .with_context(|| format!("spawning {description}"))?;

    let stdout = child.stdout.take().context("capturing command stdout")?;
    let stderr = child.stderr.take().context("capturing command stderr")?;
    let stdout_reader = thread::spawn(move || read_stream(stdout));
    let stderr_reader = thread::spawn(move || read_stream(stderr));

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().context("checking command status")? {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().context("waiting for timed-out command")?;
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_reader(stdout_reader, "stdout")?;
    let stderr = join_reader(stderr_reader, "stderr")?;
    if timed_out {
        bail!(
            "{description} timed out after {} seconds",
            timeout.as_secs_f64()
        );
    }

    Ok(TimedOutput {
        status,
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    })
}

fn read_stream(mut stream: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_reader(
    reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    stream_name: &str,
) -> Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| anyhow!("{stream_name} reader thread panicked"))?
        .context("reading command output")
}

/// Check if a command is likely to require interactive input
fn is_potentially_interactive(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    
    // Check for git commit without -m, -F, or --amend flags
    if cmd_lower.contains("git commit") {
        // If it has git commit but no message flag, it's likely interactive
        if !cmd_lower.contains(" -m ") 
            && !cmd_lower.contains(" -m\"")
            && !cmd_lower.contains(" -m'")
            && !cmd_lower.contains(" -f ")
            && !cmd_lower.contains("--message")
            && !cmd_lower.contains("--file")
            && !cmd_lower.contains("--amend")
            && !cmd_lower.contains("--no-edit")
        {
            return true;
        }
    }
    
    // Add other interactive commands as needed
    // e.g., vim, nano, less without input, interactive python, etc.
    
    false
}

pub fn format_error(err: &anyhow::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut current: Option<&(dyn Error + 'static)> = err.source();
    while let Some(src) = current {
        parts.push(src.to_string());
        current = src.source();
    }
    parts.join(": ")
}

pub fn split_commit_message(msg: &str) -> (String, Vec<String>) {
    let mut lines: Vec<String> = msg
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    if lines.is_empty() {
        return ("chore: save work".to_string(), vec![]);
    }
    let subject = lines.remove(0);
    (subject, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_shell_command_captures_stderr_without_failing() {
        let output = run_shell_command_with_timeout(
            std::path::Path::new("."),
            "printf out; printf err >&2",
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(output, "out\nerr");
    }

    #[test]
    fn timed_shell_command_is_terminated() {
        let error = run_shell_command_with_timeout(
            std::path::Path::new("."),
            "sleep 1",
            Duration::from_millis(20),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }
}
