//! Control the browser dashboard using the same session service as the desktop entry.

use std::io;
use std::path::Path;
use std::process::Command;

use clap::Subcommand;

const CONTROLLER: &str = include_str!("../../scripts/dashboard-control.sh");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Subcommand)]
pub enum Action {
  /// Start background monitoring without opening the browser
  Start,
  /// Stop background monitoring and release its port
  Stop,
  /// Show the background service state and dashboard address
  Status,
}

fn controller_command(action: Option<Action>, binary: &Path) -> Command {
  let action = match action {
    None => "open",
    Some(Action::Start) => "start",
    Some(Action::Stop) => "stop",
    Some(Action::Status) => "status",
  };
  let mut command = Command::new("/bin/sh");
  command.args(["-c", CONTROLLER, "macmon-web", action]).arg(binary);
  command
}

pub fn run(action: Option<Action>) -> io::Result<()> {
  // Older desktop entries retain this alias and compare their configured path exactly.
  let binary = std::env::current_exe()?;
  let status = controller_command(action, &binary).status()?;
  if !status.success() {
    return Err(io::Error::other(format!("dashboard control failed ({status})")));
  }
  Ok(())
}

fn managed_job_running(output: &std::process::Output) -> io::Result<bool> {
  if !output.status.success() {
    let error = String::from_utf8_lossy(&output.stderr);
    if output.status.code() == Some(113) && error.contains("Could not find service") {
      return Ok(false);
    }
    return Err(io::Error::other("cannot inspect dashboard job; use macmon web status"));
  }
  Ok(String::from_utf8_lossy(&output.stdout).lines().any(|line| {
    line
      .trim()
      .strip_prefix("pid = ")
      .and_then(|pid| pid.parse::<u32>().ok())
      .is_some_and(|pid| pid > 0)
  }))
}

/// An old managed backend owns its own sampler. Never silently start another owner beside it.
pub(crate) fn require_shareable_backend() -> io::Result<()> {
  use std::io::{Read, Write};
  use std::net::{SocketAddr, TcpStream};
  use std::time::Duration;
  let job = format!("gui/{}/com.macmon", unsafe { libc::geteuid() });
  let launchd = Command::new("launchctl").args(["print", &job]).output()?;
  // No running managed job: foreign ports or stopped plists do not prevent a standalone TUI.
  if !managed_job_running(&launchd)? {
    return Ok(());
  }
  let output = controller_command(Some(Action::Status), &std::env::current_exe()?).output()?;
  if !output.status.success() {
    return Err(io::Error::other(
      "cannot verify existing dashboard ownership; use macmon web status",
    ));
  }
  let status = String::from_utf8_lossy(&output.stdout);
  if status.starts_with("Stopped") {
    return Ok(());
  }
  let address: SocketAddr = status
    .split_whitespace()
    .last()
    .and_then(|url| url.strip_prefix("http://"))
    .and_then(|url| url.strip_suffix("/dashboard/"))
    .and_then(|address| address.parse().ok())
    .filter(|address: &SocketAddr| address.ip().to_canonical().is_loopback())
    .ok_or_else(|| io::Error::other("cannot identify the existing dashboard"))?;
  let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
  stream.set_read_timeout(Some(Duration::from_secs(1)))?;
  stream.set_write_timeout(Some(Duration::from_secs(1)))?;
  write!(stream, "GET /api/source HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")?;
  let mut reply = String::new();
  stream.take(8192).read_to_string(&mut reply)?;
  let compatible = reply.starts_with("HTTP/1.1 200 ")
    && reply
      .split_once("\r\n\r\n")
      .and_then(|(_, body)| serde_json::from_str::<serde_json::Value>(body).ok())
      .is_some_and(|body| body["protocol"] == 1);
  if compatible {
    Ok(())
  } else {
    Err(io::Error::other(
      "dashboard backend cannot share sampling; restart it with the current macmon web stop/start",
    ))
  }
}

#[cfg(test)]
mod tests {
  use std::ffi::OsStr;

  use super::*;

  #[test]
  fn actions_and_executable_paths_are_separate_shell_arguments() {
    let binary = Path::new("/Installed tools/macmon's & $(printf injected) executable");
    for (action, expected) in [
      (None, "open"),
      (Some(Action::Start), "start"),
      (Some(Action::Stop), "stop"),
      (Some(Action::Status), "status"),
    ] {
      let command = controller_command(action, binary);
      assert_eq!(command.get_program(), "/bin/sh");
      assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
          OsStr::new("-c"),
          OsStr::new(CONTROLLER),
          OsStr::new("macmon-web"),
          OsStr::new(expected),
          binary.as_os_str()
        ]
      );
    }
  }
  #[test]
  fn standalone_sampling_does_not_depend_on_stopped_plists_or_foreign_ports() {
    use std::os::unix::process::ExitStatusExt;
    let absent = std::process::Output {
      status: std::process::ExitStatus::from_raw(113 << 8),
      stdout: Vec::new(),
      stderr: b"Could not find service com.macmon".to_vec(),
    };
    assert!(!managed_job_running(&absent).unwrap());
    let stopped = std::process::Output {
      status: std::process::ExitStatus::from_raw(0),
      stdout: b"program = /another/build/macmon
state = not running
"
      .to_vec(),
      stderr: Vec::new(),
    };
    assert!(!managed_job_running(&stopped).unwrap());
    let running = std::process::Output {
      stdout: b"pid = 12345
"
      .to_vec(),
      ..stopped
    };
    assert!(managed_job_running(&running).unwrap());
    let failure = std::process::Output {
      status: std::process::ExitStatus::from_raw(5 << 8),
      stdout: Vec::new(),
      stderr: b"XPC connection interrupted".to_vec(),
    };
    assert!(managed_job_running(&failure).is_err());
  }
}
