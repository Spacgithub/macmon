//! Content-sized cooling cards in a wide ledger or a compact two-row metric grid.

use ratatui::layout::{Margin, Rect};

use super::cooling::Placement;
use super::layout::{LayoutPlan, Metric, compute_console_layout, compute_layout};

#[derive(Clone, Copy)]
struct CardSize {
  side: u16,
  columns: u16,
  rows: u16,
  width: u16,
  height: u16,
}

fn fit(width: u16, height: u16, count: u16, cell: (u16, u16), native: bool) -> Option<CardSize> {
  let guard = u16::from(native);
  let available = width.checked_sub(count - 1)? / count;
  if available < 16 {
    return None;
  }
  let columns = available.checked_sub(2 + guard)?;
  let rows = height.checked_sub(2 + guard)?;
  let side = columns.saturating_mul(cell.0).min(rows.saturating_mul(cell.1)).min(120);
  if side < 48 {
    return None;
  }
  let (columns, rows) = (side.div_ceil(cell.0) + guard, side.div_ceil(cell.1) + guard);
  Some(CardSize { side, columns, rows, width: (columns + 2).max(16), height: rows + 2 })
}

fn placements(row: Rect, count: u16, size: CardSize) -> Vec<Placement> {
  let width = count * size.width + count - 1;
  (0..count)
    .map(|index| {
      let card = Rect::new(
        row.x + (row.width - width) / 2 + index * (size.width + 1),
        row.y,
        size.width,
        size.height,
      );
      let image = Rect::new(
        card.x + 1 + (card.width - 2 - size.columns) / 2,
        card.y + 1,
        size.columns,
        size.rows,
      );
      Placement { card, image, side: size.side }
    })
    .collect()
}

/// Small cards confined to an expanded cooling panel; hidden/short panels have no image writes.
pub(super) fn panel_cards(
  area: Rect,
  count: usize,
  cell: (u16, u16),
  native: bool,
) -> Vec<Placement> {
  if !(1..=2).contains(&count) || cell.0 == 0 || cell.1 == 0 {
    return Vec::new();
  }
  let row = area.inner(Margin::new(1, 1));
  fit(row.width, row.height.min(8), count as u16, cell, native)
    .map(|size| placements(row, count as u16, size))
    .unwrap_or_default()
}

fn outer(area: Rect, procs: bool, minimum: u16) -> (Rect, Option<Rect>) {
  let top = Rect { height: if procs { minimum } else { area.height }, ..area };
  let proc = procs.then_some(Rect { y: top.bottom(), height: area.height - top.height, ..area });
  (top, proc)
}

