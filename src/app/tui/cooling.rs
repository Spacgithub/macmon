//! Real fan identities, cached images and the optional graphical cooling row.

use std::time::Duration;

use macmon::FanMetric;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Paragraph};

use super::fan_clock::Rotor;
use super::fan_raster::{FanRaster, Palette};
use super::layout::{LayoutPlan, Metric, compute_console_layout};
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
      frame.render_widget(block.title(title), place.card);
      let label = Line::from(vec![theme::heading(image.rpm.to_string()), theme::dim(" RPM")]);
      let row = Rect::new(place.card.x + 1, place.card.bottom() - 2, place.card.width - 2, 1);
      frame.render_widget(Paragraph::new(label).centered(), row);
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

pub(super) fn layout(
  area: Rect,
  procs: bool,
  clusters: usize,
  power: bool,
  fan_count: usize,
  cell_pixels: Option<(u16, u16)>,
) -> (LayoutPlan, Vec<Placement>) {
  let fallback = compute_console_layout(area, procs, clusters, power);
  let Some((cell_width, cell_height)) = cell_pixels else { return (fallback, Vec::new()) };
  if fallback.compute.is_none()
    || !(1..=2).contains(&fan_count)
    || cell_width == 0
    || cell_height == 0
  {
    return (fallback, Vec::new());
  }
  let count = fan_count as u16;
  let target: u16 = if count == 1 { 240 } else { 200 };
  let group_min = u16::try_from(clusters.saturating_add(1).saturating_mul(3).saturating_add(2))
    .unwrap_or(u16::MAX)
    .max(12);
  for (side, keep_procs) in [(target, true), (180, true), (target, false), (180, false)] {
    if keep_procs && !procs {
      continue;
    }
    let (columns, rows) = (side.div_ceil(cell_width) + 1, side.div_ceil(cell_height) + 1);
    let right_width =
      (fallback.boxes.last().unwrap().1.width).max(count * (columns + 2) + count - 1);
    let top_height = group_min.max(4 + 8 + rows + 3).saturating_add(8);
    if right_width + 1 + 36 > area.width - 2
      || u32::from(top_height) + u32::from(keep_procs) * 6 > u32::from(area.height)
    {
      continue;
    }
    let top = Rect { height: if keep_procs { top_height } else { area.height }, ..area };
    let inner = top.inner(Margin::new(1, 1));
    let network = Rect { y: inner.bottom() - 6, height: 6, ..inner };
    let compute = Rect { width: inner.width - right_width - 1, height: inner.height - 6, ..inner };
    let right = Rect { x: compute.right() + 1, width: right_width, ..compute };
    let memory = Rect { height: 4, ..right };
    let telemetry = Rect { y: right.y + 4, height: 8, ..right };
    let cooling = Rect { y: right.y + 12, height: right.height - 12, ..right };
    let body = compute.inner(Margin::new(1, 1));
    let edge =
      |index: usize| (u64::from(body.height) * index as u64 / (clusters + 1) as u64) as u16;
    let mut boxes: Vec<_> = (0..clusters)
      .map(Metric::Cluster)
      .chain([Metric::Gpu])
      .enumerate()
      .map(|(index, metric)| {
        (metric, Rect { y: body.y + edge(index), height: edge(index + 1) - edge(index), ..body })
      })
      .collect();
    boxes.push((Metric::Ram, memory));
    let placements = (0..count)
      .map(|index| {
        let edge =
          |i| (u32::from(cooling.width - count + 1) * u32::from(i) / u32::from(count)) as u16;
        let card = Rect {
          x: cooling.x + edge(index) + index,
          width: edge(index + 1) - edge(index),
          ..cooling
        };
        let image = Rect::new(
          card.x + 1 + (card.width - 2 - columns) / 2,
          card.y + 1 + (card.height - 3 - rows) / 2,
          columns,
          rows,
        );
        Placement { card, image, side }
      })
      .collect();
    let proc =
      keep_procs.then_some(Rect { y: top.bottom(), height: area.height - top.height, ..area });
    return (
      LayoutPlan {
        top: Some(top),
        boxes,
        compute: Some(compute),
        telemetry: Some(telemetry),
        network: Some(network),
        proc,
      },
      placements,
    );
  }
  (fallback, Vec::new())
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
  fn unsupported_small_vm_empty_or_many_fans_keep_current_layout() {
    for (area, power, count, cell) in [
      (Rect::new(0, 0, 120, 40), true, 2, None),
      (Rect::new(0, 0, 120, 40), true, 2, Some((0, 18))),
      (Rect::new(0, 0, 80, 24), true, 2, Some((9, 18))),
      (Rect::new(0, 0, 120, 40), false, 2, Some((9, 18))),
      (Rect::new(0, 0, 120, 40), true, 0, Some((9, 18))),
      (Rect::new(0, 0, 120, 40), true, 3, Some((9, 18))),
      (Rect::default(), true, 2, Some((9, 18))),
    ] {
      let (plan, placements) = layout(area, true, 2, power, count, cell);
      assert_eq!(plan, compute_console_layout(area, true, 2, power));
      assert!(placements.is_empty());
    }
  }

  #[test]
  fn pixel_bounds_and_cards_do_not_overlap_and_process_rows_remain_useful() {
    for (area, count, cell) in [
      (Rect::new(0, 0, 120, 40), 2, (9, 18)),
      (Rect::new(3, 2, 120, 40), 1, (9, 18)),
      (Rect::new(7, 5, 160, 60), 2, (8, 20)),
      (Rect::new(0, 0, 140, 50), 1, (9, 18)),
    ] {
      let (plan, placements) = layout(area, true, 2, true, count, Some(cell));
      assert_eq!(placements.len(), count);
      let top = plan.top.unwrap();
      assert_eq!(top.intersection(area), top);
      assert!(plan.proc.unwrap().height >= 6);
      assert_eq!(plan.network.unwrap().width, top.width - 2);
      let mut cards = Vec::new();
      for placement in placements {
        assert!(placement.side >= 180);
        assert_eq!(placement.card.intersection(placement.image), placement.image);
        assert_eq!(placement.card.intersection(top), placement.card);
        assert!(placement.image.bottom() <= placement.card.bottom() - 2);
        assert!(u32::from(placement.image.width) * u32::from(cell.0) >= u32::from(placement.side));
        assert!(u32::from(placement.image.height) * u32::from(cell.1) >= u32::from(placement.side));
        for sibling in cards.iter().chain(
          [
            plan.compute.unwrap(),
            plan.telemetry.unwrap(),
            plan.boxes.last().unwrap().1,
            plan.network.unwrap(),
          ]
          .iter(),
        ) {
          assert!(placement.card.intersection(*sibling).is_empty());
        }
        cards.push(placement.card);
      }
    }
    let (_, dual) = layout(Rect::new(0, 0, 120, 40), true, 2, true, 2, Some((9, 18)));
    assert!(dual.iter().all(|placement| placement.side == 180));
    let (_, single) = layout(Rect::new(0, 0, 140, 50), true, 2, true, 1, Some((9, 18)));
    assert_eq!(single[0].side, 240);
  }
}
