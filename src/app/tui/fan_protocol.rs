//! Query graphics, colors and cell pixels before starting the terminal input thread.

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::Event;

use super::fan_images::FanImages;
use super::fan_replies::ReplyParser;

// Image query, foreground, background, cursor, grid pixels and grid character dimensions.
const ALL_REPLIES: u8 = 0b11_1111;
const SIZE_REPLIES: u8 = 0b11_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Capabilities {
  pub foreground: [u8; 3],
  pub background: [u8; 3],
  pub accent: Option<[u8; 3]>,
  pub cell_pixels: (u16, u16),
}

pub(super) struct Detection {
  pub capabilities: Option<Capabilities>,
  pub cell_pixels: Option<(u16, u16)>,
  pub early_events: Vec<Event>,
  pub pending: Option<PendingQueries>,
}

pub(super) fn detect(
  out: &mut impl Write,
  images: Option<&mut FanImages>,
) -> io::Result<Detection> {
  let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
  let term = std::env::var("TERM").unwrap_or_default();
  if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
    return Ok(Detection {
      capabilities: None,
      cell_pixels: None,
      early_events: Vec::new(),
      pending: None,
    });
  }
  if let Some(images) = images.filter(|_| candidate(&program, &term)) {
    let id = images.probe(out)?;
    return detect_queries(out, libc::STDIN_FILENO, Duration::from_millis(700), id, ALL_REPLIES);
  }
  detect_queries(out, libc::STDIN_FILENO, Duration::from_millis(100), 0, SIZE_REPLIES)
}

fn candidate(program: &str, term: &str) -> bool {
  match program {
    "WarpTerminal" | "WezTerm" | "kitty" => true,
    "" => term == "xterm-kitty",
    _ => false,
  }
}

#[cfg(test)]
fn detect_fd(
  out: &mut impl Write,
  fd: libc::c_int,
  wait: Duration,
  id: u32,
) -> io::Result<Detection> {
  detect_queries(out, fd, wait, id, ALL_REPLIES)
}

fn detect_queries(
  out: &mut impl Write,
  fd: libc::c_int,
  wait: Duration,
  id: u32,
  requested: u8,
) -> io::Result<Detection> {
  if requested & 1 != 0 {
    out.write_all(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b]12;?\x1b\\")?;
  }
  out.write_all(b"\x1b[14t\x1b[18t")?;
  out.flush()?;
  let mut pending = PendingQueries {
    fd,
    id,
    requested,
    parser: ReplyParser::default(),
    evidence: Evidence::default(),
  };
  let early_events = pending.read(wait)?;
  Ok(Detection {
    capabilities: pending.capabilities(),
    cell_pixels: pending.cell_pixels(),
    early_events,
    pending: (!pending.finished()).then_some(pending),
  })
}

pub(super) struct PendingQueries {
  fd: libc::c_int,
  id: u32,
  requested: u8,
  parser: ReplyParser,
  evidence: Evidence,
}

impl PendingQueries {
  pub(super) fn capabilities(&self) -> Option<Capabilities> {
    (self.requested & 1 != 0).then(|| self.evidence.capabilities()).flatten()
  }

  pub(super) fn cell_pixels(&self) -> Option<(u16, u16)> {
    self.evidence.cell_pixels()
  }

  pub(super) fn finished(&self) -> bool {
    self.evidence.seen & self.requested == self.requested && !self.parser.has_pending()
  }

  /// Keep the same byte parser until all requested replies and partial input have completed.
  pub(super) fn read(&mut self, timeout: Duration) -> io::Result<Vec<Event>> {
    let mut events = Vec::new();
    let deadline = Instant::now() + timeout;
    while !self.finished() {
      let remaining = deadline.saturating_duration_since(Instant::now());
      let remaining = self.parser.idle_wait().map_or(remaining, |idle| remaining.min(idle));
      let mut poll = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
      let millis = remaining.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as i32;
      let ready = unsafe { libc::poll(&mut poll, 1, millis) };
      if ready < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
          return Err(error);
        }
      } else if ready == 0 {
        if self.parser.idle_wait().is_some_and(|idle| idle.is_zero()) {
          self.parser.flush_idle_esc(&mut events);
        }
      } else {
        if poll.revents & libc::POLLIN == 0 {
          return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let mut bytes = [0; 4096];
        let count = unsafe { libc::read(self.fd, bytes.as_mut_ptr().cast(), bytes.len()) };
        if count == 0 {
          return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if count < 0 {
          let error = io::Error::last_os_error();
          if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
          }
        } else {
          for reply in self.parser.feed(&bytes[..count as usize], &mut events)? {
            self.evidence.reply(&reply, self.id);
          }
        }
      }
      if Instant::now() >= deadline || !events.is_empty() {
        break;
      }
    }
    Ok(events)
  }
}

