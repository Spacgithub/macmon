//! Pure CPU fan rasterizer; no terminal, hardware, configuration or image-codec access.

use std::f64::consts::TAU;

/// Opaque sRGB colors supplied by the caller. Output is straight-alpha RGBA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Palette {
  pub(super) background: [u8; 3],
  pub(super) foreground: [u8; 3],
  pub(super) accent: [u8; 3],
}

#[derive(Debug)]
struct Sample {
  radius: f32,
  sector: f32,
  half_width: f32,
  light: f32,
}

/// Square image with a stationary rim/hub and seven curved rotor blades.
#[derive(Debug)]
pub(super) struct FanRaster {
  size: usize,
  samples: Vec<Sample>,
}

impl FanRaster {
  /// Precomputes four geometry samples per pixel. Sizes 1..=512 are bounded and supported.
  pub(super) fn new(size: usize) -> Option<Self> {
    if !(1..=512).contains(&size) {
      return None;
    }
    let mut samples = Vec::new();
    samples.try_reserve_exact(size * size * 4).ok()?;
    let half = size as f32 / 2.0;
    let spacing = (TAU / 7.0) as f32;
    for y in 0..size {
      for x in 0..size {
        for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
          let dx = (x as f32 + sx - half) / half;
          let dy = (y as f32 + sy - half) / half;
          let radius = dx.hypot(dy);
          let t = ((radius - 0.22) / 0.60).clamp(0.0, 1.0);
          let bend = 0.65 * t + 0.20 * t * t;
          let sector = ((dy.atan2(dx) - bend) / spacing + 0.5).rem_euclid(7.0);
          let half_width = (0.08 + 0.80 * t * (1.0 - t)) / spacing;
          let light = (0.55 - (0.25 * dx + 0.35 * dy) / radius.max(0.22)).clamp(0.0, 1.0);
          samples.push(Sample { radius, sector, half_width, light });
        }
      }
    }
    Some(Self { size, samples })
  }

  pub(super) fn size(&self) -> usize {
    self.size
  }

  pub(super) fn rgba_len(&self) -> usize {
    self.size * self.size * 4
  }

  /// Renders a continuous angle in radians. Positive angles turn clockwise in image coordinates.
  /// The highlighted first blade turns with the rotor; the rim and center shaft do not turn.
  /// Requires an exact-size output buffer and a finite angle; otherwise leaves it untouched.
  /// Reuses cached geometry and caller storage, with no allocation or trigonometry per frame.
  pub(super) fn render_into(&self, angle: f64, palette: Palette, rgba: &mut [u8]) -> bool {
    if rgba.len() != self.rgba_len() || !angle.is_finite() {
      return false;
    }
    let phase = (angle.rem_euclid(TAU) * 7.0 / TAU) as f32;
    let base = palette.background;
    let surface = palette.foreground;
    let (pixels, _) = rgba.as_chunks_mut::<4>();
    let (geometry, _) = self.samples.as_chunks::<4>();
    for (pixel, samples) in pixels.iter_mut().zip(geometry.iter()) {
      let (mut sums, mut covered) = ([0u16; 3], 0u16);
      for sample in samples {
        let r = sample.radius;
        if r > 0.96 {
          continue;
        }
        let mut color = base;
        if r >= 0.90 {
          let bevel = 1.0 - ((r - 0.93) / 0.03).abs();
          color = mix(base, surface, 0.16 + 0.64 * bevel + 0.20 * sample.light);
        } else if r <= 0.19 {
          color = mix(base, surface, 0.20 + 0.80 * sample.light);
        } else if r <= 0.22 {
          color = mix(base, palette.accent, 0.20 + 0.75 * sample.light);
        } else if r < 0.82 {
          let mut sector = sample.sector - phase;
          if sector < 0.0 {
            sector += 7.0;
          }
          if sector >= 7.0 {
            sector -= 7.0;
          }
          let blade = sector as u8;
          let offset = (sector - f32::from(blade) - 0.5).abs();
          if offset <= sample.half_width {
            let metal = if blade == 0 { palette.accent } else { surface };
            let ridge = 1.0 - offset / sample.half_width;
            color = mix(base, metal, 0.20 + 0.55 * ridge + 0.25 * sample.light);
          }
        }
        for (sum, channel) in sums.iter_mut().zip(color) {
          *sum += u16::from(channel);
        }
        covered += 1;
      }
      // Divide color by covered samples, not by four: alpha is not premultiplied.
      for (channel, sum) in pixel[..3].iter_mut().zip(sums) {
        *channel = sum.checked_div(covered).unwrap_or(0) as u8;
      }
      pixel[3] = (covered * 255 / 4) as u8;
    }
    true
  }
}

