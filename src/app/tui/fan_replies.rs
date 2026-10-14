//! Framed query replies must never become TUI shortcut keys, including after the probe deadline.

use std::io;

use ratatui::crossterm::event::{
  Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

#[derive(Default)]
pub(super) struct ReplyParser {
  pending: Vec<u8>,
}

impl ReplyParser {
  pub(super) fn feed(&mut self, data: &[u8], events: &mut Vec<Event>) -> io::Result<Vec<Vec<u8>>> {
    if data.contains(&3) {
      events.push(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
      return Ok(Vec::new());
    }
    self.pending.extend_from_slice(data);
    let mut replies = Vec::new();
    while !self.pending.is_empty() {
      if self.pending[0] == 27 {
        if self.pending.len() == 1 {
          break;
        }
        if matches!(self.pending[1], b'_' | b']' | b'[' | b'O') {
          let Some(length) = sequence_len(&self.pending) else { break };
          let reply: Vec<_> = self.pending.drain(..length).collect();
          if matches!(reply[1], b'[' | b'O') && !reply.ends_with(b"t") {
            if let Some(event) = input_sequence(&reply) {
              events.push(event);
            }
          } else {
            replies.push(reply);
          }
          continue;
        }
      }
      let offset = usize::from(self.pending[0] == 27);
      let bytes = &self.pending[offset..];
      let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) if err.valid_up_to() > 0 => {
          std::str::from_utf8(&bytes[..err.valid_up_to()]).unwrap()
        }
        Err(err) if err.error_len().is_none() => break,
        Err(_) => return Err(io::ErrorKind::InvalidData.into()),
      };
      let c = text.chars().next().unwrap();
      let mut key = character(c);
      if offset > 0 {
        key.modifiers |= KeyModifiers::ALT;
      }
      events.push(Event::Key(key));
      self.pending.drain(..offset + c.len_utf8());
    }
    if self.pending.len() > 8192 {
      return Err(io::Error::new(io::ErrorKind::InvalidData, "oversize terminal reply"));
    }
    Ok(replies)
  }

  pub(super) fn has_pending(&self) -> bool {
    !self.pending.is_empty()
  }

  /// Only after every requested reply has completed may an idle lone ESC become a key.
  pub(super) fn flush_idle_esc(&mut self, events: &mut Vec<Event>) {
    if self.pending == [27] {
      self.pending.clear();
      events.push(Event::Key(KeyCode::Esc.into()));
    }
  }
}

fn sequence_len(bytes: &[u8]) -> Option<usize> {
  match bytes[1] {
    b'_' | b']' => bytes[2..].iter().enumerate().find_map(|(n, byte)| {
      if *byte == 7 {
        Some(n + 3)
      } else if *byte == 27 && bytes.get(n + 3) == Some(&b'\\') {
        Some(n + 4)
      } else {
        None
      }
    }),
    b'[' => bytes[2..].iter().position(|byte| (0x40..=0x7e).contains(byte)).map(|n| n + 3),
    b'O' => (bytes.len() >= 3).then_some(3),
    _ => None,
  }
}

fn character(c: char) -> KeyEvent {
  let (code, modifiers) = match c {
    '\r' => (KeyCode::Enter, KeyModifiers::NONE),
    '\t' => (KeyCode::Tab, KeyModifiers::NONE),
    '\x7f' => (KeyCode::Backspace, KeyModifiers::NONE),
    '\0' => (KeyCode::Char(' '), KeyModifiers::CONTROL),
    '\x01'..='\x1a' => (KeyCode::Char((c as u8 - 1 + b'a') as char), KeyModifiers::CONTROL),
    '\x1b' => (KeyCode::Esc, KeyModifiers::NONE),
    '\x1c'..='\x1f' => (KeyCode::Char((c as u8 - 0x1c + b'4') as char), KeyModifiers::CONTROL),
    _ => {
      (KeyCode::Char(c), if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE })
    }
  };
  KeyEvent::new(code, modifiers)
}

