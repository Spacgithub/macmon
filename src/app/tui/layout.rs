//! Adaptive metrics above the process list: a grouped wide console or the compact metric grid.
//! Both keep a full-width network table when it fits; VMs omit unavailable power sources.

use ratatui::layout::{Margin, Rect};

/// Share of the screen height for the metrics box over the process list, in percent.
const METRICS_HEIGHT_PCT: u32 = 40;
/// Two graph rows, a compact network table and borders.
const METRICS_MIN_HEIGHT: u16 = 14;
/// Fewest process rows worth showing; with less room the process list is auto-hidden.
const PROC_MIN_ROWS: u16 = 3;
/// Rows of the process box besides the process rows: borders and the table header.
const PROC_CHROME: u16 = 3;

/// One box of the metrics box: a title over a history graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Metric {
  /// CPU cluster, by index into the cluster list.
  Cluster(usize),
  Gpu,
  Ram,
  CpuPower,
  GpuPower,
  AnePower,
  /// System power, or CPU + GPU + ANE without the system power sensor.
  TotalPower,
}

/// Rectangles for one frame. Hidden parts are `None` / left out; every rect is non-empty.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct LayoutPlan {
  /// Metrics box, borders included.
  pub(super) top: Option<Rect>,
  /// Metric boxes inside the metrics box, borders included: the top row left to right, then the
  /// bottom row.
  pub(super) boxes: Vec<(Metric, Rect)>,
  /// Wide console's compute group and power/temperature ledger; absent in compact/VM layouts.
  pub(super) compute: Option<Rect>,
  pub(super) telemetry: Option<Rect>,
  /// Full-width interface table, when the metric graphs also have room.
  pub(super) network: Option<Rect>,
  /// Process box, borders included.
  pub(super) proc: Option<Rect>,
}

/// Boxes of the top row: CPU clusters, GPU, RAM.
fn top_row(clusters: usize) -> Vec<Metric> {
  let mut boxes: Vec<Metric> = (0..clusters).map(Metric::Cluster).collect();
  boxes.extend([Metric::Gpu, Metric::Ram]);
  boxes
}

/// Boxes of the bottom row: CPU, GPU, ANE and total power.
const BOTTOM_ROW: [Metric; 4] =
  [Metric::CpuPower, Metric::GpuPower, Metric::AnePower, Metric::TotalPower];

/// Height of the metrics box over the process list on a screen `height` rows tall:
/// `METRICS_HEIGHT_PCT` of it, rounded, but at least `METRICS_MIN_HEIGHT`.
fn metrics_height(height: u16) -> u16 {
  // a share of `height`, so it fits in u16
  let share = ((u32::from(height) * METRICS_HEIGHT_PCT + 50) / 100) as u16;
  share.max(METRICS_MIN_HEIGHT).min(height)
}

/// `count` (at least one) boxes side by side splitting the width of `row` evenly; the odd cells
/// are spread between them. Boxes without cells are left out.
fn split_row(row: Rect, count: usize) -> impl Iterator<Item = Rect> {
  let (width, count) = (u64::from(row.width), count as u64);
  // offsets stay within `row.width`, so they fit in u16
  let edge = move |i: u64| (width * i / count) as u16;
  (0..count)
    .map(move |i| Rect { x: row.x + edge(i), width: edge(i + 1) - edge(i), ..row })
    .filter(|r| !r.is_empty())
}

/// Whether `area` has room for the process list under the metrics box: at least `PROC_MIN_ROWS`
/// process rows. The process list setting doesn't matter.
pub(super) fn procs_fit(area: Rect) -> bool {
  !area.is_empty() && area.height - metrics_height(area.height) >= PROC_MIN_ROWS + PROC_CHROME
}

