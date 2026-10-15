//! Pure CPU fan rasterizer; no terminal, hardware, configuration or image-codec access.

use std::f64::consts::TAU;
use std::sync::Mutex;

/// Opaque sRGB colors supplied by the caller. Output is straight-alpha RGBA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Palette {
  pub(super) background: [u8; 3],
  pub(super) foreground: [u8; 3],
  pub(super) accent: [u8; 3],
}

const OUTSIDE: u8 = 0;
const SHELL: u8 = 1;
const WELL: u8 = 2;
const RING: u8 = 3;
const HUB: u8 = 4;
const SCREW: u8 = 5;
const ROTOR: u8 = 6;

#[derive(Clone, Copy, Debug)]
struct Sample {
  layer: u8,
  tone: u8,
  light: u8,
  sector: f32,
  span: f32,
  gradient: [f32; 2],
}

#[derive(Debug)]
struct Materials {
  palette: Option<Palette>,
  fixed: Vec<[u8; 4]>,
  surfaces: [[[u8; 3]; 256]; 6],
  blades: [[[[u8; 3]; 256]; 16]; 2],
}

/// Rounded mechanical housing, four recessed screws, a fixed ring/shaft and seven swept blades.
#[derive(Debug)]
pub(super) struct FanRaster {
  size: usize,
  samples: Vec<[Sample; 4]>,
  rotating: Vec<usize>,
  gradient: [(f32, f32); 1024],
  materials: Mutex<Materials>,
}

impl FanRaster {
  /// Precomputes four geometry samples per pixel. Sizes 1..=512 are bounded and supported.
  pub(super) fn new(size: usize) -> Option<Self> {
    if !(1..=512).contains(&size) {
      return None;
    }
    let mut samples = Vec::new();
    let mut rotating = Vec::new();
    let mut fixed = Vec::new();
    samples.try_reserve_exact(size * size).ok()?;
    rotating.try_reserve_exact(size * size).ok()?;
    fixed.try_reserve_exact(size * size).ok()?;
    fixed.resize(size * size, [0; 4]);
    let profile: [(f32, f32); 2048] =
      std::array::from_fn(|i| blade_profile((i as f32 + 0.5) / 2048.0));
    let half = size as f32 / 2.0;
    for y in 0..size {
      for x in 0..size {
        let pixel = std::array::from_fn(|i| {
          let dx = (x as f32 + 0.25 + (i % 2) as f32 * 0.5 - half) / half;
          let dy = (y as f32 + 0.25 + (i / 2) as f32 * 0.5 - half) / half;
          geometry(dx, dy, &profile)
        });
        if pixel.iter().any(|s: &Sample| s.layer == ROTOR) {
          rotating.push(samples.len());
        }
        samples.push(pixel);
      }
    }
    let materials = Materials {
      palette: None,
      fixed,
      surfaces: [[[0; 3]; 256]; 6],
      blades: [[[[0; 3]; 256]; 16]; 2],
    };
    let gradient = std::array::from_fn(|i| {
      let angle = i as f32 / 1023.0 * TAU as f32 / 7.0;
      (angle.cos(), angle.sin())
    });
    Some(Self { size, samples, rotating, gradient, materials: Mutex::new(materials) })
  }

  pub(super) fn size(&self) -> usize {
    self.size
  }

  pub(super) fn rgba_len(&self) -> usize {
    self.size * self.size * 4
  }

  #[cfg(test)]
  pub(super) fn geometry_ptr(&self) -> *const () {
    self.samples.as_ptr().cast()
  }

