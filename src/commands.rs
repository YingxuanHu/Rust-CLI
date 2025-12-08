use std::{error::Error, io};

use anyhow::{Context, Result, anyhow, bail};

pub fn run_command(cwd: &std::path::Path, program: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                anyhow!("command '{program}' not found in PATH")
            } else {
                e.into()
            }
        })
        .with_context(|| format!("running {program} {:?}", args))?;

    if !output.status.success() {
        // Allow ripgrep exit code 1 (no matches) as a soft success.
        if program == "rg" && output.status.code() == Some(1) {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            return Ok(stdout.trim().to_string());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        bail!(
            "{program} {:?} exited with {}.\nstdout:\n{}\nstderr:\n{}",
            args,
            output.status,
            stdout,
            stderr
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout.trim().to_string())
}

pub fn run_shell_command(cwd: &std::path::Path, cmd: &str) -> Result<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output()
        .context("spawning shell")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        if !stderr.trim().is_empty() {
            bail!("{}", stderr.trim());
        } else {
            bail!("command exited with {}", output.status);
        }
    }

    // Combine stdout and stderr for complete output
    let mut result = stdout.to_string();
    if !stderr.trim().is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&stderr);
    }
    Ok(result.trim().to_string())
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

