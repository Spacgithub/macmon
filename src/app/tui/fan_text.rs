//! Complete fan images in ANSI-colored Braille cells, using the canonical raster mask.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::theme;

/// Draws the mask made with background=black, foreground=white and accent=cyan, not terminal RGB.
/// `cell_pixels` preserves the native square's physical aspect ratio within the enclosing area.
/// Every owned cell is reset, including padding/invalid input, so previous frames cannot remain.
/// Four by two subcells retain thin housing/ring edges and the full seven-blade silhouette.
/// Reuses the caller's RGBA/buffer storage without allocation, trigonometry or new geometry.
pub(super) fn render(
  buffer: &mut Buffer,
  area: Rect,
  rgba: &[u8],
  side: usize,
  cell_pixels: (u16, u16),
) {
  let visible = area.intersection(buffer.area);
  let valid = (1..=512).contains(&side)
    && side.checked_mul(side).and_then(|n| n.checked_mul(4)) == Some(rgba.len())
    && cell_pixels.0 > 0
    && cell_pixels.1 > 0;
  let (cw, ch) = (i64::from(cell_pixels.0), i64::from(cell_pixels.1));
  let pad_x = i64::from(area.width) * cw - side.min(512) as i64;
  let pad_y = i64::from(area.height) * ch - side.min(512) as i64;
  const DOTS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
  for y in visible.top()..visible.bottom() {
    for x in visible.left()..visible.right() {
      let (mut bits, mut brightest, mut accent) = (0, 0, false);
      if valid {
        for (dy, row) in DOTS.iter().enumerate() {
          for (dx, &bit) in row.iter().enumerate() {
            // Fixed-point centers: quarters of a pixel horizontally, eighths vertically.
            let px = (4 * i64::from(x - area.x) + 2 * dx as i64 + 1) * cw - 2 * pad_x;
            let py = (8 * i64::from(y - area.y) + 2 * dy as i64 + 1) * ch - 4 * pad_y;
            let (light, colored) = sample(rgba, side, px, py, cw, ch);
            if light >= 40 {
              bits |= bit;
              brightest = brightest.max(light);
              accent |= colored;
            }
          }
        }
      }
      let color = if accent {
        theme::SECTION
      } else if brightest >= 72 {
        theme::TEXT
      } else {
        theme::DIM
      };
      let glyph =
        if bits == 0 { ' ' } else { char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' ') };
      buffer[(x, y)].reset();
      buffer[(x, y)].set_char(glyph).set_fg(color);
    }
  }
}

/// Aggregate a subcell's footprint, retaining the alpha silhouette even on the shadowed housing.
fn sample(rgba: &[u8], side: usize, px: i64, py: i64, cw: i64, ch: i64) -> (u16, bool) {
  let limit = side as i64;
  if px < 0 || py < 0 || px >= limit * 4 || py >= limit * 8 {
    return (0, false);
  }
  let left = if px < 2 * cw { 0 } else { (px - cw).div_euclid(4) as usize };
  let right = if px + 2 * cw >= limit * 4 { side } else { (px + cw + 3).div_euclid(4) as usize };
  let top = if py < 2 * ch { 0 } else { (py - ch).div_euclid(8) as usize };
  let bottom = if py + 2 * ch >= limit * 8 { side } else { (py + ch + 7).div_euclid(8) as usize };
  let (mut total, mut count, mut accented, mut outline) = (0_u32, 0_u32, 0_u32, false);
  for y in top..bottom {
    for x in left..right {
      let p = &rgba[(y * side + x) * 4..][..4];
      let light = (54 * u16::from(p[0]) + 183 * u16::from(p[1]) + 19 * u16::from(p[2])) / 256;
      let light = light * u16::from(p[3]) / 255;
      let edge = p[3] >= 128
        && (x == 0
          || x + 1 == side
          || y == 0
          || y + 1 == side
          || rgba[(y * side + x - 1) * 4 + 3] < 128
          || rgba[(y * side + x + 1) * 4 + 3] < 128
          || rgba[((y - 1) * side + x) * 4 + 3] < 128
          || rgba[((y + 1) * side + x) * 4 + 3] < 128);
      total += u32::from(light);
      count += 1;
      outline |= edge;
      accented += u32::from(p[3] >= 128 && i16::from(p[1].min(p[2])) - i16::from(p[0]) > 140);
    }
  }
  let accent = accented > 0 && accented * 8 >= count;
  let light = total.checked_div(count).unwrap_or(0) as u16;
  (if outline || accent { light.max(40) } else { light }, accent)
}