#[derive(Default)]
struct Evidence {
  seen: u8,
  ack: bool,
  foreground: Option<[u8; 3]>,
  background: Option<[u8; 3]>,
  ansi6: Option<[u8; 3]>,
  cursor: Option<[u8; 3]>,
  pixels: Option<(u32, u32)>,
  cells: Option<(u32, u32)>,
}

impl Evidence {
  fn reply(&mut self, bytes: &[u8], id: u32) {
    let Ok(reply) = std::str::from_utf8(bytes) else { return };
    if let Some(apc) = reply.strip_prefix("\x1b_G").and_then(|s| s.strip_suffix("\x1b\\")) {
      if let Some((header, status)) = apc.split_once(';')
        && header.split(',').any(|part| part == format!("i={id}"))
      {
        self.seen |= 1;
        self.ack |= status == "OK";
      }
    } else if let Some(osc) = reply
      .strip_prefix("\x1b]")
      .and_then(|s| s.strip_suffix("\x1b\\").or_else(|| s.strip_suffix('\x07')))
    {
      if let Some((code, value)) = osc.split_once(';') {
        self.seen |= match code {
          "10" => 2,
          "11" => 4,
          "12" => 8,
          _ => 0,
        };
        match code {
          "10" => self.foreground = rgb(value),
          "11" => self.background = rgb(value),
          "12" => self.cursor = rgb(value),
          "4" if value.starts_with("6;") => self.ansi6 = rgb(&value[2..]),
          _ => {}
        }
      }
    } else if let Some(csi) = reply.strip_prefix("\x1b[").and_then(|s| s.strip_suffix('t')) {
      self.seen |= match csi.split(';').next() {
        Some("4") => 16,
        Some("8") => 32,
        _ => 0,
      };
      let Ok(parts) = csi.split(';').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>() else {
        return;
      };
      match parts.as_slice() {
        [4, height, width] if *width > 0 && *height > 0 => self.pixels = Some((*width, *height)),
        [8, rows, cols] if *cols > 0 && *rows > 0 => self.cells = Some((*cols, *rows)),
        _ => {}
      }
    }
  }

  fn cell_pixels(&self) -> Option<(u16, u16)> {
    let (width, height) = self.pixels?;
    let (cols, rows) = self.cells?;
    let cell_pixels = (
      u16::try_from(width.checked_div(cols)?).ok()?,
      u16::try_from(height.checked_div(rows)?).ok()?,
    );
    (cell_pixels.0 > 0 && cell_pixels.1 > 0).then_some(cell_pixels)
  }

  fn capabilities(&self) -> Option<Capabilities> {
    if !self.ack {
      return None;
    }
    Some(Capabilities {
      foreground: self.foreground?,
      background: self.background?,
      accent: self.cursor.or(self.ansi6),
      cell_pixels: self.cell_pixels()?,
    })
  }
}

fn rgb(value: &str) -> Option<[u8; 3]> {
  let mut parts = value.strip_prefix("rgb:")?.split('/');
  let mut result = [0; 3];
  for component in &mut result {
    let hex = parts.next()?;
    if !(1..=4).contains(&hex.len()) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
      return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    let maximum = (1 << (hex.len() * 4)) - 1;
    *component = ((value * 255 + maximum / 2) / maximum) as u8;
  }
  parts.next().is_none().then_some(result)
}

#[cfg(test)]
mod tests {
  use std::fs::File;
  use std::os::fd::{AsRawFd, FromRawFd};
  use std::thread;