/// Fixed-point color mixing keeps every metal shade within the caller's colors.
fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
  let w = (t.clamp(0.0, 1.0) * 256.0) as u16;
  std::array::from_fn(|i| ((u16::from(a[i]) * (256 - w) + u16::from(b[i]) * w) / 256) as u8)
}

#[cfg(test)]
mod tests {
  use std::hint::black_box;
  use std::time::Instant;

  use super::{FanRaster, Palette, TAU};

  // Synthetic colors for tests, unrelated to any terminal palette or user setting.
  const PALETTE: Palette =
    Palette { background: [12, 24, 36], foreground: [80, 140, 200], accent: [240, 170, 70] };

  fn render(raster: &FanRaster, angle: f64, palette: Palette) -> Vec<u8> {
    let mut rgba = vec![0; raster.rgba_len()];
    assert!(raster.render_into(angle, palette, &mut rgba));
    rgba
  }

  fn point(raster: &FanRaster, rgba: &[u8], radius: f64, angle: f64) -> [u8; 4] {
    let half = raster.size() as f64 / 2.0;
    let x = (half + half * radius * angle.cos()).floor() as usize;
    let y = (half + half * radius * angle.sin()).floor() as usize;
    rgba[(y * raster.size() + x) * 4..][..4].try_into().unwrap()
  }

