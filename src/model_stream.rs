//! Shared subprocess streaming with one deadline for output and process exit.

use std::{process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

/// Keep incomplete code points between pipe reads. A pipe read is not a UTF-8
/// character boundary, even when the process always writes valid text.
#[derive(Default)]
struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    fn push(&mut self, bytes: &[u8], eof: bool) -> String {
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    text.push_str(valid);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    text.push_str(std::str::from_utf8(&self.pending[..valid]).unwrap());
                    match error.error_len() {
                        Some(invalid) => {
                            text.push('\u{fffd}');
                            self.pending.drain(..valid + invalid);
                        }
                        None => {
                            self.pending.drain(..valid);
                            if eof {
                                text.push_str(&String::from_utf8_lossy(&self.pending));
                                self.pending.clear();
                            }
                            break;
                        }
                    }
                }
            }
        }
        text
    }
}

pub async fn run(
    mut command: Command,
    deadline: Duration,
    mut on_chunk: impl FnMut(&str) -> Result<()>,
) -> Result<String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("starting Ollama chat")?;
    let mut stdout = child.stdout.take().context("capturing Ollama response")?;
    let mut stderr = child
        .stderr
        .take()
        .context("capturing Ollama diagnostics")?;

    let result = timeout(deadline, async {
        let output = async {
            let mut decoder = Utf8Decoder::default();
            let mut response = String::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let count = stdout
                    .read(&mut buffer)
                    .await
                    .context("reading Ollama response")?;
                let chunk = decoder.push(&buffer[..count], count == 0);
                if !chunk.is_empty() {
                    on_chunk(&chunk)?;
                    response.push_str(&chunk);
                }
                if count == 0 {
                    return Ok::<_, anyhow::Error>(response);
                }
            }
        };
        let diagnostics = async {
            let mut captured = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let count = stderr
                    .read(&mut buffer)
                    .await
                    .context("reading Ollama diagnostics")?;
                if count == 0 {
                    return Ok::<_, anyhow::Error>(String::from_utf8_lossy(&captured).into_owned());
                }
                // Drain all stderr so a verbose child cannot block, but bound
                // the diagnostic text retained for an unsuccessful response.
                let keep = count.min(4096_usize.saturating_sub(captured.len()));
                captured.extend_from_slice(&buffer[..keep]);
            }
        };
        let (response, diagnostics) = tokio::try_join!(output, diagnostics)?;
        let status = child.wait().await.context("waiting for Ollama chat")?;
        if !status.success() {
            bail!("Ollama chat exited with {status}: {}", diagnostics.trim());
        }
        Ok(response)
    })
    .await;

    match result {
        Ok(Ok(response)) => Ok(response),
        failure => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            match failure {
                Ok(Err(error)) => Err(error),
                Err(_) => bail!("Ollama chat timed out after {} seconds", deadline.as_secs()),
                Ok(Ok(_)) => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_survives_every_possible_pipe_boundary() {
        let text = "Hello 🦀 你好 café!";
        for split in 0..=text.len() {
            let mut decoder = Utf8Decoder::default();
            let output = decoder.push(&text.as_bytes()[..split], false)
                + &decoder.push(&text.as_bytes()[split..], true);
            assert_eq!(output, text, "split at byte {split}");
        }
        let mut decoder = Utf8Decoder::default();
        let mut output = String::new();
        for byte in text.bytes() {
            output.push_str(&decoder.push(&[byte], false));
        }
        output.push_str(&decoder.push(&[], true));
        assert_eq!(output, text);
    }

    #[test]
    fn invalid_and_incomplete_utf8_are_replaced_without_losing_following_text() {
        let mut decoder = Utf8Decoder::default();
        assert_eq!(decoder.push(&[b'a', 0xff, b'b', 0xf0, 0x9f], true), "a�b�");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn deadline_covers_silent_output_and_wait_after_closed_pipes() {
        for script in ["exec sleep 5", "exec 1>&-; exec 2>&-; exec sleep 5"] {
            let mut command = Command::new("sh");
            command.arg("-c").arg(script);
            let start = std::time::Instant::now();
            let error = run(command, Duration::from_millis(50), |_| Ok(()))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("timed out"));
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_output_streams_and_failure_contains_stderr() {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("printf hello; printf problem >&2; exit 7");
        let mut streamed = String::new();
        let error = run(command, Duration::from_secs(2), |chunk| {
            streamed.push_str(chunk);
            Ok(())
        })
        .await
        .unwrap_err();
        assert_eq!(streamed, "hello");
        assert!(error.to_string().contains("problem"));
    }
}