#[cfg(test)]
mod tests {
  use std::f64::consts::TAU;
  use std::hint::black_box;
  use std::time::Instant;

  use ratatui::style::{Color, Modifier};

  use super::super::fan_raster::{FanRaster, Palette};
  use super::*;

  // Canonical geometry mask, independent of the actual terminal's light/dark palette.
  const MASK: Palette = Palette { background: [0; 3], foreground: [255; 3], accent: [0, 255, 255] };

  fn image(side: usize, angle: f64) -> Vec<u8> {
    let raster = FanRaster::new(side).unwrap();
    let mut rgba = vec![0; raster.rgba_len()];
    assert!(raster.render_into(angle, MASK, &mut rgba));
    rgba
  }

  fn dots(buffer: &Buffer, area: Rect, cell: (u16, u16)) -> Vec<(f64, f64)> {
    let mut result = Vec::new();
    for y in area.top()..area.bottom() {
      for x in area.left()..area.right() {
        let symbol = buffer[(x, y)].symbol().chars().next().unwrap();
        let bits = (symbol as u32).saturating_sub(0x2800) as u8;
        for (dy, row) in [[1, 8], [2, 16], [4, 32], [64, 128]].iter().enumerate() {
          for (dx, bit) in row.iter().enumerate() {
            if bits & bit != 0 {
              result.push((
                (f64::from(x - area.x) + (dx as f64 + 0.5) / 2.0) * f64::from(cell.0),
                (f64::from(y - area.y) + (dy as f64 + 0.5) / 4.0) * f64::from(cell.1),
              ));
            }
          }
        }
      }
    }
    result
  }

  #[test]
  fn full_fan_uses_ansi_cells_and_repeated_still_frames_are_identical() {
    let area = Rect::new(0, 0, 14, 7);
    let mut buffer = Buffer::empty(area);
    let rgba = image(112, 0.31);
    render(&mut buffer, area, &rgba, 112, (8, 16));
    let first = buffer.clone();
    assert!(dots(&buffer, area, (8, 16)).len() > 150);
    assert!(buffer.content.iter().any(|c| c.fg == theme::SECTION));
    assert!(buffer.content.iter().any(|c| c.fg == theme::TEXT));
    assert!(buffer.content.iter().any(|c| c.symbol() == " "));
    let mask = |x, y| buffer[(x, y)].symbol().chars().next().unwrap() as u32 - 0x2800;
    assert_eq!(mask(0, 3) & 0x47, 0x47, "left housing edge");
    assert_eq!(mask(13, 3) & 0xb8, 0xb8, "shadowed right housing edge");
    assert_eq!(mask(7, 0) & 0x09, 0x09, "top housing edge");
    assert_eq!(mask(7, 6) & 0xc0, 0xc0, "shadowed bottom housing edge");
    assert!(
      dots(&buffer, area, (8, 16)).len() < 14 * 7 * 8 * 85 / 100,
      "the housing must not become a solid block"
    );
    for cell in &buffer.content {
      assert!(!matches!(cell.fg, Color::Rgb(..)));
      assert_eq!(cell.bg, Color::Reset);
    }
    render(&mut buffer, area, &rgba, 112, (8, 16));
    assert_eq!(buffer, first);
    render(&mut buffer, area, &image(112, 0.75), 112, (8, 16));
    assert_ne!(buffer, first);
    let changed =
      buffer.content.iter().zip(&first.content).filter(|(a, b)| a.symbol() != b.symbol()).count();
    assert!(changed >= 6, "the rotor changes across multiple cells, not one glyph");
  }