pub(super) fn layout(
  area: Rect,
  procs: bool,
  clusters: usize,
  power: bool,
  fan_count: usize,
  cell_pixels: (u16, u16),
  native: bool,
) -> (LayoutPlan, Vec<Placement>) {
  let fallback = compute_console_layout(area, procs, clusters, power);
  if !power || !(1..=2).contains(&fan_count) || cell_pixels.0 == 0 || cell_pixels.1 == 0 {
    return (fallback, Vec::new());
  }
  // A denser console must not displace a process list that the original compact grid can show.
  let keep_procs = procs && compute_layout(area, true, clusters, power).proc.is_some();
  let fallback = if keep_procs && fallback.proc.is_none() {
    compute_layout(area, procs, clusters, power)
  } else {
    fallback
  };
  let budget = area.height.saturating_sub(u16::from(keep_procs) * 6);
  let count = fan_count as u16;
  let width = area.width.saturating_sub(2);
  if area.width >= 100 {
    let right_max = width.saturating_sub(37);
    if let Some(size) = fit(right_max, budget.saturating_sub(20), count, cell_pixels, native) {
      let tiers = u16::try_from(clusters.saturating_add(1).saturating_mul(3).saturating_add(2))
        .unwrap_or(u16::MAX);
      let minimum = tiers.max(12 + size.height).saturating_add(8);
      if minimum <= budget {
        let (top, proc) = outer(area, keep_procs, minimum);
        let inner = top.inner(Margin::new(1, 1));
        let right_width = (count * size.width + count - 1).max(34);
        let network = Rect { y: inner.bottom() - 6, height: 6, ..inner };
        let compute =
          Rect { width: inner.width - right_width - 1, height: inner.height - 6, ..inner };
        let right = Rect { x: compute.right() + 1, width: right_width, ..compute };
        let memory = Rect { height: 4, ..right };
        let telemetry = Rect { y: right.y + 4, height: 8, ..right };
        let body = compute.inner(Margin::new(1, 1));
        let edge = |i: usize| (u64::from(body.height) * i as u64 / (clusters + 1) as u64) as u16;
        let mut boxes: Vec<_> = (0..clusters)
          .map(Metric::Cluster)
          .chain([Metric::Gpu])
          .enumerate()
          .map(|(i, metric)| {
            (metric, Rect { y: body.y + edge(i), height: edge(i + 1) - edge(i), ..body })
          })
          .collect();
        boxes.push((Metric::Ram, memory));
        let row = Rect { y: right.y + 12, height: size.height, ..right };
        return (
          LayoutPlan {
            top: Some(top),
            boxes,
            compute: Some(compute),
            telemetry: Some(telemetry),
            network: Some(network),
            proc,
          },
          placements(row, count, size),
        );
      }
    }
  }
  let Some(size) = fit(width, budget.saturating_sub(12), count, cell_pixels, native) else {
    return (fallback, Vec::new());
  };
  let (top, proc) = outer(area, keep_procs, size.height + 12);
  // An eight-row shell yields only the two three-row graph groups, without its own network.
  let mut plan = compute_layout(Rect { height: 8, ..top }, false, clusters, power);
  if plan.boxes.len() != clusters.saturating_add(6) || plan.boxes.iter().any(|(_, r)| r.width < 8) {
    return (fallback, Vec::new());
  }
  let graph_height = top.height - size.height - 6;
  let upper = graph_height.div_ceil(2);
  for (_, rect) in &mut plan.boxes {
    let lower = rect.y > top.y + 1;
    rect.y = top.y + 1 + if lower { upper } else { 0 };
    rect.height = if lower { graph_height - upper } else { upper };
  }
  let inner = top.inner(Margin::new(1, 1));
  let row = Rect { y: inner.y + graph_height, height: size.height, ..inner };
  plan.top = Some(top);
  plan.proc = proc;
  plan.network = Some(Rect { y: row.bottom(), height: 4, ..inner });
  (plan, placements(row, count, size))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn assert_geometry(
    area: Rect,
    plan: &LayoutPlan,
    cards: &[Placement],
    cell: (u16, u16),
    native: bool,
  ) {
    let top = plan.top.unwrap();
    assert_eq!(top.intersection(area), top);
    if let Some(proc) = plan.proc {
      assert!(proc.height >= 6 && proc.y == top.bottom());
      assert_eq!(proc.bottom(), area.bottom());
    }
    let network = plan.network.unwrap();
    assert_eq!(network.width, area.width - 2);
    let mut siblings: Vec<_> = plan.boxes.iter().map(|(_, rect)| *rect).collect();
    siblings.extend(plan.telemetry);
    siblings.push(network);
    for card in cards {
      assert!((48..=120).contains(&card.side));
      assert!(card.card.width >= 16);
      assert_eq!(card.card.intersection(card.image), card.image);
      assert_eq!(card.card.intersection(top), card.card);
      assert_eq!(card.image.height + 2, card.card.height);
      assert_eq!(card.image.width, card.side.div_ceil(cell.0) + u16::from(native));
      assert_eq!(card.image.height, card.side.div_ceil(cell.1) + u16::from(native));
      for sibling in &siblings {
        assert!(card.card.intersection(*sibling).is_empty(), "{card:?}, {sibling:?}");
      }
      if let Some(compute) = plan.compute {
        assert!(card.card.intersection(compute).is_empty());
      }
      siblings.push(card.card);
    }
    for (_, rect) in &plan.boxes {
      assert!(rect.width > 0 && rect.height >= 3);
      assert_eq!(rect.intersection(top), *rect);
    }
  }

  #[test]
  fn wide_and_compact_windows_keep_all_cards_network_and_original_processes() {
    for (width, height) in [(120, 40), (80, 24), (160, 60)] {
      for cell in [(9, 18), (8, 16), (8, 20), (10, 16), (14, 28)] {
        for native in [false, true] {
          for count in 1..=2 {
            let area = Rect::new(3, 2, width, height);
            let (plan, cards) = layout(area, true, 2, true, count, cell, native);
            assert_eq!(cards.len(), count);
            assert!(plan.proc.is_some());
            assert_geometry(area, &plan, &cards, cell, native);
            assert_eq!(plan.network.unwrap().height, if width == 80 { 4 } else { 6 });
          }
        }
      }
    }
    let (plan, cards) = layout(Rect::new(0, 0, 80, 24), true, 2, true, 2, (9, 18), true);
    assert_eq!(cards[0].side, 54);
    assert_eq!(cards[0].card.height, 6);
    assert_eq!(plan.top.unwrap().height, 18);
    assert_eq!(plan.proc.unwrap().height, 6);
    let (_, cards) = layout(Rect::new(0, 0, 80, 24), true, 2, true, 2, (9, 18), false);
    assert_eq!(cards[0].side, 72);
  }

  #[test]
  fn image_diameter_tracks_available_pixels_instead_of_two_presets() {
    let mut native_sides = std::collections::BTreeSet::new();
    let mut text_sides = std::collections::BTreeSet::new();
    for height in 31..=40 {
      for (native, sides) in [(true, &mut native_sides), (false, &mut text_sides)] {
        let area = Rect::new(0, 0, 120, height);
        let (plan, cards) = layout(area, true, 2, true, 2, (9, 18), native);
        assert_eq!(cards.len(), 2);
        assert_geometry(area, &plan, &cards, (9, 18), native);
        sides.insert(cards[0].side);
      }
    }
    assert!(
      native_sides.contains(&54) && native_sides.contains(&90) && native_sides.contains(&108)
    );
    assert!(native_sides.len() >= 5 && text_sides.len() >= 4);
  }

  #[test]
  fn cards_do_not_stretch_with_window_height_or_when_processes_are_hidden() {
    for native in [false, true] {
      for count in 1..=2 {
        for cell in [(9, 18), (8, 16), (14, 28)] {
          let (_, original) = layout(Rect::new(0, 0, 160, 60), true, 2, true, count, cell, native);
          for (height, procs) in [(60, false), (120, true), (120, false)] {
            let area = Rect::new(0, 0, 160, height);
            let (plan, cards) = layout(area, procs, 2, true, count, cell, native);
            assert_eq!(cards[0].side, original[0].side);
            assert_eq!(
              (cards[0].card.width, cards[0].card.height),
              (original[0].card.width, original[0].card.height)
            );
            assert_eq!(plan.proc.is_some(), procs);
            assert_geometry(area, &plan, &cards, cell, native);
          }
        }
      }
    }
  }

  #[test]
  fn tiny_missing_vm_and_unknown_pixels_keep_the_original_layout() {
    for (area, power, count, cell) in [
      (Rect::new(0, 0, 5, 3), true, 2, (9, 18)),
      (Rect::new(0, 0, 80, 16), true, 2, (9, 18)),
      (Rect::default(), true, 2, (9, 18)),
      (Rect::new(0, 0, 120, 40), true, 0, (9, 18)),
      (Rect::new(0, 0, 120, 40), true, 3, (9, 18)),
      (Rect::new(0, 0, 120, 40), false, 2, (9, 18)),
      (Rect::new(0, 0, 120, 40), true, 2, (0, 18)),
    ] {
      for native in [false, true] {
        let (plan, cards) = layout(area, true, 2, power, count, cell, native);
        assert_eq!(plan, compute_console_layout(area, true, 2, power));
        assert!(cards.is_empty());
      }
    }
  }

  #[test]
  fn unreadable_fans_fall_back_without_hiding_compact_processes() {
    let area = Rect::new(0, 0, 100, 30);
    assert!(compute_console_layout(area, true, 4, true).proc.is_none());
    let compact = compute_layout(area, true, 4, true);
    assert!(compact.proc.is_some());
    for native in [false, true] {
      let (plan, cards) = layout(area, true, 4, true, 2, (1, 1), native);
      assert!(cards.is_empty());
      assert_eq!(plan, compact);
    }
  }
}