/// Splits `area` into the metrics box and the process box below it.
///
/// With the process list on (`procs`), the metrics box takes `METRICS_HEIGHT_PCT` of the height
/// and the process box the rest; left with fewer than `PROC_MIN_ROWS` process rows, the process
/// list is auto-hidden. Without it, the metrics box takes the whole screen. Without `power`, the
/// top row of boxes takes the graph area above the network table.
pub(super) fn compute_layout(area: Rect, procs: bool, clusters: usize, power: bool) -> LayoutPlan {
  let mut plan = LayoutPlan::default();
  if area.is_empty() {
    return plan;
  }

  let mut top = area;
  if procs && procs_fit(area) {
    top.height = metrics_height(area.height);
    plan.proc = Some(Rect { y: top.bottom(), height: area.height - top.height, ..area });
  }
  plan.top = Some(top);

  let mut inner = top.inner(Margin::new(1, 1));
  if !inner.is_empty() && inner.height >= 10 {
    let height = (inner.height - 6).min(6);
    plan.network = Some(Rect { y: inner.bottom() - height, height, ..inner });
    inner.height -= height;
  }
  let upper = if power { inner.height.div_ceil(2) } else { inner.height };
  let rows = [
    (Rect { height: upper, ..inner }, top_row(clusters)),
    (Rect { y: inner.y + upper, height: inner.height - upper, ..inner }, BOTTOM_ROW.to_vec()),
  ];
  for (row, metrics) in rows {
    if row.is_empty() {
      continue;
    }
    let count = metrics.len();
    plan.boxes.extend(metrics.into_iter().zip(split_row(row, count)));
  }

  plan
}

/// The selected console on wide, tall windows; compact windows and VMs use the original grid.
pub(super) fn compute_console_layout(
  area: Rect,
  procs: bool,
  clusters: usize,
  power: bool,
) -> LayoutPlan {
  let count = clusters.saturating_add(1);
  let minimum = u16::try_from(count.saturating_mul(3).saturating_add(2).max(12).saturating_add(8))
    .unwrap_or(u16::MAX);
  if !power || area.width < 100 || area.height < 30 || area.height < minimum {
    return compute_layout(area, procs, clusters, power);
  }
  let mut top = area;
  top.height = metrics_height(area.height).max(minimum);
  let proc = (procs && area.height - top.height >= PROC_MIN_ROWS + PROC_CHROME).then_some(Rect {
    y: top.bottom(),
    height: area.height - top.height,
    ..area
  });
  if proc.is_none() {
    top = area;
  }
  let mut inner = top.inner(Margin::new(1, 1));
  let network = Rect { y: inner.bottom() - 6, height: 6, ..inner };
  inner.height -= 6;
  let compute = Rect { width: (u32::from(inner.width) * 3 / 5) as u16, ..inner };
  let right = Rect { x: compute.right() + 1, width: inner.width - compute.width - 1, ..inner };
  let memory = Rect { height: 4, ..right };
  let telemetry = Rect { y: right.y + 4, height: right.height - 4, ..right };
  let body = compute.inner(Margin::new(1, 1));
  let edge = |i: usize| (u64::from(body.height) * i as u64 / count as u64) as u16;
  let mut boxes: Vec<_> = (0..clusters)
    .map(Metric::Cluster)
    .chain([Metric::Gpu])
    .enumerate()
    .map(|(i, metric)| {
      (metric, Rect { y: body.y + edge(i), height: edge(i + 1) - edge(i), ..body })
    })
    .collect();
  boxes.push((Metric::Ram, memory));
  LayoutPlan {
    top: Some(top),
    boxes,
    compute: Some(compute),
    telemetry: Some(telemetry),
    network: Some(network),
    proc,
  }
}

#[cfg(test)]
mod tests {
  use ratatui::layout::{Margin, Rect};

  use super::{
    LayoutPlan, PROC_CHROME, PROC_MIN_ROWS, compute_console_layout, compute_layout, procs_fit,
  };