fn input_sequence(bytes: &[u8]) -> Option<Event> {
  if bytes == b"\x1b[I" || bytes == b"\x1b[O" {
    return Some(if bytes[2] == b'I' { Event::FocusGained } else { Event::FocusLost });
  }
  if bytes.starts_with(b"\x1b[<") {
    return mouse_sequence(bytes);
  }
  let text = std::str::from_utf8(&bytes[2..bytes.len() - 1]).ok()?;
  let mut params = text.split(';');
  let number = params.next().unwrap_or("");
  let code = match *bytes.last()? {
    b'A' => KeyCode::Up,
    b'B' => KeyCode::Down,
    b'C' => KeyCode::Right,
    b'D' => KeyCode::Left,
    b'H' => KeyCode::Home,
    b'F' => KeyCode::End,
    b'P'..=b'S' => KeyCode::F(bytes[bytes.len() - 1] - b'P' + 1),
    b'Z' => KeyCode::BackTab,
    b'~' => match number {
      "1" | "7" => KeyCode::Home,
      "2" => KeyCode::Insert,
      "3" => KeyCode::Delete,
      "4" | "8" => KeyCode::End,
      "5" => KeyCode::PageUp,
      "6" => KeyCode::PageDown,
      "11" => KeyCode::F(1),
      "12" => KeyCode::F(2),
      "13" => KeyCode::F(3),
      "14" => KeyCode::F(4),
      "15" => KeyCode::F(5),
      "17" => KeyCode::F(6),
      "18" => KeyCode::F(7),
      "19" => KeyCode::F(8),
      "20" => KeyCode::F(9),
      "21" => KeyCode::F(10),
      "23" => KeyCode::F(11),
      "24" => KeyCode::F(12),
      _ => return None,
    },
    _ => return None,
  };
  let value = params.next().and_then(|n| n.parse::<u8>().ok()).unwrap_or(1).saturating_sub(1);
  let mut modifiers = KeyModifiers::NONE;
  if code == KeyCode::BackTab {
    modifiers |= KeyModifiers::SHIFT;
  }
  for (bit, modifier) in
    [(1, KeyModifiers::SHIFT), (2, KeyModifiers::ALT), (4, KeyModifiers::CONTROL)]
  {
    if value & bit != 0 {
      modifiers |= modifier;
    }
  }
  Some(Event::Key(KeyEvent::new(code, modifiers)))
}

