//! Process-level checks: machine output and setup failure are public contracts.

use std::process::{Command, Stdio};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_llm_cli"))
}

#[test]
fn json_config_errors_are_one_object_with_a_failure_status() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.toml");
    std::fs::write(&path, "not valid = [").unwrap();
    let output = cli()
        .args(["ask", "hello", "--json", "--config"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["error"].as_str().unwrap().contains("config"));
}

#[test]
fn noninteractive_missing_setup_fails_without_prompting_or_polluting_stdout() {
    let directory = tempfile::tempdir().unwrap();
    let output = cli()
        .args(["ask", "hello"])
        .current_dir(directory.path())
        .env("PATH", directory.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Ollama"));
}

#[test]
fn completions_do_not_require_a_config_or_model_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let output = cli()
        .args(["completions", "bash", "--config"])
        .arg(directory.path().join("absent.toml"))
        .env("PATH", directory.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("_llm_cli"));
}