  #[test]
  fn console_thresholds_and_cpu_tier_counts_keep_room_for_their_content() {
    for (width, height, clusters, power, console, procs) in [
      (99, 30, 2, true, false, true),
      (100, 29, 2, true, false, true),
      (100, 30, 2, false, false, true),
      (100, 30, 0, true, true, true),
      (100, 30, 2, true, true, true),
      (100, 30, 3, true, true, true),
      (100, 30, 4, true, true, false),
      (100, 30, 6, true, false, true),
    ] {
      let area = Rect::new(3, 2, width, height);
      let plan = compute_console_layout(area, true, clusters, power);
      let ctx = format!("{area:?}, clusters {clusters}, power {power}");
      assert_eq!(plan.compute.is_some(), console, "{ctx}");
      assert_eq!(plan.proc.is_some(), procs, "{ctx}");
      if let Some(compute) = plan.compute {
        assert_eq!(plan.boxes.len(), clusters + 2, "{ctx}");
        let body = compute.inner(Margin::new(1, 1));
        for (_, r) in plan.boxes.iter().take(clusters + 1) {
          assert!(r.height >= 3 && body.intersection(*r) == *r, "{ctx}: {r:?}");
        }
        assert_eq!(plan.boxes.last().unwrap().1.height, 4);
        assert!(plan.telemetry.unwrap().height >= 8);
        assert_eq!(plan.network.unwrap().height, 6);
      } else {
        assert_eq!(plan, compute_layout(area, true, clusters, power), "{ctx}");
      }
    }
  }

  #[test]
  fn console_siblings_stay_inside_and_do_not_overlap() {
    for area in
      [Rect::new(3, 2, 100, 30), Rect::new(7, 5, 400, 120), Rect::new(0, 0, u16::MAX, u16::MAX)]
    {
      for (clusters, procs) in [(0, true), (2, true), (3, false), (6, true)] {
        let plan = compute_console_layout(area, procs, clusters, true);
        let ctx = format!("{area:?}, clusters {clusters}, procs {procs}");
        let top = plan.top.unwrap();
        assert_eq!(area.intersection(top), top, "{ctx}");
        if let Some(proc) = plan.proc {
          assert_eq!(proc.bottom(), area.bottom());
          assert_eq!(proc.y, top.bottom());
          assert!(proc.height >= PROC_MIN_ROWS + PROC_CHROME);
        } else {
          assert_eq!(top, area);
        }
        let inner = top.inner(Margin::new(1, 1));
        let siblings: Vec<_> = if let Some(compute) = plan.compute {
          vec![
            compute,
            plan.boxes.last().unwrap().1,
            plan.telemetry.unwrap(),
            plan.network.unwrap(),
          ]
        } else {
          plan.boxes.iter().map(|(_, r)| *r).chain(plan.network).collect()
        };
        for (i, a) in siblings.iter().enumerate() {
          assert!(!a.is_empty() && inner.intersection(*a) == *a, "{ctx}: {a:?}");
          assert!(siblings[i + 1..].iter().all(|b| !a.intersects(*b)), "{ctx}: {a:?}");
        }
      }
    }
  }

  #[test]
  fn metrics_take_40_percent_and_procs_the_rest() {
    // (width, height, metrics box height): 40 % of the height, rounded, but at least two rows of
    // boxes with a graph row each
    for (width, height, top) in [(200, 50, 20), (110, 32, 14), (80, 24, 14), (100, 20, 14)] {
      let plan = compute_layout(Rect::new(0, 0, width, height), true, 2, true);
      let ctx = format!("{width}x{height}");
      assert_eq!(plan.top, Some(Rect::new(0, 0, width, top)), "{ctx}");
      assert_eq!(plan.proc, Some(Rect::new(0, top, width, height - top)), "{ctx}");
    }
  }

