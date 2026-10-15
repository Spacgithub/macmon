//! Owned Kitty RAW32 images; inline transport for Warp, file transport elsewhere.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone)]
struct Resources {
  directory: PathBuf,
  ids: [u32; 2],
  exposed: bool,
}

// Retain ownership until terminal restoration, including error returns and aborting panic hooks.
static ACTIVE: Mutex<Option<Resources>> = Mutex::new(None);
#[cfg(test)]
pub(super) static TEST_SESSION: Mutex<()> = Mutex::new(());
#[cfg(test)]
pub(super) static FRAME_SUBMISSIONS: std::sync::atomic::AtomicU64 =
  std::sync::atomic::AtomicU64::new(0);

pub(super) struct FanImages {
  resources: Resources,
  areas: [Option<[u16; 4]>; 2],
  inline: bool,
}

impl FanImages {
  pub(super) fn new() -> io::Result<Self> {
    Self::with_inline(std::env::var("TERM_PROGRAM").is_ok_and(|name| name == "WarpTerminal"))
  }

  fn with_inline(inline: bool) -> io::Result<Self> {
    let mut nonce = [0; 8];
    File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    let pid = std::process::id();
    let id = ((u64::from_le_bytes(nonce) as u32 ^ pid) & 0x3fff_ffff) | 0x4000_0000;
    let directory =
      std::env::temp_dir().join(format!("macmon-fans-{pid}-{:016x}", u64::from_le_bytes(nonce)));
    DirBuilder::new().mode(0o700).create(&directory)?;
    let resources = Resources { directory, ids: [id, id | 0x8000_0000], exposed: false };
    let registration = resources.clone();
    let occupied = {
      let mut active = ACTIVE.lock().unwrap_or_else(|err| err.into_inner());
      if active.is_some() {
        true
      } else {
        *active = Some(registration);
        false
      }
    };
    if occupied {
      let _ = fs::remove_dir(&resources.directory);
      return Err(io::Error::new(io::ErrorKind::AlreadyExists, "fan image session already active"));
    }
    Ok(Self { resources, areas: [None; 2], inline })
  }

  fn expose(&self) {
    if let Some(resources) = ACTIVE.lock().unwrap_or_else(|err| err.into_inner()).as_mut() {
      resources.exposed = true;
    }
  }

  /// Query the same medium and RAW32 format used by the actual fan images.
  pub(super) fn probe(&mut self, out: &mut impl Write) -> io::Result<u32> {
    let id = self.resources.ids[0];
    if self.inline {
      self.expose();
      inline(out, &format!("a=q,t=d,f=32,s=1,v=1,i={id}"), &[0; 4], 0)?;
      out.flush()?;
      return Ok(id);
    }
    let path = self.resources.directory.join("probe.raw");
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
    file.write_all(&[0; 4])?;
    drop(file);
    let payload = base64(path.as_os_str().as_bytes());
    self.expose();
    write!(out, "\x1b_Ga=q,t=f,f=32,s=1,v=1,i={id};{payload}\x1b\\")?;
    out.flush()?;
    Ok(id)
  }

  /// `origin` is zero-based; `cells` reserves space for the native square pixel image.
  pub(super) fn render(
    &mut self,
    out: &mut impl Write,
    rgba: &[u8],
    side: u16,
    slot: usize,
    origin: (u16, u16),
    cells: (u16, u16),
  ) -> io::Result<()> {
    let bytes = usize::from(side).checked_pow(2).and_then(|n| n.checked_mul(4));
    if side == 0 || slot >= 2 || cells.0 == 0 || cells.1 == 0 || bytes != Some(rgba.len()) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid fan image dimensions or slot",
      ));
    }
    let path = self.resources.directory.join(format!("{slot}.raw"));
    if !self.inline {
      let staging = self.resources.directory.join(format!("{slot}.pending"));
      let stored = (|| {
        let mut file =
          OpenOptions::new().write(true).create_new(true).mode(0o600).open(&staging)?;
        file.write_all(rgba)?;
        drop(file);
        fs::rename(&staging, &path)
      })();
      if stored.is_err() {
        let _ = fs::remove_file(&staging);
      }
      stored?;
    }
    let id = self.resources.ids[slot];
    let area = [origin.0, origin.1, cells.0, cells.1];
    if self.areas[slot].is_some_and(|old| old != area) {
      delete(out, id, false)?;
      self.areas[slot] = None;
    }
    self.expose();
    write!(out, "\x1b7\x1b[{};{}H", u32::from(origin.1) + 1, u32::from(origin.0) + 1,)?;
    if self.inline {
      inline(out, &format!("a=T,t=d,f=32,s={side},v={side},i={id},p=1,C=1"), rgba, 2)?;
    } else {
      let payload = base64(path.as_os_str().as_bytes());
      write!(out, "\x1b_Ga=T,t=f,f=32,s={side},v={side},i={id},p=1,C=1,q=2;{payload}\x1b\\")?;
    }
    out.write_all(b"\x1b8")?;
    out.flush()?;
    self.areas[slot] = Some(area);
    #[cfg(test)]
    FRAME_SUBMISSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(())
  }

  pub(super) fn hide(&mut self, out: &mut impl Write) -> io::Result<()> {
    let mut result = Ok(());
    for (slot, area) in self.areas.iter_mut().enumerate() {
      if area.is_some() {
        match delete(out, self.resources.ids[slot], false) {
          Ok(()) => *area = None,
          Err(err) => result = Err(err),
        }
      }
    }
    result.and(out.flush())
  }
}

