use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use regex::Regex;

use crate::error::AppError;

const QUICK_TUNNEL_TIMEOUT: Duration = Duration::from_secs(20);

fn quick_tunnel_url_regex() -> Regex {
    Regex::new(r"https://[a-zA-Z0-9.-]+\.trycloudflare\.com").expect("valid regex")
}

pub fn extract_trycloudflare_url(line: &str) -> Option<String> {
    quick_tunnel_url_regex()
        .find(line)
        .map(|matched| matched.as_str().to_string())
}

fn reader_thread(
    stream: impl std::io::Read + Send + 'static,
    sender: mpsc::Sender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines().map_while(Result::ok) {
            let _ = sender.send(line);
        }
    })
}

#[derive(Debug)]
pub struct CloudflaredQuickTunnel {
    child: Child,
    pub public_url: String,
}

impl CloudflaredQuickTunnel {
    pub fn start(local_url: &str, binary: Option<&Path>) -> Result<Self, AppError> {
        let command = binary
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("cloudflared"));
        let mut child = Command::new(command)
            .args(["tunnel", "--url", local_url])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| AppError::Config(format!("failed to start cloudflared: {error}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Config("cloudflared stdout not captured".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Config("cloudflared stderr not captured".to_string()))?;

        let (sender, receiver) = mpsc::channel::<String>();
        let stdout_handle = reader_thread(stdout, sender.clone());
        let stderr_handle = reader_thread(stderr, sender);
        let deadline = std::time::Instant::now() + QUICK_TUNNEL_TIMEOUT;

        let public_url = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| AppError::Config(format!("failed to poll cloudflared: {error}")))?
            {
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(AppError::Config(format!(
                    "cloudflared exited before publishing a URL: {status}"
                )));
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                let _ = child.kill();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(AppError::Config(
                    "timed out waiting for cloudflared quick tunnel URL".to_string(),
                ));
            }

            match receiver.recv_timeout(remaining.min(Duration::from_millis(250))) {
                Ok(line) => {
                    if let Some(url) = extract_trycloudflare_url(&line) {
                        break url;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(AppError::Config(
                        "cloudflared output stream ended before URL appeared".to_string(),
                    ));
                }
            }
        };

        Ok(Self { child, public_url })
    }
}

impl Drop for CloudflaredQuickTunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::extract_trycloudflare_url;

    #[test]
    fn extracts_quick_tunnel_url_from_log_line() {
        let line =
            "INF Quick Tunnel has been created! Visit it at https://abc-123.trycloudflare.com";
        assert_eq!(
            extract_trycloudflare_url(line),
            Some("https://abc-123.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn ignores_lines_without_quick_tunnel_url() {
        assert_eq!(extract_trycloudflare_url("plain log line"), None);
    }
}
