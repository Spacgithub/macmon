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
  let binary = std::env::current_exe()?.canonicalize()?;
  let status = controller_command(action, &binary).status()?;
  if !status.success() {
    return Err(io::Error::other(format!("dashboard control failed ({status})")));
  }
  Ok(())
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
}