  #[test]
  fn physical_cell_ratios_keep_centered_square_and_complete_outer_silhouette() {
    for (width, height, cw, ch, side) in
      [(8, 4, 8, 16, 64), (14, 7, 8, 20, 112), (14, 7, 10, 16, 112), (14, 7, 14, 28, 120)]
    {
      let area = Rect::new(0, 0, width, height);
      let mut buffer = Buffer::empty(area);
      render(&mut buffer, area, &image(side, 0.0), side, (cw, ch));
      let points = dots(&buffer, area, (cw, ch));
      assert!(points.len() > 40);
      let (min_x, max_x, min_y, max_y) = points.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY),
        |(min_x, max_x, min_y, max_y), &(x, y)| {
          (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
        },
      );
      let span_x = max_x - min_x + f64::from(cw) / 2.0;
      let span_y = max_y - min_y + f64::from(ch) / 4.0;
      assert!((span_x - span_y).abs() <= f64::from(cw) / 2.0 + f64::from(ch) / 4.0);
      assert!(span_x >= side as f64 * 0.85 && span_y >= side as f64 * 0.85);
      assert!(((min_x + max_x) / 2.0 - f64::from(width * cw) / 2.0).abs() <= f64::from(cw) / 2.0);
      assert!(((min_y + max_y) / 2.0 - f64::from(height * ch) / 2.0).abs() <= f64::from(ch) / 4.0);
    }
  }

  #[test]
  fn clipping_and_invalid_images_clear_only_the_owned_cells() {
    let canvas = Rect::new(3, 5, 20, 10);
    let mut original = Buffer::empty(canvas);
    for cell in &mut original.content {
      cell.set_char('x').set_fg(Color::Magenta).set_bg(Color::Yellow);
      cell.modifier = Modifier::BOLD;
    }
    for (area, side, rgba, cell) in [
      (Rect::new(1, 7, 10, 6), 64, image(64, 0.0), (8, 16)),
      (Rect::new(7, 8, 8, 4), 0, vec![], (8, 16)),
      (Rect::new(7, 8, 8, 4), 64, vec![0; 15], (8, 16)),
      (Rect::new(7, 8, 8, 4), 64, image(64, 0.0), (0, 16)),
      (Rect::new(u16::MAX, u16::MAX, 0, 0), usize::MAX, vec![], (u16::MAX, u16::MAX)),
    ] {
      let mut buffer = original.clone();
      render(&mut buffer, area, &rgba, side, cell);
      let visible = area.intersection(canvas);
      for y in canvas.top()..canvas.bottom() {
        for x in canvas.left()..canvas.right() {
          if visible.contains((x, y).into()) {
            assert_eq!(buffer[(x, y)].bg, Color::Reset);
            assert_eq!(buffer[(x, y)].modifier, Modifier::empty());
          } else {
            assert_eq!(buffer[(x, y)], original[(x, y)]);
          }
        }
      }
    }
    let area = Rect::new(7, 8, 8, 4);
    let mut buffer = original;
    render(&mut buffer, area, &image(64, 0.0), 64, (8, 16));
    render(&mut buffer, area, &[0; 64 * 64 * 4], 64, (8, 16));
    for y in area.top()..area.bottom() {
      for x in area.left()..area.right() {
        assert_eq!(buffer[(x, y)].symbol(), " ");
      }
    }
  }

  #[test]
  #[ignore = "explicit raster-to-cell CPU benchmark, not terminal presentation FPS"]
  fn benchmark_1200_braille_frames() {
    let area = Rect::new(0, 0, 14, 7);
    let raster = FanRaster::new(112).unwrap();
    let mut rgba = vec![0; raster.rgba_len()];
    let mut buffer = Buffer::empty(area);
    assert!(raster.render_into(0.0, MASK, &mut rgba));
    let start = Instant::now();
    for _ in 0..1200 {
      render(black_box(&mut buffer), area, &rgba, 112, (8, 16));
    }
    println!(
      "Braille conversion only: 1200 frames, {:.3}us/frame",
      start.elapsed().as_secs_f64() * 1e6 / 1200.0
    );
    assert!(raster.render_into(TAU / 7.0, MASK, &mut rgba));
  }
}