  /// Renders radians clockwise, preserving the first blade's rotating accent and fixed hardware.
  /// Invalid buffers/angles are untouched. Geometry, materials and output storage are reused;
  /// after construction this performs no allocation or trigonometry, including palette changes.
  pub(super) fn render_into(&self, angle: f64, palette: Palette, rgba: &mut [u8]) -> bool {
    if rgba.len() != self.rgba_len() || !angle.is_finite() {
      return false;
    }
    let phase = (angle.rem_euclid(TAU) * 7.0 / TAU) as f32;
    let mut materials = self.materials.lock().unwrap_or_else(|err| err.into_inner());
    if materials.palette != Some(palette) {
      materials.recolor(palette, &self.samples);
    }
    rgba.copy_from_slice(materials.fixed.as_flattened());
    let (pixels, _) = rgba.as_chunks_mut::<4>();
    for &index in &self.rotating {
      let mut colors = [[0; 3]; 4];
      let mut covered = 0;
      for (color, sample) in colors.iter_mut().zip(&self.samples[index]) {
        if sample.layer == OUTSIDE {
          continue;
        }
        let surface = if sample.layer == ROTOR { WELL } else { sample.layer };
        *color = materials.surfaces[usize::from(surface)][usize::from(sample.tone)];
        if sample.layer == ROTOR {
          let mut sector = sample.sector - phase;
          if sector < 0.0 {
            sector += 7.0;
          }
          if sector >= 7.0 {
            sector -= 7.0;
          }
          let blade = sector as usize;
          let offset = sector - blade as f32;
          if offset < sample.span {
            let (cos, sin) = self.gradient[(offset * 1023.0) as usize];
            let tone = usize::from(byte(0.5 + sample.gradient[0] * cos + sample.gradient[1] * sin));
            *color = materials.blades[usize::from(blade == 0)][usize::from(sample.light)][tone];
          }
        }
        covered += 1;
      }
      pixels[index] = resolve(colors, covered);
    }
    true
  }
}

fn geometry(x: f32, y: f32, profile: &[(f32, f32); 2048]) -> Sample {
  let radius = x.hypot(y);
  let qx = x.abs() - 0.816;
  let qy = y.abs() - 0.816;
  let edge = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - 0.184;
  let light = (0.60 - 0.22 * x - 0.30 * y).clamp(0.0, 1.0);
  let mut sample =
    Sample { layer: OUTSIDE, tone: 0, light: 0, sector: 0.0, span: 0.0, gradient: [0.0; 2] };
  if edge > 0.0 {
    return sample;
  }
  sample.layer = SHELL;
  sample.tone = byte(if edge > -0.012 { 0.55 + 0.45 * light } else { 0.06 + 0.45 * light });
  if radius < 0.91 {
    sample.tone = byte(0.08 + 0.25 * light);
    if radius > 0.84 {
      sample.tone = byte(0.08 + 0.25 * light + 0.08 * (1.0 - (radius - 0.84) / 0.07));
    } else if radius > 0.765 {
      sample.layer = RING;
      let ridge = (1.0 - ((radius - 0.831) / 0.009).abs()).max(0.0);
      let bevel = (1.0 - ((radius - 0.795) / 0.015).abs()).max(0.0);
      sample.tone = byte(0.08 + 0.16 * bevel * light + 0.72 * ridge);
    } else {
      sample.layer = WELL;
      sample.tone = byte(0.06 * light);
      if radius < 0.74 && radius > 0.245 {
        sample.layer = ROTOR;
        sample.tone = 0;
        let (start, span) = profile[((radius / 0.74 * 2048.0) as usize).min(2047)];
        sample.sector = ((y.atan2(x) - start) * 7.0 / TAU as f32).rem_euclid(7.0);
        sample.span = span;
        // CSS square gradients span |sin(124deg)| + |cos(124deg)| box widths.
        let (sin, cos) = start.sin_cos();
        sample.gradient = [
          radius / (1.48 * 1.388_230_5) * (0.829_037_6 * cos + 0.559_192_9 * sin),
          radius / (1.48 * 1.388_230_5) * (-0.829_037_6 * sin + 0.559_192_9 * cos),
        ];
        let shadow = ((x.hypot(y - 0.025) - 0.245) / 0.06).clamp(0.0, 1.0);
        sample.light = (15.0 * shadow * (0.94 + 0.06 * light)) as u8;
      } else if radius <= 0.245 {
        sample.layer = HUB;
        sample.tone =
          if radius > 0.235 { 255 } else { byte((x + 0.0672).hypot(y + 0.12) / 0.4733) };
      }
    }
  }
  let sx = x.abs() - 0.79;
  let sy = y.abs() - 0.79;
  let screw = sx.hypot(sy);
  if screw < 0.045 {
    sample.layer = SCREW;
    let slot = (sx + sy).abs() < 0.006 || (sx - sy).abs() < 0.006;
    sample.tone = byte(if screw > 0.033 {
      0.18 + 0.40 * light
    } else if slot {
      0.0
    } else {
      0.12 + 0.24 * light
    });
  }
  sample
}