fn delete(out: &mut impl Write, id: u32, data: bool) -> io::Result<()> {
  let kind = if data { 'I' } else { 'i' };
  write!(out, "\x1b_Ga=d,d={kind},i={id},q=2;\x1b\\")
}

/// Call before any terminal mode commands: close partial APCs, then delete owned images/files.
/// Continue restoring terminal modes even if image cleanup cannot write to the terminal.
pub(super) fn cleanup(out: &mut impl Write) {
  let resources = ACTIVE.lock().unwrap_or_else(|err| err.into_inner()).take();
  if let Some(resources) = resources {
    if resources.exposed {
      // End the APC before either image deletion or the caller's terminal mode commands.
      let _ = out.write_all(b"\x1b\\");
      for id in resources.ids {
        let _ = delete(out, id, true);
      }
    }
    let _ = out.flush();
    let _ = fs::remove_dir_all(resources.directory);
  }
}

fn inline(out: &mut impl Write, controls: &str, rgba: &[u8], quiet: u8) -> io::Result<()> {
  let encoded = base64(rgba);
  let count = encoded.len().div_ceil(4096);
  for (index, chunk) in encoded.as_bytes().chunks(4096).enumerate() {
    let more = u8::from(index + 1 < count);
    if index == 0 {
      write!(out, "\x1b_G{controls},q={quiet},m={more};")?;
    } else {
      write!(out, "\x1b_Gm={more},q={quiet};")?;
    }
    out.write_all(chunk)?;
    out.write_all(b"\x1b\\")?;
  }
  Ok(())
}

fn base64(bytes: &[u8]) -> String {
  const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
  for chunk in bytes.chunks(3) {
    let value = (u32::from(chunk[0]) << 16)
      | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
      | u32::from(*chunk.get(2).unwrap_or(&0));
    for n in 0..4 {
      result.push(if n > chunk.len() {
        '='
      } else {
        TABLE[((value >> (18 - n * 6)) & 63) as usize] as char
      });
    }
  }
  result
}

#[cfg(test)]
mod tests {
  use std::os::unix::fs::PermissionsExt;

  use super::*;

  #[test]
  fn base64_paths_preserve_arbitrary_bytes_and_padding() {
    for (bytes, encoded) in [
      (&b""[..], ""),
      (&b"M"[..], "TQ=="),
      (&b"Ma"[..], "TWE="),
      (&b"Man"[..], "TWFu"),
      (&[0, 255, 128][..], "AP+A"),
    ] {
      assert_eq!(base64(bytes), encoded);
    }
  }

