//! Runs one command and reports what the operating system counted for it.

use std::{
    io::Read,
    mem::MaybeUninit,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};

/// `ru_maxrss` is in bytes on macOS and in kilobytes on Linux.
const MAXRSS_UNIT: u64 = if cfg!(target_os = "macos") { 1 } else { 1024 };

#[derive(Debug)]
pub struct Usage {
    pub wall_seconds: f64,
    pub user_seconds: f64,
    pub system_seconds: f64,
    /// The largest resident set of the command or of any child it waited for.
    pub peak_rss_bytes: u64,
    pub stdout: String,
    pub stderr: String,
    pub outcome: Exit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    Success,
    Failed(String),
    TimedOut,
}

/// Runs `command` in its own process group and kills the whole group at `timeout`.
pub fn run(command: &mut Command, timeout: Duration) -> Result<Usage> {
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .with_context(|| format!("starting {}", command.get_program().to_string_lossy()))?;
    let pid = child.id() as libc::pid_t;
    let stdout = drain(child.stdout.take().context("stdout is piped")?);
    let stderr = drain(child.stderr.take().context("stderr is piped")?);

    let (finished, watched) = mpsc::channel::<()>();
    let watchdog = thread::spawn(move || {
        let expired = watched.recv_timeout(timeout).is_err();
        if expired {
            // SAFETY: killpg takes plain integers. The group is the child's own.
            unsafe { libc::killpg(pid, libc::SIGKILL) };
        }
        expired
    });

    let (status, usage) = wait(pid)?;
    let wall_seconds = started.elapsed().as_secs_f64();
    finished.send(()).ok();
    let timed_out = watchdog.join().expect("the watchdog does not panic");

    Ok(Usage {
        wall_seconds,
        user_seconds: seconds(usage.ru_utime),
        system_seconds: seconds(usage.ru_stime),
        peak_rss_bytes: usage.ru_maxrss as u64 * MAXRSS_UNIT,
        stdout: stdout.join().expect("the reader does not panic"),
        stderr: stderr.join().expect("the reader does not panic"),
        outcome: match (timed_out, describe(status)) {
            (true, _) => Exit::TimedOut,
            (false, None) => Exit::Success,
            (false, Some(failure)) => Exit::Failed(failure),
        },
    })
}

fn drain(mut stream: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).ok();
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn wait(pid: libc::pid_t) -> Result<(libc::c_int, libc::rusage)> {
    let mut status = 0;
    let mut usage = MaybeUninit::<libc::rusage>::zeroed();
    loop {
        // SAFETY: both pointers are valid for the call, and wait4 fills `usage` on success.
        let reaped = unsafe { libc::wait4(pid, &mut status, 0, usage.as_mut_ptr()) };
        if reaped == pid {
            // SAFETY: wait4 returned the child, so it wrote `usage`.
            return Ok((status, unsafe { usage.assume_init() }));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            bail!("waiting for process {pid}: {error}");
        }
    }
}

fn seconds(time: libc::timeval) -> f64 {
    time.tv_sec as f64 + time.tv_usec as f64 / 1e6
}

/// Why the status is a failure, or nothing when it is a clean exit.
fn describe(status: libc::c_int) -> Option<String> {
    if libc::WIFSIGNALED(status) {
        return Some(format!("killed by signal {}", libc::WTERMSIG(status)));
    }
    match libc::WEXITSTATUS(status) {
        0 => None,
        code => Some(format!("exit status {code}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    const LONG: Duration = Duration::from_secs(30);

    #[test]
    fn a_clean_exit_keeps_its_output_and_usage() {
        let usage = run(&mut shell("echo out; echo err >&2"), LONG).unwrap();
        assert_eq!(usage.outcome, Exit::Success);
        assert_eq!(usage.stdout, "out\n");
        assert_eq!(usage.stderr, "err\n");
        assert!(usage.peak_rss_bytes > 0);
    }

    #[test]
    fn a_failure_reports_its_status() {
        let usage = run(&mut shell("exit 3"), LONG).unwrap();
        assert_eq!(usage.outcome, Exit::Failed("exit status 3".to_string()));
    }

    #[test]
    fn a_crash_reports_its_signal() {
        let usage = run(&mut shell("kill -SEGV $$"), LONG).unwrap();
        assert_eq!(
            usage.outcome,
            Exit::Failed(format!("killed by signal {}", libc::SIGSEGV))
        );
    }

    #[test]
    fn the_timeout_kills_children_too() {
        let started = Instant::now();
        let usage = run(
            &mut shell("sleep 30 & sleep 30"),
            Duration::from_millis(100),
        )
        .unwrap();
        assert_eq!(usage.outcome, Exit::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
