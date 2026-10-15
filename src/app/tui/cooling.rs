//! Real fan identities, cached images and the optional graphical cooling row.

use std::time::Duration;

use macmon::FanMetric;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType};

use super::fan_clock::Rotor;
use super::fan_raster::{FanRaster, Palette};
use super::theme;

#[derive(Debug)]
pub(super) struct FanImage {
  pub(super) name: String,
  pub(super) rpm: u32,
  pub(super) rgba: Vec<u8>,
  pub(super) rotor: Rotor,
}

#[derive(Debug, Default)]
pub(super) struct Cooling {
  items: Vec<FanImage>,
  raster: Option<FanRaster>,
}

impl Cooling {
  pub(super) fn sync(&mut self, fans: &[FanMetric]) {
    let mut previous = std::mem::take(&mut self.items);
    self.items = fans
      .iter()
      .map(|fan| {
        let mut image = previous
          .iter()
          .position(|item| item.name == fan.name)
          .map(|index| previous.remove(index))
          .unwrap_or_else(|| FanImage {
            name: fan.name.clone(),
            rpm: fan.rpm,
            rgba: Vec::new(),
            rotor: Rotor::default(),
          });
        image.rpm = fan.rpm;
        image.rotor.set_rpm(Some(fan.rpm));
        image
      })
      .collect();
  }

  pub(super) fn items(&self) -> &[FanImage] {
    &self.items
  }

  pub(super) fn moving(&self) -> bool {
    self.items.iter().any(|image| image.rotor.moving())
  }

  pub(super) fn frame(&mut self, delta: Duration) {
    for image in &mut self.items {
      image.rotor.advance(delta);
    }
  }

  /// Only geometry or fan-count changes allocate; frames reuse each output buffer.
  pub(super) fn rasterize(&mut self, side: u16, palette: Palette, moving_only: bool) -> bool {
    if self.raster.as_ref().is_none_or(|raster| raster.size() != usize::from(side)) {
      self.raster = FanRaster::new(usize::from(side));
    }
    let Some(raster) = &self.raster else { return false };
    for image in &mut self.items {
      if moving_only && !image.rotor.moving() {
        continue;
      }
      image.rgba.resize(raster.rgba_len(), 0);
      if !raster.render_into(image.rotor.angle, palette, &mut image.rgba) {
        return false;
      }
    }
    true
  }

  pub(super) fn render(&self, frame: &mut Frame, placements: &[Placement]) {
    for (image, place) in self.items.iter().zip(placements) {
      let title = Line::from(vec![
        theme::value(" Cooling ", theme::SECTION),
        theme::heading(&image.name),
        theme::text(" "),
      ]);
      let block = Block::bordered().border_type(BorderType::Rounded).border_style(theme::BORDER);
      let label =
        Line::from(vec![theme::heading(format!(" {}", image.rpm)), theme::dim(" RPM ")]).centered();
      frame.render_widget(block.title(title).title_bottom(label), place.card);
    }
  }
}

/// Image origin/coverage in cells, and its actual square side in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Placement {
  pub(super) card: Rect,
  pub(super) image: Rect,
  pub(super) side: u16,
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fan(name: &str, rpm: u32) -> FanMetric {
    FanMetric { name: name.into(), rpm, max_rpm: None }
  }

  #[test]
  fn identities_survive_reorder_and_zero_or_disappearance_stops() {
    let mut cooling = Cooling::default();
    cooling.sync(&[fan("fan0", 1000), fan("fan1", 4000)]);
    for _ in 0..60 {
      cooling.frame(Duration::from_nanos(1_000_000_000 / 60));
    }
    let angles: Vec<_> = cooling.items.iter().map(|item| item.rotor.angle).collect();
    assert!(angles[1] > angles[0] && cooling.moving());
    cooling.sync(&[fan("fan1", 0), fan("fan0", 1000)]);
    assert_eq!(cooling.items[0].rotor.angle, angles[1]);
    assert_eq!(cooling.items[1].rotor.angle, angles[0]);
    assert_eq!(cooling.items[0].rotor.velocity, 0.0);
    cooling.frame(Duration::from_secs(1));
    assert_eq!(cooling.items[0].rotor.angle, angles[1]);
    cooling.sync(&[fan("fan1", 0)]);
    assert_eq!(cooling.items.len(), 1);
    assert!(!cooling.moving());
    cooling.sync(&[]);
    assert!(cooling.items.is_empty() && !cooling.moving());
  }

  #[test]
  fn images_reuse_output_storage_and_geometry_between_frames() {
    let mut cooling = Cooling::default();
    cooling.sync(&[fan("actual", 1700)]);
    let palette = Palette { background: [1, 2, 3], foreground: [4, 5, 6], accent: [7, 8, 9] };
    assert!(cooling.rasterize(180, palette, false));
    let allocation = cooling.items()[0].rgba.as_ptr();
    let geometry = cooling.raster.as_ref().unwrap().geometry_ptr();
    cooling.frame(Duration::from_millis(17));
    assert!(cooling.rasterize(180, palette, false));
    assert_eq!(cooling.items()[0].rgba.as_ptr(), allocation);
    assert_eq!(cooling.raster.as_ref().unwrap().geometry_ptr(), geometry);
    assert_eq!(cooling.items()[0].name, "actual");
    assert_eq!(cooling.items()[0].rpm, 1700);
    assert!(!cooling.rasterize(0, palette, false));
  }

  #[test]
  fn animation_frames_leave_a_stopped_fans_pixels_unchanged() {
    let mut cooling = Cooling::default();
    cooling.sync(&[fan("still", 0), fan("turning", 1800)]);
    let palette =
      Palette { background: [1, 2, 3], foreground: [220, 230, 240], accent: [5, 180, 190] };
    assert!(cooling.rasterize(180, palette, false));
    let before: Vec<_> = cooling.items().iter().map(|fan| fan.rgba.clone()).collect();
    cooling.frame(Duration::from_millis(100));
    assert!(cooling.rasterize(180, palette, true));
    assert_eq!(cooling.items()[0].rgba, before[0]);
    assert_ne!(cooling.items()[1].rgba, before[1]);
  }
}