  #[test]
  fn owned_files_updates_movement_hiding_and_cleanup() {
    let _session = TEST_SESSION.lock().unwrap();
    let mut out = Vec::new();
    cleanup(&mut out);
    let images = FanImages::with_inline(false).unwrap();
    let unused = images.resources.directory.clone();
    cleanup(&mut out);
    assert!(out.is_empty());
    assert!(!unused.exists());
    let mut images = FanImages::with_inline(false).unwrap();
    let directory = images.resources.directory.clone();
    let ids = images.resources.ids;
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[0], 0);
    assert_eq!(fs::metadata(&directory).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(FanImages::with_inline(false).err().unwrap().kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(images.probe(&mut out).unwrap(), ids[0]);
    let probe_path = directory.join("probe.raw");
    assert_eq!(fs::read(&probe_path).unwrap(), [0; 4]);
    assert_eq!(fs::metadata(&probe_path).unwrap().permissions().mode() & 0o777, 0o600);
    let probe = String::from_utf8(out.clone()).unwrap();
    assert_eq!(
      probe,
      format!(
        "\x1b_Ga=q,t=f,f=32,s=1,v=1,i={};{}\x1b\\",
        ids[0],
        base64(probe_path.as_os_str().as_bytes())
      )
    );
    let header = probe.split_once(';').unwrap().0;
    assert!(!header.contains("a=T"));
    assert!(!header.contains(",p="));
    assert!(!header.contains(",c="));
    assert!(!header.contains(",r="));
    assert!(!probe.contains("\x1b["));
    assert_eq!(images.areas, [None; 2]);
    out.clear();
    let rgba = [42; 16];
    for slot in 0..2 {
      images.render(&mut out, &rgba, 2, slot, (3, 4), (5, 6)).unwrap();
      let path = directory.join(format!("{slot}.raw"));
      assert_eq!(fs::read(&path).unwrap(), rgba);
      assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 3);
    let first = String::from_utf8(out.clone()).unwrap();
    assert!(first.contains("\x1b7\x1b[5;4H\x1b_Ga=T,t=f,f=32,s=2,v=2,"));
    assert!(first.contains("p=1,C=1,q=2;"));
    assert!(!first.contains(",c="));
    assert!(!first.contains(",r="));
    assert!(!first.contains("a=d"));
    out.clear();
    images.render(&mut out, &[99; 16], 2, 0, (3, 4), (5, 6)).unwrap();
    assert_eq!(fs::read(directory.join("0.raw")).unwrap(), [99; 16]);
    assert!(!String::from_utf8(out.clone()).unwrap().contains("a=d"));
    out.clear();
    images.render(&mut out, &rgba, 2, 0, (8, 4), (5, 6)).unwrap();
    let moved = String::from_utf8(out.clone()).unwrap();
    assert!(moved.starts_with(&format!("\x1b_Ga=d,d=i,i={},q=2;\x1b\\", ids[0])));
    assert!(moved.contains("\x1b[5;9H"));
    out.clear();
    images.hide(&mut out).unwrap();
    for id in ids {
      assert!(String::from_utf8(out.clone()).unwrap().contains(&format!("d=i,i={id},q=2")));
    }
    assert_eq!(images.areas, [None; 2]);
    assert_eq!(
      images.render(&mut out, &rgba, 2, 2, (0, 0), (1, 1)).unwrap_err().kind(),
      io::ErrorKind::InvalidInput
    );
    assert_eq!(
      images.render(&mut out, &rgba[..15], 2, 0, (0, 0), (1, 1)).unwrap_err().kind(),
      io::ErrorKind::InvalidInput
    );
    out.clear();
    cleanup(&mut out);
    let deleted = String::from_utf8(out).unwrap();
    for id in ids {
      assert!(deleted.contains(&format!("d=I,i={id},q=2")));
    }
    assert!(!deleted.contains("d=A"));
    assert!(!directory.exists());

    struct Broken;
    impl Write for Broken {
      fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
      }
      fn flush(&mut self) -> io::Result<()> {
        Ok(())
      }
    }
    let images = FanImages::with_inline(false).unwrap();
    let directory = images.resources.directory.clone();
    cleanup(&mut Broken);
    assert!(!directory.exists());
  }

  #[test]
  fn terminal_restore_closes_a_failed_image_apc_before_disabling_modes_once() {
    struct FailOnce {
      bytes: Vec<u8>,
      fail_at: usize,
      failed: bool,
    }
    impl Write for FailOnce {
      fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.failed && self.bytes.len() == self.fail_at {
          self.failed = true;
          return Err(io::ErrorKind::BrokenPipe.into());
        }
        let count =
          if self.failed { bytes.len() } else { bytes.len().min(self.fail_at - self.bytes.len()) };
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
      }
      fn flush(&mut self) -> io::Result<()> {
        Ok(())
      }
    }

