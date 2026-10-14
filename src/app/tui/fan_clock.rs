//! Monotonic frame deadlines and independent illustrative fan rotors, separate from sampling.

use std::time::{Duration, Instant};

/// Target period; this does not measure the terminal's presentation rate.
pub(super) const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// Pauses without preserving elapsed time; delayed frames are skipped.
#[derive(Debug, Default)]
pub(super) struct FrameClock {
  next: Option<Instant>,
  previous: Option<Instant>,
}

impl FrameClock {
  /// Repeated activation keeps the deadline; deactivation discards pause time.
  pub(super) fn set_active(&mut self, active: bool, now: Instant) {
    if !active {
      self.next = None;
      self.previous = None;
    } else if self.next.is_none() {
      self.next = Some(now + FRAME_INTERVAL);
      self.previous = Some(now);
    }
  }

  /// No deadline means the caller can block waiting for input or a metric sample.
  pub(super) fn wait(&self, now: Instant) -> Option<Duration> {
    self.next.map(|deadline| deadline.saturating_duration_since(now))
  }

  /// Returns at most 100 ms of elapsed time and schedules only the next future frame.
  pub(super) fn frame(&mut self, now: Instant) -> Option<Duration> {
    let deadline = self.next?;
    if now < deadline {
      return None;
    }
    let elapsed =
      now.saturating_duration_since(self.previous.unwrap()).min(Duration::from_millis(100));
    let missed = now.saturating_duration_since(deadline).as_nanos() / FRAME_INTERVAL.as_nanos();
    self.next = Some(deadline + FRAME_INTERVAL.mul_f64((missed + 1) as f64));
    self.previous = Some(now);
    Some(elapsed)
  }
}

/// One fan's phase and smoothed visual speed, retaining its real reported RPM.
#[derive(Debug, Default, Clone)]
pub(super) struct Rotor {
  /// Clockwise angle in radians.
  pub(super) angle: f64,
  /// Illustrative radians per second; never exported as a hardware measurement.
  pub(super) velocity: f64,
  rpm: Option<u32>,
}

impl Rotor {
  /// A zero or missing reading stops immediately without changing the phase.
  pub(super) fn set_rpm(&mut self, rpm: Option<u32>) {
    self.rpm = rpm;
    if rpm.unwrap_or(0) == 0 {
      self.velocity = 0.0;
    }
  }

  /// Only a positive actual RPM reading allows animation.
  pub(super) fn moving(&self) -> bool {
    self.rpm.unwrap_or(0) > 0
  }

  /// Smooths positive RPM updates while bounding a single step to 100 ms.
  pub(super) fn advance(&mut self, delta: Duration) {
    if !self.moving() {
      return;
    }
    let rpm = f64::from(self.rpm.unwrap());
    // Illustrative visual speed; never reports this mapping as hardware RPM or a limit.
    let target = std::f64::consts::TAU * (0.12 + 0.68 * rpm / (rpm + 3000.0));
    let seconds = delta.min(Duration::from_millis(100)).as_secs_f64();
    self.velocity += (target - self.velocity) * (1.0 - (-4.0 * seconds).exp());
    self.angle = (self.angle + self.velocity * seconds).rem_euclid(std::f64::consts::TAU);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn disabled_clock_blocks_instead_of_scheduling_frames() {
    let mut clock = FrameClock::default();
    let now = Instant::now();
    assert!(clock.wait(now).is_none());
    clock.set_active(true, now);
    assert_eq!(clock.wait(now), Some(FRAME_INTERVAL));
    clock.set_active(false, now + Duration::from_secs(8));
    assert!(clock.wait(now).is_none());
    assert!(clock.frame(now + Duration::from_secs(99)).is_none());
    clock.set_active(true, now + Duration::from_secs(99));
    assert_eq!(clock.frame(now + Duration::from_secs(99) + FRAME_INTERVAL), Some(FRAME_INTERVAL));
  }

  #[test]
  fn early_events_do_not_reset_the_frame_deadline_and_late_frames_are_skipped() {
    let now = Instant::now();
    let mut clock = FrameClock::default();
    clock.set_active(true, now);
    for millis in 1..=16 {
      let event = now + Duration::from_millis(millis);
      clock.set_active(true, event);
      assert!(clock.frame(event).is_none());
    }
    assert_eq!(clock.frame(now + FRAME_INTERVAL), Some(FRAME_INTERVAL));
    let stalled = now + Duration::from_secs(9);
    assert_eq!(clock.frame(stalled), Some(Duration::from_millis(100)));
    assert!(clock.frame(stalled).is_none());
    let wait = clock.wait(stalled).unwrap();
    assert!(wait > Duration::ZERO && wait <= FRAME_INTERVAL);
  }

  #[test]
  fn sixty_frames_are_scheduled_independently_of_five_second_sampling() {
    let now = Instant::now();
    let mut clock = FrameClock::default();
    clock.set_active(true, now);
    let mut count = 0;
    for tick in 1..=60 {
      let frame = now + FRAME_INTERVAL * tick;
      assert!(clock.frame(frame).is_some());
      count += 1;
    }
    assert_eq!(count, 60);
    assert!(FRAME_INTERVAL * 60 < Duration::from_secs(5));
  }

  #[test]
  fn each_fan_follows_its_own_rpm_and_zero_or_absence_stops_immediately() {
    let mut slow = Rotor::default();
    let mut fast = Rotor::default();
    slow.set_rpm(Some(1000));
    fast.set_rpm(Some(4000));
    for _ in 0..60 {
      slow.advance(FRAME_INTERVAL);
      fast.advance(FRAME_INTERVAL);
    }
    assert!(fast.velocity > slow.velocity && fast.angle > slow.angle);
    let before = fast.angle;
    for rpm in [Some(0), None] {
      fast.set_rpm(rpm);
      assert!(!fast.moving() && fast.velocity == 0.0);
      fast.advance(Duration::from_secs(5));
      assert_eq!(fast.angle, before);
    }
  }

  #[test]
  fn rpm_changes_preserve_phase_and_smooth_velocity_with_bounded_delayed_steps() {
    let mut rotor = Rotor::default();
    rotor.set_rpm(Some(1500));
    rotor.advance(FRAME_INTERVAL);
    let (angle, velocity) = (rotor.angle, rotor.velocity);
    rotor.set_rpm(Some(6000));
    assert_eq!(rotor.rpm, Some(6000));
    assert_eq!((rotor.angle, rotor.velocity), (angle, velocity));
    rotor.advance(FRAME_INTERVAL);
    let target = std::f64::consts::TAU * (0.12 + 0.68 * 6000.0 / 9000.0);
    assert!(rotor.angle > angle && rotor.velocity > velocity && rotor.velocity < target);

    let mut delayed = rotor.clone();
    rotor.advance(Duration::from_millis(100));
    delayed.advance(Duration::from_secs(5));
    assert_eq!((rotor.angle, rotor.velocity), (delayed.angle, delayed.velocity));
    delayed.set_rpm(Some(u32::MAX));
    for _ in 0..600 {
      delayed.advance(FRAME_INTERVAL);
      assert!(delayed.velocity.is_finite());
      assert!((0.0..std::f64::consts::TAU).contains(&delayed.angle));
    }
    assert_eq!(delayed.rpm, Some(u32::MAX));
  }
}
