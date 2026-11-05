use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u128,
}

impl ProcessResult {
    pub fn success(&self) -> bool {
        !self.cancelled && !self.timed_out && self.exit_code == Some(0)
    }
    pub fn report(&self) -> String {
        format!(
            "exit={:?}, timed_out={}, cancelled={}, elapsed={}ms\n{}\n{}",
            self.exit_code,
            self.timed_out,
            self.cancelled,
            self.elapsed_ms,
            self.stdout,
            self.stderr
        )
    }
}

pub fn run(
    command: &str,
    args: &[String],
    cwd: &Path,
    timeout_ms: u64,
) -> Result<ProcessResult, String> {
    run_with_cancel(
        command,
        args,
        cwd,
        timeout_ms,
        Arc::new(AtomicBool::new(false)),
    )
}

pub fn run_with_cancel(
    command: &str,
    args: &[String],
    cwd: &Path,
    timeout_ms: u64,
    cancel: Arc<AtomicBool>,
) -> Result<ProcessResult, String> {
    if timeout_ms == 0 {
        return Err("timeout_ms 必须大于 0".into());
    }
    run_controlled(
        command,
        args,
        cwd,
        Some(Duration::from_millis(timeout_ms)),
        cancel,
    )
}

pub fn run_unbounded_with_cancel(
    command: &str,
    args: &[String],
    cwd: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<ProcessResult, String> {
    run_controlled(command, args, cwd, None, cancel)
}

fn run_controlled(
    command: &str,
    args: &[String],
    cwd: &Path,
    timeout: Option<Duration>,
    cancel: Arc<AtomicBool>,
) -> Result<ProcessResult, String> {
    if !cwd.is_dir() {
        return Err(format!("工作目录不存在：{}", cwd.display()));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let start = Instant::now();
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("启动 {command} 失败：{e}"))?;
        let pid = child.id();
        let mut stdout = child.stdout.take().ok_or("stdout unavailable")?;
        let mut stderr = child.stderr.take().ok_or("stderr unavailable")?;
        let out = tokio::spawn(async move {
            let mut buf = Vec::new();
            stdout.read_to_end(&mut buf).await.map(|_| buf)
        });
        let err = tokio::spawn(async move {
            let mut buf = Vec::new();
            stderr.read_to_end(&mut buf).await.map(|_| buf)
        });
        let wait = async {
            loop {
                if cancel.load(Ordering::Relaxed) { return None; }
                tokio::select! { status = child.wait() => return Some(status), _ = tokio::time::sleep(Duration::from_millis(50)) => {} }
            }
        };
        let waited = match timeout {
            Some(timeout) => tokio::time::timeout(timeout, wait).await,
            None => Ok(wait.await),
        };
        let cancelled = matches!(waited, Ok(None));
        let timed_out = waited.is_err();
        // A shell may leave descendants holding the output pipes open.
        #[cfg(unix)]
        if timed_out || cancelled {
            if let Some(pid) = pid { unsafe { libc::kill(-(pid as i32), libc::SIGKILL); } }
        }
        let status = match waited {
            Ok(Some(status)) => status.map_err(|e| e.to_string())?,
            Ok(None) | Err(_) => {
                let _ = child.start_kill();
                child.wait().await.map_err(|e| e.to_string())?
            }
        };
        let stdout = out
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let stderr = err
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(ProcessResult {
            exit_code: status.code(),
            timed_out: timed_out && !cancelled,
            cancelled,
            stdout: String::from_utf8_lossy(&stdout).into(),
            stderr: String::from_utf8_lossy(&stderr).into(),
            elapsed_ms: start.elapsed().as_millis(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captures_failure_and_both_streams() {
        let result = run(
            "sh",
            &["-c".into(), "printf out; printf err >&2; exit 7".into()],
            Path::new("."),
            2000,
        )
        .unwrap();
        assert!(!result.success());
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.stdout, "out");
        assert_eq!(result.stderr, "err");
    }
    #[test]
    fn timeout_terminates_shell_descendants() {
        let result = run("sh", &["-c".into(), "sleep 10".into()], Path::new("."), 50).unwrap();
        assert!(result.timed_out);
        assert!(result.elapsed_ms < 2000);
    }
    #[test]
    fn rejects_missing_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(run("true", &[], &dir.path().join("missing"), 100).is_err());
    }
    #[test]
    fn cancellation_kills_running_process() {
        let token = Arc::new(AtomicBool::new(false));
        let worker_token = Arc::clone(&token);
        let worker = std::thread::spawn(move || {
            run_with_cancel(
                "sh",
                &["-c".into(), "sleep 10".into()],
                Path::new("."),
                10_000,
                worker_token,
            )
            .unwrap()
        });
        std::thread::sleep(Duration::from_millis(50));
        token.store(true, Ordering::Relaxed);
        let result = worker.join().unwrap();
        assert!(result.cancelled);
        assert!(result.elapsed_ms < 2000);
    }
}