  #[test]
  fn sizes_buffers_and_angles_are_checked_without_touching_invalid_output() {
    assert!(FanRaster::new(0).is_none());
    assert!(FanRaster::new(513).is_none());
    for size in [200, 240] {
      let raster = FanRaster::new(size).unwrap();
      assert_eq!(raster.size(), size);
      assert_eq!(raster.rgba_len(), size * size * 4);
      let mut rgba = vec![77; raster.rgba_len()];
      for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!raster.render_into(angle, PALETTE, &mut rgba));
        assert!(rgba.iter().all(|&byte| byte == 77));
      }
      let mut short = vec![77; raster.rgba_len() - 1];
      assert!(!raster.render_into(0.0, PALETTE, &mut short));
      assert!(short.iter().all(|&byte| byte == 77));
      assert_eq!(render(&raster, -0.125, PALETTE), render(&raster, TAU - 0.125, PALETTE));
      assert_eq!(render(&raster, 0.0, PALETTE), render(&raster, TAU, PALETTE));
    }
  }

  #[test]
  fn exterior_is_transparent_and_antialiased_edges_have_straight_alpha() {
    let raster = FanRaster::new(240).unwrap();
    let mono = Palette { background: PALETTE.foreground, accent: PALETTE.foreground, ..PALETTE };
    let rgba = render(&raster, 0.0, mono);
    for (x, y) in [(0, 0), (239, 0), (0, 239), (239, 239)] {
      assert_eq!(&rgba[(y * 240 + x) * 4..][..4], &[0, 0, 0, 0]);
    }
    let mut edges = 0;
    for pixel in rgba.as_chunks::<4>().0.iter() {
      if pixel[3] == 0 {
        assert_eq!(*pixel, [0, 0, 0, 0]);
      } else if pixel[3] < 255 {
        edges += 1;
        assert_eq!(pixel[..3], PALETTE.foreground);
        assert!([63, 127, 191].contains(&pixel[3]));
      }
    }
    assert!(edges > 100);
    assert_eq!(&rgba[(120 * 240 + 120) * 4..][..4], &[80, 140, 200, 255]);
  }

  #[test]
  fn circular_rim_and_center_shaft_stay_fixed_while_the_rotor_changes() {
    let raster = FanRaster::new(240).unwrap();
    let first = render(&raster, 0.0, PALETTE);
    for angle in [0.01, 0.31, 0.75, TAU / 7.0] {
      let next = render(&raster, angle, PALETTE);
      assert_ne!(first, next);
      for (i, samples) in raster.samples.as_chunks::<4>().0.iter().enumerate() {
        if samples.iter().all(|sample| sample.radius <= 0.22 || sample.radius >= 0.82) {
          assert_eq!(&first[i * 4..][..4], &next[i * 4..][..4]);
        }
      }
    }
  }

  #[test]
  fn seven_curved_blades_repeat_geometrically_but_the_highlight_rotates_a_full_turn() {
    let raster = FanRaster::new(240).unwrap();
    let radius = 0.55;
    let t = (radius - 0.22) / 0.60;
    let bend = 0.65 * t + 0.20 * t * t;
    let first = render(&raster, 0.0, PALETTE);
    let turned = render(&raster, TAU / 7.0, PALETTE);
    for blade in 0..7 {
      let center = bend + blade as f64 * TAU / 7.0;
      let pixel = point(&raster, &first, radius, center);
      assert_eq!(pixel[0] > pixel[2], blade == 0, "highlighted blade {blade}");
      assert_eq!(point(&raster, &first, radius, center + TAU / 14.0)[..3], PALETTE.background);
    }
    let old = point(&raster, &turned, radius, bend);
    let marked = point(&raster, &turned, radius, bend + TAU / 7.0);
    assert!(old[2] > old[0]);
    assert!(marked[0] > marked[2]);
    assert_ne!(first, turned);
    let unmarked = Palette { accent: PALETTE.foreground, ..PALETTE };
    let a = render(&raster, 0.0, unmarked);
    let b = render(&raster, TAU / 7.0, unmarked);
    let changed = a
      .as_chunks::<4>()
      .0
      .iter()
      .zip(b.as_chunks::<4>().0.iter())
      .filter(|(a, b)| (a[..3] != PALETTE.background) != (b[..3] != PALETTE.background))
      .count();
    assert!(changed <= 8, "seven-fold geometry differs at {changed} pixels");
    // Bending changes the angular center with radius instead of making straight spokes.
    let outer_t = (0.76 - 0.22) / 0.60;
    let outer_center = 0.65 * outer_t + 0.20 * outer_t * outer_t;
    let marked = point(&raster, &first, 0.76, outer_center);
    assert!(marked[0] > marked[2]);
    assert_eq!(point(&raster, &first, 0.76, bend)[..3], PALETTE.background);
  }

  #[test]
  fn all_visible_colors_come_from_the_supplied_palette() {
    let raster = FanRaster::new(200).unwrap();
    let palette = Palette { background: [32; 3], foreground: [32; 3], accent: [32; 3] };
    let rgba = render(&raster, 0.4, palette);
    for pixel in rgba.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] != 0) {
      assert_eq!(pixel[..3], [32; 3]);
    }
    let first = render(&raster, 0.4, PALETTE);
    assert_eq!(
      first.as_chunks::<4>().0.iter().map(|p| p[3]).collect::<Vec<_>>(),
      rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect::<Vec<_>>()
    );
    assert_ne!(first, rgba);
  }

  #[test]
  fn metal_has_spatial_light_beveled_rim_and_a_highlight_along_each_curved_blade() {
    let raster = FanRaster::new(240).unwrap();
    let rgba = render(&raster, 0.0, PALETTE);
    let above = point(&raster, &rgba, 0.12, -3.0 * TAU / 8.0);
    let below = point(&raster, &rgba, 0.12, TAU / 8.0);
    assert!(above[0] > below[0] && above[2] > below[2], "hub light: {above:?}, {below:?}");
    let ridge = point(&raster, &rgba, 0.93, -TAU / 8.0);
    let inner = point(&raster, &rgba, 0.91, -TAU / 8.0);
    let outer = point(&raster, &rgba, 0.95, -TAU / 8.0);
    assert!(
      ridge[2] > inner[2] && ridge[2] > outer[2],
      "rim bevel: {inner:?}, {ridge:?}, {outer:?}"
    );
    let t = (0.55 - 0.22) / 0.60;
    let bend = 0.65 * t + 0.20 * t * t;
    let center = point(&raster, &rgba, 0.55, bend);
    let edge = point(&raster, &rgba, 0.55, bend + 0.19);
    assert!(center[0] > edge[0], "blade ridge: {center:?}, {edge:?}");
    for pixel in rgba.as_chunks::<4>().0.iter().filter(|p| p[3] != 0) {
      for i in 0..3 {
        let low = PALETTE.background[i].min(PALETTE.foreground[i]).min(PALETTE.accent[i]);
        let high = PALETTE.background[i].max(PALETTE.foreground[i]).max(PALETTE.accent[i]);
        assert!((low..=high).contains(&pixel[i]));
      }
    }
  }

  #[test]
  #[ignore = "explicit CPU-only raster benchmark, not terminal presentation FPS"]
  fn benchmark_240px_120_frames() {
    let setup = Instant::now();
    let raster = FanRaster::new(240).unwrap();
    let mut rgba = vec![0; raster.rgba_len()];
    let setup_ms = setup.elapsed().as_secs_f64() * 1000.0;
    for _ in 0..3 {
      assert!(raster.render_into(0.0, PALETTE, &mut rgba));
    }
    let start = Instant::now();
    for frame in 0..120 {
      let angle = black_box(frame as f64 * TAU / 120.0);
      assert!(raster.render_into(angle, PALETTE, &mut rgba));
      black_box(&rgba);
    }
    let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
    println!(
      "CPU raster only: 240x240, 120 frames, setup={setup_ms:.3}ms, total={milliseconds:.3}ms, mean={:.3}ms/frame, geometry={} bytes, RGBA={} bytes",
      milliseconds / 120.0,
      raster.samples.len() * std::mem::size_of::<super::Sample>(),
      rgba.len()
    );
  }
}