    let _session = TEST_SESSION.lock().unwrap();
    for frame in [false, true] {
      let mut images = FanImages::with_inline(true).unwrap();
      let directory = images.resources.directory.clone();
      let cursor_prefix = if frame { b"\x1b7\x1b[1;1H".len() } else { 0 };
      let mut out = FailOnce { bytes: Vec::new(), fail_at: cursor_prefix + 7, failed: false };
      let result = if frame {
        images.render(&mut out, &[42; 16], 2, 0, (0, 0), (2, 2))
      } else {
        images.probe(&mut out).map(|_| ())
      };
      assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
      assert!(out.failed);
      let apc = out.bytes.windows(3).position(|bytes| bytes == b"\x1b_G").unwrap();
      assert!(!out.bytes[apc..].windows(2).any(|bytes| bytes == b"\x1b\\"));
      let partial = out.bytes.len();
      let active = std::sync::atomic::AtomicBool::new(true);
      let mut raw_off = 0;
      assert!(super::super::restore_term_once(&active, &mut out, || {
        raw_off += 1;
        Err(io::ErrorKind::Other.into())
      }));
      let restored = std::str::from_utf8(&out.bytes[partial..]).unwrap();
      assert!(restored.starts_with("\x1b\\"), "close the image APC first: {restored:?}");
      let first_mode = restored.find("\x1b[?").unwrap();
      for id in images.resources.ids {
        let deletion = restored.find(&format!("\x1b_Ga=d,d=I,i={id},q=2;\x1b\\")).unwrap();
        assert!(deletion < first_mode, "delete images before restoring modes");
      }
      assert!(restored.contains("\x1b[?1004l"));
      assert!(restored.ends_with("\x1b[?1049l\x1b[?25h"));
      assert!(!directory.exists());
      assert_eq!(raw_off, 1);
      let length = out.bytes.len();
      assert!(!super::super::restore_term_once(&active, &mut out, || {
        raw_off += 1;
        Ok(())
      }));
      assert_eq!(out.bytes.len(), length);
      assert_eq!(raw_off, 1);
    }
  }

  #[test]
  fn inline_probe_and_frames_are_chunked_without_file_access() {
    let _session = TEST_SESSION.lock().unwrap();
    let mut images = FanImages::with_inline(true).unwrap();
    let mut out = Vec::new();
    let id = images.probe(&mut out).unwrap();
    assert_eq!(
      String::from_utf8(out.clone()).unwrap(),
      format!("\x1b_Ga=q,t=d,f=32,s=1,v=1,i={id},q=0,m=0;AAAAAA==\x1b\\")
    );
    for side in [1_u16, 32, 180, 240] {
      out.clear();
      let rgba: Vec<u8> = (0..usize::from(side).pow(2) * 4).map(|i| (i % 251) as u8).collect();
      images.render(&mut out, &rgba, side, 0, (3, 4), (30, 15)).unwrap();
      let encoded = base64(&rgba);
      let text = String::from_utf8(out.clone()).unwrap();
      let body = text.strip_prefix("\x1b7\x1b[5;4H").unwrap().strip_suffix("\x1b8").unwrap();
      let chunks: Vec<_> = body.split_terminator("\x1b\\").collect();
      assert_eq!(chunks.len(), encoded.len().div_ceil(4096));
      let mut joined = String::new();
      for (index, chunk) in chunks.iter().enumerate() {
        let (header, data) = chunk.strip_prefix("\x1b_G").unwrap().split_once(';').unwrap();
        let more = u8::from(index + 1 < chunks.len());
        assert!(!data.is_empty() && data.len() <= 4096 && data.len() % 4 == 0);
        if index == 0 {
          assert_eq!(header, format!("a=T,t=d,f=32,s={side},v={side},i={id},p=1,C=1,q=2,m={more}"));
        } else {
          assert_eq!(header, format!("m={more},q=2"));
        }
        joined.push_str(data);
      }
      assert_eq!(joined, encoded);
      assert_eq!(fs::read_dir(&images.resources.directory).unwrap().count(), 0);
    }
    out.clear();
    cleanup(&mut out);
    let cleaned = String::from_utf8(out).unwrap();
    assert!(cleaned.starts_with("\x1b\\"));
    for id in images.resources.ids {
      assert!(cleaned.contains(&format!("a=d,d=I,i={id},q=2")));
    }
    assert!(!images.resources.directory.exists());
  }
}