/// Intersect the reference blade outline with a radius; only construction uses roots/angles.
fn blade_profile(radius: f32) -> (f32, f32) {
  let outline = [
    (0.0, 0.0),
    (0.06, -0.12),
    (0.16, -0.32),
    (0.30, -0.48),
    (0.48, -0.60),
    (0.66, -0.64),
    (0.82, -0.62),
    (0.92, -0.56),
    (0.86, -0.38),
    (0.76, -0.20),
    (0.60, -0.04),
    (0.40, 0.08),
    (0.20, 0.14),
    (0.08, 0.08),
  ];
  let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
  for (&(x, y), &(nx, ny)) in outline.iter().zip(outline.iter().cycle().skip(1)) {
    let (dx, dy) = (nx - x, ny - y);
    let a = dx * dx + dy * dy;
    let b = 2.0 * (x * dx + y * dy);
    let c = x * x + y * y - radius * radius;
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
      continue;
    }
    for t in [(-b - discriminant.sqrt()) / (2.0 * a), (-b + discriminant.sqrt()) / (2.0 * a)] {
      if (0.0..=1.0).contains(&t) {
        let angle = (y + t * dy).atan2(x + t * dx);
        low = low.min(angle);
        high = high.max(angle);
      }
    }
  }
  if low.is_finite() && high > low { (low, (high - low) * 7.0 / TAU as f32) } else { (0.0, 0.0) }
}

impl Materials {
  fn recolor(&mut self, palette: Palette, samples: &[[Sample; 4]]) {
    let Palette { background: bg, foreground: fg, accent } = palette;
    for tone in 0..256 {
      let t = tone as f32 / 255.0;
      let deep = shade(bg, 0.46);
      self.surfaces[usize::from(SHELL)][tone] =
        mix(mix(shade(bg, 0.78), accent, 0.10 * t), fg, 0.23 * t);
      self.surfaces[usize::from(WELL)][tone] = mix(deep, bg, t);
      self.surfaces[usize::from(RING)][tone] = mix(mix(deep, accent, 0.98 * t), fg, 0.06 * t);
      let hub_light = mix(mix(bg, fg, 0.30), accent, 0.10);
      let hub_middle = mix(mix(bg, accent, 0.12), fg, 0.05);
      let hub_shadow = mix(shade(bg, 0.82), accent, 0.035);
      self.surfaces[usize::from(HUB)][tone] = if tone == 255 {
        mix(mix(bg, fg, 0.38), accent, 0.10)
      } else if t < 0.44 {
        mix(hub_light, hub_middle, t / 0.44)
      } else {
        mix(hub_middle, hub_shadow, (t - 0.44) / 0.31)
      };
      self.surfaces[usize::from(SCREW)][tone] = mix(shade(bg, 0.30), fg, 0.55 * t);
      for highlighted in 0..2 {
        let glint = if highlighted == 0 { mix(mix(bg, fg, 0.70), accent, 0.20) } else { accent };
        let stops = [
          (0.0, mix(bg, fg, 0.04)),
          (0.27, mix(bg, accent, 0.10)),
          (0.51, mix(mix(bg, accent, 0.40), fg, 0.02)),
          (0.63, glint),
          (0.79, mix(bg, accent, 0.25)),
          (1.0, mix(bg, fg, 0.06)),
        ];
        let pair = stops.windows(2).find(|pair| t <= pair[1].0).unwrap_or(&stops[4..]);
        let color = mix(pair[0].1, pair[1].1, (t - pair[0].0) / (pair[1].0 - pair[0].0));
        for light in 0..16 {
          self.blades[highlighted][light][tone] =
            mix(deep, color, 0.28 + 0.72 * light as f32 / 15.0);
        }
      }
    }
    for (pixel, samples) in self.fixed.iter_mut().zip(samples) {
      let mut covered = 0;
      let colors = samples.map(|sample| {
        if sample.layer == OUTSIDE {
          return [0; 3];
        }
        covered += 1;
        let surface = if sample.layer == ROTOR { WELL } else { sample.layer };
        self.surfaces[usize::from(surface)][usize::from(sample.tone)]
      });
      *pixel = resolve(colors, covered);
    }
    self.palette = Some(palette);
  }
}