fn mouse_sequence(bytes: &[u8]) -> Option<Event> {
  let text = std::str::from_utf8(&bytes[3..bytes.len() - 1]).ok()?;
  let parts = text.split(';').map(str::parse::<u16>).collect::<Result<Vec<_>, _>>().ok()?;
  let &[button, column, row] = parts.as_slice() else { return None };
  let kind = match button & !28 {
    0 if bytes.ends_with(b"M") => MouseEventKind::Down(MouseButton::Left),
    0 if bytes.ends_with(b"m") => MouseEventKind::Up(MouseButton::Left),
    32 => MouseEventKind::Drag(MouseButton::Left),
    64 => MouseEventKind::ScrollUp,
    65 => MouseEventKind::ScrollDown,
    _ => return None,
  };
  let mut modifiers = KeyModifiers::NONE;
  for (bit, modifier) in
    [(4, KeyModifiers::SHIFT), (8, KeyModifiers::ALT), (16, KeyModifiers::CONTROL)]
  {
    if button & bit != 0 {
      modifiers |= modifier;
    }
  }
  Some(Event::Mouse(MouseEvent {
    kind,
    column: column.checked_sub(1)?,
    row: row.checked_sub(1)?,
    modifiers,
  }))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn key(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
  }

  #[test]
  fn every_reply_split_preserves_keyboard_input() {
    let reply =
      b"\x1b_Gi=123;ERROR: invalid q and a\x1b\\\x1b]10;rgb:1111/2222/3333\x07\x1b[4;400;800t";
    for split in 0..=reply.len() {
      let mut parser = ReplyParser::default();
      let mut events = Vec::new();
      let mut replies = parser.feed(&reply[..split], &mut events).unwrap();
      replies.extend(parser.feed(&reply[split..], &mut events).unwrap());
      assert_eq!(replies.len(), 3, "split {split}");
      assert!(events.is_empty(), "split {split}");
      parser.feed(b"aaq", &mut events).unwrap();
      assert_eq!(events, vec![key('a'), key('a'), key('q')]);
    }
  }

  #[test]
  fn focus_arrows_controls_alt_and_unicode_are_preserved() {
    let mut parser = ReplyParser::default();
    let mut events = Vec::new();
    parser.feed(b"\x1b[O\x1b[I\x1b[A\x1b[1;5D\x1b[5~\x1bOP\x1b/\r\t\x7f", &mut events).unwrap();
    assert_eq!(&events[..2], &[Event::FocusLost, Event::FocusGained]);
    assert_eq!(events[2], Event::Key(KeyCode::Up.into()));
    assert_eq!(events[3], Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL)));
    assert_eq!(events[4], Event::Key(KeyCode::PageUp.into()));
    assert_eq!(events[5], Event::Key(KeyCode::F(1).into()));
    assert_eq!(events[6], Event::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::ALT)));
    assert_eq!(
      &events[7..],
      &[
        Event::Key(KeyCode::Enter.into()),
        Event::Key(KeyCode::Tab.into()),
        Event::Key(KeyCode::Backspace.into())
      ]
    );
    events.clear();
    parser.feed(&"中".as_bytes()[..1], &mut events).unwrap();
    parser.feed(&"中".as_bytes()[1..], &mut events).unwrap();
    assert_eq!(events, vec![key('中')]);
  }

  #[test]
  fn raw_ctrl_c_wins_over_incomplete_reply() {
    for prefix in [b"\x1b_Gi=123;".as_slice(), b"\x1b]11;rgb:", b"\x1b[4;100;"] {
      let mut parser = ReplyParser::default();
      let mut events = Vec::new();
      parser.feed(prefix, &mut events).unwrap();
      parser.feed(b"\x03", &mut events).unwrap();
      assert_eq!(
        events,
        vec![Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))]
      );
    }
  }

  #[test]
  fn late_escape_and_utf8_continue_in_the_same_byte_parser() {
    let mut parser = ReplyParser::default();
    let mut events = Vec::new();
    parser.feed(b"\x1b", &mut events).unwrap();
    assert!(parser.has_pending());
    let replies = parser.feed(b"_Gi=123;ERROR: a q\x1b\\", &mut events).unwrap();
    assert_eq!(replies.len(), 1);
    assert!(events.is_empty());
    parser.feed(b"\xe4", &mut events).unwrap();
    assert!(parser.has_pending());
    parser.feed(b"\xb8\xadqa", &mut events).unwrap();
    assert_eq!(events, vec![key('中'), key('q'), key('a')]);
    assert!(!parser.has_pending());
  }

  #[test]
  fn oversize_replies_fail_without_resetting_framing() {
    let mut parser = ReplyParser::default();
    parser.feed(b"\x1b]11;rgb:", &mut Vec::new()).unwrap();
    let mut events = Vec::new();
    assert_eq!(
      parser.feed(&vec![b'a'; 9000], &mut events).unwrap_err().kind(),
      io::ErrorKind::InvalidData
    );
    assert!(events.is_empty());
    assert!(parser.has_pending());
    parser.feed(b"\x03", &mut events).unwrap();
    assert_eq!(events, vec![Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))]);
  }

  #[test]
  fn idle_flush_never_releases_partial_utf8_or_control_sequences() {
    for prefix in [b"\xe4".as_slice(), b"\x1b[", b"\x1b]11;rgb:", b"\x1b_Gi=123;"] {
      let mut parser = ReplyParser::default();
      let mut events = Vec::new();
      parser.feed(prefix, &mut events).unwrap();
      parser.flush_idle_esc(&mut events);
      assert!(parser.has_pending());
      assert!(events.is_empty());
    }
    let mut parser = ReplyParser::default();
    let mut events = Vec::new();
    parser.feed(b"\x1b", &mut events).unwrap();
    parser.flush_idle_esc(&mut events);
    assert_eq!(events, vec![Event::Key(KeyCode::Esc.into())]);
    assert!(!parser.has_pending());
  }

  #[test]
  fn sgr_left_click_and_wheel_events_survive_every_split() {
    let bytes = b"\x1b[<8;80;24M\x1b[<0;80;24m\x1b[<64;4;5M\x1b[<65;4;5M";
    for split in 0..=bytes.len() {
      let mut parser = ReplyParser::default();
      let mut events = Vec::new();
      parser.feed(&bytes[..split], &mut events).unwrap();
      parser.feed(&bytes[split..], &mut events).unwrap();
      let expected = [
        (MouseEventKind::Down(MouseButton::Left), 79, 23, KeyModifiers::ALT),
        (MouseEventKind::Up(MouseButton::Left), 79, 23, KeyModifiers::NONE),
        (MouseEventKind::ScrollUp, 3, 4, KeyModifiers::NONE),
        (MouseEventKind::ScrollDown, 3, 4, KeyModifiers::NONE),
      ]
      .map(|(kind, column, row, modifiers)| {
        Event::Mouse(MouseEvent { kind, column, row, modifiers })
      });
      assert_eq!(events, expected, "split {split}");
    }
  }
}