  use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
  };

  use super::*;

  const PROBE_ID: u32 = 0x4d12_3456;

  fn key(c: char) -> Event {
    Event::Key(KeyCode::Char(c).into())
  }

  fn replies(status: &str) -> Vec<u8> {
    format!("\x1b_Gi={PROBE_ID};{status}\x1b\\\x1b]10;rgb:ffff/ffff/ffff\x07\x1b]11;rgb:0000/0000/0000\x07\x1b]12;rgb:ffff/8888/0000\x07\x1b[4;600;800t\x1b[8;30;80t").into_bytes()
  }

  fn evidence() -> Evidence {
    let mut evidence = Evidence::default();
    for reply in [
      format!("\x1b_Gi={PROBE_ID};OK\x1b\\"),
      "\x1b]10;rgb:aaaa/bbbb/cccc\x1b\\".into(),
      "\x1b]11;rgb:1111/2222/3333\x07".into(),
      "\x1b[4;600;800t".into(),
      "\x1b[8;30;80t".into(),
    ] {
      evidence.reply(reply.as_bytes(), PROBE_ID);
    }
    evidence
  }

  fn pipe() -> (File, File) {
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
  }

  #[test]
  fn candidate_names_only_enable_attempts() {
    for name in ["WarpTerminal", "WezTerm", "kitty"] {
      assert!(candidate(name, "xterm-256color"));
    }
    assert!(candidate("", "xterm-kitty"));
    for name in ["Apple_Terminal", "vscode", "unknown"] {
      assert!(!candidate(name, "xterm-kitty"));
    }
    assert!(!candidate("", "xterm-256color"));
  }

  #[test]
  fn text_geometry_queries_need_neither_image_resources_nor_color_replies() {
    let (input, mut output) = pipe();
    output.write_all(b"\x1b[4;800;900t\x1b[8;40;100t").unwrap();
    let mut commands = Vec::new();
    let detection =
      detect_queries(&mut commands, input.as_raw_fd(), Duration::ZERO, 0, SIZE_REPLIES).unwrap();
    assert_eq!(commands, b"\x1b[14t\x1b[18t");
    assert_eq!(detection.cell_pixels, Some((9, 20)));
    assert!(detection.capabilities.is_none() && detection.pending.is_none());
  }

  #[test]
  fn extra_replies_finish_geometry_without_enabling_unrequested_graphics() {
    let (input, mut output) = pipe();
    output.write_all(&replies("OK")).unwrap();
    let detection =
      detect_queries(&mut Vec::new(), input.as_raw_fd(), Duration::ZERO, PROBE_ID, SIZE_REPLIES)
        .unwrap();
    assert_eq!(detection.cell_pixels, Some((10, 20)));
    assert!(detection.capabilities.is_none() && detection.pending.is_none());
  }

  #[test]
  fn failed_graphics_ack_still_provides_geometry_for_the_text_backend() {
    let (input, mut output) = pipe();
    output.write_all(&replies("ENOTSUP")).unwrap();
    let detection =
      detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::ZERO, PROBE_ID).unwrap();
    assert_eq!(detection.cell_pixels, Some((10, 20)));
    assert!(detection.capabilities.is_none() && detection.pending.is_none());
    let mut data = evidence();
    data.pixels = Some((0, 600));
    assert!(data.cell_pixels().is_none());
  }

  #[test]
  fn only_matching_ack_and_queried_grid_dimensions_enable_images() {
    let mut data = evidence();
    let caps = data.capabilities().unwrap();
    assert_eq!(caps.cell_pixels, (10, 20));
    assert_eq!(caps.foreground, [170, 187, 204]);
    assert_eq!(caps.background, [17, 34, 51]);
    assert_eq!(caps.accent, None);
    data.reply(b"\x1b]12;rgb:f/8/0\x1b\\", PROBE_ID);
    assert_eq!(data.capabilities().unwrap().accent, Some([255, 136, 0]));
    data.reply(b"\x1b]4;6;rgb:00/ff/80\x07", PROBE_ID);
    assert_eq!(data.capabilities().unwrap().accent, Some([255, 136, 0]));
    data.ack = false;
    data.reply(b"\x1b_Gi=1;OK\x1b\\", PROBE_ID);
    data.reply(format!("\x1b_Gi={PROBE_ID};ERROR: OK\x1b\\").as_bytes(), PROBE_ID);
    assert!(data.capabilities().is_none());
    data.ack = true;
    data.foreground = None;
    assert!(data.capabilities().is_none());
    data.foreground = Some([0; 3]);
    data.pixels = None;
    assert!(data.capabilities().is_none());
    let mut wrong_id = Evidence::default();
    wrong_id.reply(b"\x1b_Gi=1;OK\x1b\\", PROBE_ID);
    assert_eq!(wrong_id.seen, 0);
  }

  #[test]
  fn malformed_values_complete_the_request_but_never_enable_images() {
    assert_eq!(rgb("rgb:8000/ffff/0000"), Some([128, 255, 0]));
    for value in [
      "rgb:/00/00",
      "rgb:12345/00/00",
      "rgb:gg/00/00",
      "rgb:ff/ff/ff/ff",
      "rgb:+f/00/00",
      "#ffffff",
    ] {
      assert_eq!(rgb(value), None);
    }
    let mut data = Evidence::default();
    for reply in [
      b"\x1b[4;0;800t".as_slice(),
      b"\x1b[8;30;0t",
      b"\x1b[4;bad;600;800t",
      b"\x1b]10;rgb:gg/00/00\x1b\\",
    ] {
      data.reply(reply, PROBE_ID);
    }
    assert!(data.pixels.is_none());
    assert!(data.cells.is_none());
    assert!(data.foreground.is_none());
    assert_eq!(data.seen, 16 | 32 | 2);
  }

  #[test]
  fn all_six_replies_finish_early_without_optional_queries() {
    let (input, mut terminal) = pipe();
    terminal.write_all(&replies("OK")).unwrap();
    let mut out = Vec::new();
    let started = Instant::now();
    let detected =
      detect_fd(&mut out, input.as_raw_fd(), Duration::from_millis(700), PROBE_ID).unwrap();
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(detected.capabilities.unwrap().cell_pixels, (10, 20));
    assert!(detected.early_events.is_empty());
    assert!(detected.pending.is_none());
    let sent = String::from_utf8(out).unwrap();
    assert!(!sent.contains("a=q"));
    assert!(!sent.contains("\x1b]4;"));
    assert!(!sent.contains("\x1b[16t"));
    assert!(sent.contains("\x1b[14t\x1b[18t"));
  }

  #[test]
  fn deadline_keeps_lone_escape_until_the_late_reply_finishes() {
    let (input, mut terminal) = pipe();
    terminal.write_all(b"\x1b").unwrap();
    let detected =
      detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::from_millis(10), PROBE_ID).unwrap();
    assert!(detected.early_events.is_empty());
    let mut pending = detected.pending.unwrap();
    assert!(pending.read(Duration::ZERO).unwrap().is_empty());
    assert!(!pending.finished());
    terminal.write_all(format!("_Gi={PROBE_ID};ERROR: a q\x1b\\").as_bytes()).unwrap();
    assert!(pending.read(Duration::from_millis(5)).unwrap().is_empty());
    assert!(!pending.finished());
    let complete = replies("ERROR: a q");
    let remaining = &complete[complete.windows(2).position(|bytes| bytes == b"\x1b]").unwrap()..];
    terminal.write_all(remaining).unwrap();
    assert!(pending.read(Duration::from_millis(20)).unwrap().is_empty());
    assert!(pending.finished());
    assert!(pending.capabilities().is_none());
  }

  #[test]
  fn complete_replies_with_partial_utf8_stay_raw_until_the_character_completes() {
    let (input, mut terminal) = pipe();
    let mut bytes = replies("OK");
    bytes.push(0xe4);
    terminal.write_all(&bytes).unwrap();
    let detected =
      detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::from_millis(10), PROBE_ID).unwrap();
    assert!(detected.capabilities.is_some());
    let mut pending = detected.pending.unwrap();
    assert!(pending.read(Duration::ZERO).unwrap().is_empty());
    assert!(!pending.finished());
    terminal.write_all(b"\xb8\xadqa").unwrap();
    assert_eq!(
      pending.read(Duration::from_millis(20)).unwrap(),
      vec![key('中'), key('q'), key('a')]
    );
    assert!(pending.finished());
  }

  #[test]
  fn raw_ctrl_c_survives_late_unclosed_csi() {
    let (input, mut terminal) = pipe();
    terminal.write_all(b"\x1b[4;100;").unwrap();
    let detected =
      detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::from_millis(10), PROBE_ID).unwrap();
    let mut pending = detected.pending.unwrap();
    terminal.write_all(b"\x03").unwrap();
    let started = Instant::now();
    let events = pending.read(Duration::from_millis(700)).unwrap();
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(events, vec![Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))]);
    assert!(!pending.finished());
  }

  #[test]
  fn manual_pause_and_focus_events_return_without_waiting_for_missing_queries() {
    let (input, mut terminal) = pipe();
    let detected = detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::ZERO, PROBE_ID).unwrap();
    let mut pending = detected.pending.unwrap();
    terminal.write_all(b"a\x1b[O").unwrap();
    let started = Instant::now();
    assert_eq!(pending.read(Duration::from_millis(700)).unwrap(), vec![key('a'), Event::FocusLost]);
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(!pending.finished());
  }

  #[test]
  fn mouse_and_keyboard_input_work_while_queries_are_pending() {
    let (input, mut terminal) = pipe();
    terminal.write_all(b"aa\x1b[<0;4;5M\x1b[<64;4;5M").unwrap();
    let detected =
      detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::from_millis(10), PROBE_ID).unwrap();
    assert_eq!(
      detected.early_events,
      vec![
        key('a'),
        key('a'),
        Event::Mouse(MouseEvent {
          kind: MouseEventKind::Down(MouseButton::Left),
          column: 3,
          row: 4,
          modifiers: KeyModifiers::NONE
        }),
        Event::Mouse(MouseEvent {
          kind: MouseEventKind::ScrollUp,
          column: 3,
          row: 4,
          modifiers: KeyModifiers::NONE
        }),
      ]
    );
    let mut pending = detected.pending.unwrap();
    terminal.write_all(b"q").unwrap();
    assert_eq!(pending.read(Duration::from_millis(700)).unwrap(), vec![key('q')]);
  }

  #[test]
  fn completed_requests_release_idle_escape_without_waiting_for_the_full_probe() {
    let (input, mut terminal) = pipe();
    let mut bytes = replies("OK");
    bytes.push(27);
    terminal.write_all(&bytes).unwrap();
    let detected =
      detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::from_millis(700), PROBE_ID).unwrap();
    assert_eq!(detected.early_events, vec![Event::Key(KeyCode::Esc.into())]);
    assert!(detected.pending.is_none());
    assert!(detected.capabilities.is_some());
  }

  #[test]
  fn every_missing_query_still_returns_idle_escape_and_preserves_following_shortcuts() {
    let frames = ReplyParser::default().feed(&replies("OK"), &mut Vec::new()).unwrap();
    assert_eq!(frames.len(), 6);
    for missing in 0..frames.len() {
      let (input, mut terminal) = pipe();
      for (index, frame) in frames.iter().enumerate() {
        if index != missing {
          terminal.write_all(frame).unwrap();
        }
      }
      let detected =
        detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::ZERO, PROBE_ID).unwrap();
      let mut pending = detected.pending.unwrap();
      terminal.write_all(b"\x1b").unwrap();
      let started = Instant::now();
      assert_eq!(
        pending.read(Duration::from_millis(700)).unwrap(),
        vec![Event::Key(KeyCode::Esc.into())]
      );
      assert!(started.elapsed() < Duration::from_millis(100), "missing {missing}");
      assert!(!pending.finished());
      terminal.write_all("aq中".as_bytes()).unwrap();
      assert_eq!(
        pending.read(Duration::from_millis(700)).unwrap(),
        vec![key('a'), key('q'), key('中')]
      );
      assert!(!pending.finished());
      terminal.write_all(&frames[missing]).unwrap();
      assert!(pending.read(Duration::from_millis(20)).unwrap().is_empty());
      assert!(pending.finished());
    }
  }

  #[test]
  fn idle_literal_bracket_returns_before_the_query_deadline_and_retains_a_late_prefix() {
    let (input, mut terminal) = pipe();
    terminal.write_all(b"\x1b").unwrap();
    let detected =
      detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::from_millis(700), PROBE_ID).unwrap();
    assert_eq!(detected.early_events, vec![Event::Key(KeyCode::Esc.into())]);
    let mut pending = detected.pending.unwrap();
    terminal.write_all(b"[").unwrap();
    let started = Instant::now();
    assert_eq!(pending.read(Duration::from_millis(700)).unwrap(), vec![key('[')]);
    assert!(started.elapsed() < Duration::from_millis(100));
    terminal.write_all(b"4;600;800taq").unwrap();
    assert_eq!(pending.read(Duration::from_millis(700)).unwrap(), vec![key('a'), key('q')]);
    assert!(!pending.finished());
  }

  #[test]
  fn fake_terminal_byte_chunks_complete_without_real_tty_or_global_session() {
    let (input, mut terminal) = pipe();
    let producer = thread::spawn(move || {
      for byte in replies("OK") {
        terminal.write_all(&[byte]).unwrap();
      }
    });
    let detected =
      detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::from_millis(100), PROBE_ID).unwrap();
    producer.join().unwrap();
    assert_eq!(detected.capabilities.unwrap().cell_pixels, (10, 20));
    assert!(detected.early_events.is_empty());
    assert!(detected.pending.is_none());
  }

  #[test]
  fn closed_input_fails_instead_of_spinning_with_unresolved_queries() {
    let (input, terminal) = pipe();
    drop(terminal);
    assert!(
      matches!(detect_fd(&mut Vec::new(), input.as_raw_fd(), Duration::from_millis(700), PROBE_ID), Err(error) if error.kind() == io::ErrorKind::UnexpectedEof)
    );
  }
}