fn byte(t: f32) -> u8 {
  (t.clamp(0.0, 1.0) * 255.0) as u8
}

fn shade(color: [u8; 3], light: f32) -> [u8; 3] {
  color.map(|channel| (f32::from(channel) * light) as u8)
}

/// Fixed-point mixing derives all shades from the caller's colors, including shadows/glints.
fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
  let w = (t.clamp(0.0, 1.0) * 256.0) as u16;
  std::array::from_fn(|i| ((u16::from(a[i]) * (256 - w) + u16::from(b[i]) * w) / 256) as u8)
}

fn resolve(colors: [[u8; 3]; 4], covered: u16) -> [u8; 4] {
  let mut pixel = [0; 4];
  for channel in 0..3 {
    let sum: u16 = colors.iter().map(|color| u16::from(color[channel])).sum();
    pixel[channel] = sum.checked_div(covered).unwrap_or(0) as u8;
  }
  pixel[3] = (covered * 255 / 4) as u8;
  pixel
}

#[cfg(test)]
mod tests {
  use std::hint::black_box;
  use std::time::Instant;

  use super::{FanRaster, Palette, TAU};

  // Synthetic colors, unrelated to terminal state or user settings.
  const PALETTE: Palette =
    Palette { background: [16, 23, 34], foreground: [229, 238, 248], accent: [114, 216, 232] };

  fn render(raster: &FanRaster, angle: f64, palette: Palette) -> Vec<u8> {
    let mut rgba = vec![0; raster.rgba_len()];
    assert!(raster.render_into(angle, palette, &mut rgba));
    rgba
  }

  fn pixel(raster: &FanRaster, rgba: &[u8], x: f64, y: f64) -> [u8; 4] {
    let half = raster.size() as f64 / 2.0;
    let col = ((x + 1.0) * half).floor() as usize;
    let row = ((y + 1.0) * half).floor() as usize;
    rgba[(row * raster.size() + col) * 4..][..4].try_into().unwrap()
  }

  fn polar(raster: &FanRaster, rgba: &[u8], radius: f64, angle: f64) -> [u8; 4] {
    pixel(raster, rgba, radius * angle.cos(), radius * angle.sin())
  }