  #[test]
  fn hidden_or_auto_hidden_list_gives_metrics_the_full_height() {
    let area = Rect::new(0, 0, 120, 40);
    let plan = compute_layout(area, false, 2, true);
    assert_eq!((plan.top, plan.proc), (Some(area), None));
    // 38 rows inside, six for the network table and 16 for each row of graphs.
    assert!(plan.boxes.iter().all(|(_, r)| r.height == 16), "{plan:?}");

    // A 14-row metrics box would leave five rows, one short of the smallest process box.
    let small = Rect::new(0, 0, 100, 19);
    let plan = compute_layout(small, true, 2, true);
    assert_eq!((plan.top, plan.proc), (Some(small), None));
    assert!(!procs_fit(small) && procs_fit(Rect { height: 20, ..small }));
  }

  #[test]
  fn without_power_the_top_row_takes_the_power_row_height() {
    use super::Metric::*;
    // 200x50 with the process list: 18 inner rows, six reserved for the network table.
    let area = Rect::new(0, 0, 200, 50);
    let with = compute_layout(area, true, 1, true);
    let rows =
      |plan: &LayoutPlan| plan.boxes.iter().map(|(m, r)| (*m, r.y, r.height)).collect::<Vec<_>>();
    assert_eq!(
      rows(&with),
      [
        (Cluster(0), 1, 6),
        (Gpu, 1, 6),
        (Ram, 1, 6),
        (CpuPower, 7, 6),
        (GpuPower, 7, 6),
        (AnePower, 7, 6),
        (TotalPower, 7, 6)
      ]
    );

    // the same metrics and process boxes, one row of boxes
    let without = compute_layout(area, true, 1, false);
    assert_eq!((without.top, without.proc), (with.top, with.proc));
    assert_eq!(rows(&without), [(Cluster(0), 1, 12), (Gpu, 1, 12), (Ram, 1, 12)]);
    assert_eq!(without.network, with.network);
    let widths = |plan: &LayoutPlan| {
      plan.boxes.iter().map(|(_, r)| (r.x, r.width)).take(3).collect::<Vec<_>>()
    };
    assert_eq!(widths(&without), widths(&with));
  }

  #[test]
  fn boxes_stay_inside_and_never_overlap() {
    let widths = [1, 2, 3, 5, 10, 69, 80, 110, 200, 400];
    let heights = [1, 2, 3, 5, 8, 13, 14, 24, 50, 120];
    for (width, height) in widths.into_iter().flat_map(|w| heights.map(|h| (w, h))) {
      let area = Rect::new(3, 2, width, height);
      let cases = [(0, true, true), (2, false, true), (2, true, true), (3, true, true)];
      for (clusters, procs, power) in cases.into_iter().chain([(1, true, false), (2, false, false)])
      {
        let plan = compute_layout(area, procs, clusters, power);
        let ctx = format!("{area:?} clusters={clusters} procs={procs} power={power}");

        // the metrics box on top, the process box right under it to the bottom
        let top = plan.top.expect("metrics box");
        assert_eq!(plan.proc.is_some(), procs && procs_fit(area), "{ctx}");
        match plan.proc {
          Some(proc) => {
            let rest = Rect { y: top.bottom(), height: area.bottom() - top.bottom(), ..area };
            assert_eq!(proc, rest, "{ctx}");
            assert!(proc.height >= PROC_MIN_ROWS + PROC_CHROME, "{ctx}");
          }
          None => assert_eq!(top, area, "{ctx}: metrics take the full height"),
        }

        // boxes sit inside the borders of the metrics box and never overlap
        let inner = top.inner(Margin::new(1, 1));
        if let Some(network) = plan.network {
          assert!(!network.is_empty(), "{ctx}");
          assert_eq!(inner.intersection(network), network, "{ctx}");
          assert!(plan.boxes.iter().all(|(_, r)| !r.intersects(network)), "{ctx}");
        }
        for (i, (_, a)) in plan.boxes.iter().enumerate() {
          assert!(!a.is_empty() && inner.intersection(*a) == *a, "{a:?} out of {inner:?}: {ctx}");
          for (_, b) in &plan.boxes[i + 1..] {
            assert!(!a.intersects(*b), "{a:?} overlaps {b:?} in {ctx}");
          }
        }
      }
    }
  }
}