  #[test]
  fn sizes_buffers_and_angles_are_checked_without_touching_invalid_output() {
    assert!(FanRaster::new(0).is_none());
    assert!(FanRaster::new(513).is_none());
    for size in [1, 2, 200, 240, 512] {
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
  fn blade_profiles_are_finite_and_empty_radii_do_not_create_a_blade() {
    for radius in [0.0, 2.0, f32::NAN, f32::INFINITY] {
      assert_eq!(super::blade_profile(radius), (0.0, 0.0));
    }
    for i in 0..2048 {
      let (start, span) = super::blade_profile((i as f32 + 0.5) / 2048.0);
      assert!(start.is_finite() && span.is_finite() && span > 0.0);
    }
    let raster = FanRaster::new(40).unwrap();
    let mut output = vec![0; raster.rgba_len()];
    // A missing radial profile leaves the deep well visible even at an exact sector boundary.
    let mut empty = raster.samples.clone();
    for sample in empty.iter_mut().flatten().filter(|s| s.layer == super::ROTOR) {
      sample.span = 0.0;
      sample.sector = 0.0;
    }
    let empty_raster = FanRaster { samples: empty, ..raster };
    assert!(empty_raster.render_into(0.0, PALETTE, &mut output));
    assert_eq!(polar(&empty_raster, &output, 0.5, 0.0)[..3], [7, 10, 15]);
  }

  #[test]
  fn rounded_housing_has_transparent_corners_and_straight_alpha_edges() {
    let raster = FanRaster::new(240).unwrap();
    let mono = Palette { background: [255; 3], foreground: [255; 3], accent: [255; 3] };
    let rgba = render(&raster, 0.0, mono);
    for (x, y) in [(-0.99, -0.99), (0.99, -0.99), (-0.99, 0.99), (0.99, 0.99)] {
      assert_eq!(pixel(&raster, &rgba, x, y), [0; 4]);
    }
    assert_eq!(pixel(&raster, &rgba, 0.0, -0.93)[3], 255);
    assert_eq!(pixel(&raster, &rgba, 0.91, 0.91)[3], 255);
    let mut edges = 0;
    for p in rgba.as_chunks::<4>().0.iter() {
      if p[3] == 0 {
        assert_eq!(*p, [0; 4]);
      } else {
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        if p[3] != 255 {
          edges += 1;
          assert!([63, 127, 191].contains(&p[3]));
          assert!(p[0] > p[3], "edge RGB must not be premultiplied: {p:?}");
        }
      }
    }
    assert!(edges > 40);
  }

  #[test]
  fn screws_ring_and_shaft_stay_fixed_while_seven_blades_move() {
    let raster = FanRaster::new(240).unwrap();
    let first = render(&raster, 0.0, PALETTE);
    for angle in [0.01, 0.31, 0.75, TAU / 7.0] {
      let next = render(&raster, angle, PALETTE);
      assert_ne!(first, next);
      for (i, samples) in raster.samples.iter().enumerate() {
        if samples.iter().all(|sample| sample.layer != super::ROTOR) {
          assert_eq!(&first[i * 4..][..4], &next[i * 4..][..4]);
        }
      }
      for (x, y) in [(-0.79, -0.79), (0.79, -0.79), (-0.79, 0.79), (0.79, 0.79)] {
        let slot = pixel(&raster, &next, x, y);
        let shoulder = pixel(&raster, &next, x + 0.032, y);
        assert!(slot[0] < shoulder[0], "screw recess {x}, {y}");
      }
    }
    let unmarked = Palette { background: [128; 3], foreground: [128; 3], accent: [128; 3] };
    let a = render(&raster, 0.0, unmarked);
    let b = render(&raster, TAU / 7.0, unmarked);
    let changed =
      a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()).filter(|(a, b)| a != b).count();
    assert!(changed < 80, "seven-fold image differs at {changed} pixels");
    let occupancy: Vec<_> = (0..720)
      .map(|i| polar(&raster, &first, 0.68, i as f64 * TAU / 720.0)[1] > PALETTE.background[1] + 10)
      .collect();
    let starts = (0..720).filter(|&i| occupancy[i] && !occupancy[(i + 719) % 720]).count();
    assert_eq!(starts, 7, "seven separate swept tips");
  }

  #[test]
  fn accent_belongs_to_one_blade_and_turns_clockwise_with_the_rotor() {
    let raster = FanRaster::new(240).unwrap();
    let palette = Palette { background: [20; 3], foreground: [180; 3], accent: [255, 70, 40] };
    let center = |angle| {
      let rgba = render(&raster, angle, palette);
      let (mut x, mut y, mut count) = (0.0, 0.0, 0.0);
      for &index in &raster.rotating {
        let p = &rgba[index * 4..][..4];
        if i16::from(p[0]) - i16::from(p[2]) > 140 {
          x += (index % 240) as f64 - 119.5;
          y += (index / 240) as f64 - 119.5;
          count += 1.0;
        }
      }
      assert!(count > 100.0, "a broad accent surface must remain visible");
      let length = x.hypot(y);
      (x / length, y / length)
    };
    let (x, y) = center(0.0);
    let (nx, ny) = center(TAU / 4.0);
    assert!((x * nx + y * ny).abs() < 0.15, "accent turns a quarter turn");
    assert!(x * ny - y * nx > 0.95, "positive phase turns clockwise");
  }

  #[test]
  fn curved_metal_surfaces_have_glints_shadows_and_palette_driven_color() {
    let raster = FanRaster::new(240).unwrap();
    let rgba = render(&raster, 0.0, PALETTE);
    let above = pixel(&raster, &rgba, -0.07, -0.09);
    let below = pixel(&raster, &rgba, 0.10, 0.14);
    assert!(above[2] > below[2] + 20, "fixed hub highlight");
    let ring = polar(&raster, &rgba, 0.831, -TAU / 8.0);
    let outer = polar(&raster, &rgba, 0.85, -TAU / 8.0);
    assert!(ring[1] > outer[1] + 40, "thin luminous ring");
    let mut shades = std::collections::BTreeSet::new();
    for sample in rgba.as_chunks::<4>().0.iter().filter(|p| p[3] == 255) {
      shades.insert(sample[..3].to_vec());
      for i in 0..3 {
        let high = PALETTE.background[i].max(PALETTE.foreground[i]).max(PALETTE.accent[i]);
        assert!(sample[i] <= high);
      }
    }
    assert!(shades.len() > 200, "continuous material shading");
    let well = polar(&raster, &rgba, 0.755, 0.0);
    assert!(well[0] < PALETTE.background[0] && well[2] < PALETTE.background[2]);
    let neutral =
      render(&raster, 0.4, Palette { background: [32; 3], foreground: [200; 3], accent: [120; 3] });
    assert!(neutral.as_chunks::<4>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2]));
    let changed = render(
      &raster,
      0.0,
      Palette { background: [230; 3], foreground: [40; 3], accent: [180, 50, 90] },
    );
    assert_ne!(rgba, changed);
    assert_eq!(
      rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect::<Vec<_>>(),
      changed.as_chunks::<4>().0.iter().map(|p| p[3]).collect::<Vec<_>>()
    );
  }

  #[test]
  fn geometry_and_material_buffers_are_reused_when_phase_or_palette_changes() {
    let raster = FanRaster::new(200).unwrap();
    let geometry = raster.geometry_ptr();
    let fixed = raster.materials.lock().unwrap().fixed.as_ptr();
    let rotating = raster.rotating.as_ptr();
    let mut rgba = vec![0; raster.rgba_len()];
    let output = rgba.as_ptr();
    for (angle, palette) in
      [(0.0, PALETTE), (0.7, PALETTE), (0.9, Palette { accent: [90, 180, 70], ..PALETTE })]
    {
      assert!(raster.render_into(angle, palette, &mut rgba));
      assert_eq!(raster.geometry_ptr(), geometry);
      assert_eq!(raster.materials.lock().unwrap().fixed.as_ptr(), fixed);
      assert_eq!(raster.rotating.as_ptr(), rotating);
      assert_eq!(rgba.as_ptr(), output);
    }
  }

  #[test]
  #[ignore = "explicit CPU-only raster benchmark, not terminal presentation FPS"]
  fn benchmark_240px_120_frames() {
    let setup = Instant::now();
    let raster = FanRaster::new(240).unwrap();
    let mut rgba = vec![0; raster.rgba_len()];
    assert!(raster.render_into(0.0, PALETTE, &mut rgba));
    let setup_ms = setup.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    for frame in 0..120 {
      assert!(raster.render_into(black_box(frame as f64 * TAU / 120.0), PALETTE, &mut rgba));
      black_box(&rgba);
    }
    let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
    let geometry_bytes = raster.samples.capacity() * std::mem::size_of::<[super::Sample; 4]>()
      + raster.rotating.capacity() * std::mem::size_of::<usize>()
      + std::mem::size_of_val(&raster.gradient);
    let materials = raster.materials.lock().unwrap();
    let material_bytes = materials.fixed.capacity() * 4
      + std::mem::size_of_val(&materials.surfaces)
      + std::mem::size_of_val(&materials.blades);
    println!(
      "CPU raster only: 240x240, 120 frames, setup={setup_ms:.3}ms, total={milliseconds:.3}ms, mean={:.3}ms/frame, geometry={geometry_bytes} bytes, materials={material_bytes} bytes, RGBA={} bytes",
      milliseconds / 120.0,
      rgba.len()
    );
  }
}
