#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
  Linear,
  CubicBezier {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
  },
  Steps {
    count: u32,
    position: StepPosition,
  },
  /// A spring that comes to rest without going past: quick at first, settling slowly, as the platform's own
  /// moves of screens and sheets are. `velocity` is the speed the value already has when the run starts, as
  /// a share of the way per duration: 0 from rest, and 2 for something let go while moving at a speed that
  /// would cover the way twice in the duration. A speed too great for the spring to take without passing
  /// the end is taken as the greatest it can.
  Spring {
    velocity: f64,
  },
}

/// How stiff [`Easing::Spring`] is: by the end of the duration it is within a twentieth of a percent of rest.
const SPRING_RATE: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepPosition {
  JumpStart,
  JumpEnd,
  JumpNone,
  JumpBoth,
}

impl Easing {
  pub const EASE: Self = Self::CubicBezier {
    x1: 0.25,
    y1: 0.1,
    x2: 0.25,
    y2: 1.0,
  };
  pub const EASE_IN: Self = Self::CubicBezier {
    x1: 0.42,
    y1: 0.0,
    x2: 1.0,
    y2: 1.0,
  };
  pub const EASE_OUT: Self = Self::CubicBezier {
    x1: 0.0,
    y1: 0.0,
    x2: 0.58,
    y2: 1.0,
  };
  pub const EASE_IN_OUT: Self = Self::CubicBezier {
    x1: 0.42,
    y1: 0.0,
    x2: 0.58,
    y2: 1.0,
  };

  pub fn evaluate(&self, t: f64) -> f64 {
    match self {
      Self::Linear => t,
      Self::CubicBezier { x1, y1, x2, y2 } => {
        if t <= 0.0 {
          return 0.0;
        }
        if t >= 1.0 {
          return 1.0;
        }
        let u = cubic_bezier_solve(t, *x1, *x2);
        bezier_y(u, *y1, *y2)
      }
      Self::Steps { count, position } => evaluate_steps(t, *count, *position),
      Self::Spring { velocity } => {
        if t <= 0.0 {
          return 0.0;
        }
        if t >= 1.0 {
          return 1.0;
        }
        // A critically damped spring from 0 to 1 that starts at `velocity`: 1 - (1 + (w - v) t) e^(-w t),
        // which never passes 1 while v <= w. Scaled to end at 1 exactly.
        let velocity = velocity.clamp(0.0, SPRING_RATE);
        let at = |t: f64| 1.0 - (1.0 + (SPRING_RATE - velocity) * t) * (-SPRING_RATE * t).exp();
        at(t) / at(1.0)
      }
    }
  }

  /// How far `t` (in `0..=1`) moves, forward or backward, before a step easing's value changes: to the next of
  /// its `count` equal steps. `None` for an easing whose value changes continuously, and for a `t` that is not a
  /// number.
  pub(super) fn distance_to_next_step(&self, t: f64, forward: bool) -> Option<f64> {
    let Self::Steps { count, position } = self else {
      return None;
    };
    if t.is_nan() {
      return None;
    }
    let count = f64::from((*count).max(1));
    let t = t.clamp(0.0, 1.0);
    // The value is 0 at `t <= 0` and 1 at `t >= 1` whatever the position; a position that jumps at that end leaves
    // the value there at once.
    let jumps_at_start = matches!(position, StepPosition::JumpStart | StepPosition::JumpBoth);
    let jumps_at_end = matches!(position, StepPosition::JumpEnd | StepPosition::JumpBoth);
    if (forward && t <= 0.0 && jumps_at_start) || (!forward && t >= 1.0 && jumps_at_end) {
      return Some(0.0);
    }
    let step = (t * count).floor();
    let boundary = if forward {
      ((step + 1.0) / count).min(1.0)
    } else {
      step.min(count - 1.0) / count
    };
    Some((boundary - t).abs())
  }
}

impl Default for Easing {
  fn default() -> Self {
    Self::EASE
  }
}

fn bezier_x(u: f64, x1: f64, x2: f64) -> f64 {
  let u2 = u * u;
  let u3 = u2 * u;
  let inv = 1.0 - u;
  let inv2 = inv * inv;
  3.0 * inv2 * u * x1 + 3.0 * inv * u2 * x2 + u3
}

fn bezier_x_deriv(u: f64, x1: f64, x2: f64) -> f64 {
  let u2 = u * u;
  let inv = 1.0 - u;
  3.0 * inv * inv * x1 + 6.0 * inv * u * (x2 - x1) + 3.0 * u2 * (1.0 - x2)
}

fn bezier_y(u: f64, y1: f64, y2: f64) -> f64 {
  let u2 = u * u;
  let u3 = u2 * u;
  let inv = 1.0 - u;
  let inv2 = inv * inv;
  3.0 * inv2 * u * y1 + 3.0 * inv * u2 * y2 + u3
}

fn cubic_bezier_solve(t: f64, x1: f64, x2: f64) -> f64 {
  let mut u = t;

  for _ in 0..8 {
    let dx = bezier_x(u, x1, x2) - t;
    if dx.abs() < 1e-7 {
      return u;
    }
    let ddx = bezier_x_deriv(u, x1, x2);
    if ddx.abs() < 1e-12 {
      break;
    }
    u -= dx / ddx;
    u = u.clamp(0.0, 1.0);
  }

  let mut lo = 0.0_f64;
  let mut hi = 1.0_f64;
  u = (lo + hi) * 0.5;
  for _ in 0..20 {
    let x = bezier_x(u, x1, x2);
    if (x - t).abs() < 1e-7 {
      return u;
    }
    if x < t {
      lo = u;
    } else {
      hi = u;
    }
    u = (lo + hi) * 0.5;
  }
  u
}

fn evaluate_steps(t: f64, count: u32, position: StepPosition) -> f64 {
  if t <= 0.0 {
    return 0.0;
  }
  if t >= 1.0 {
    return 1.0;
  }

  let steps = count as f64;
  let (intervals, offset) = match position {
    StepPosition::JumpStart => (steps, 1.0),
    StepPosition::JumpEnd => (steps, 0.0),
    StepPosition::JumpBoth => (steps + 1.0, 1.0),
    StepPosition::JumpNone => ((steps - 1.0).max(1.0), 0.0),
  };

  let current_step = (t * steps).floor() + offset;
  (current_step / intervals).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn linear_identity() {
    let e = Easing::Linear;
    assert_eq!(e.evaluate(0.0), 0.0);
    assert_eq!(e.evaluate(0.5), 0.5);
    assert_eq!(e.evaluate(1.0), 1.0);
  }

  #[test]
  fn ease_endpoints() {
    assert_eq!(Easing::EASE.evaluate(0.0), 0.0);
    assert_eq!(Easing::EASE.evaluate(1.0), 1.0);
  }

  #[test]
  fn ease_in_slow_start() {
    let v = Easing::EASE_IN.evaluate(0.25);
    assert!(v < 0.25, "ease-in at t=0.25 should be < 0.25, got {v}");
  }

  #[test]
  fn ease_out_fast_start() {
    let v = Easing::EASE_OUT.evaluate(0.25);
    assert!(v > 0.25, "ease-out at t=0.25 should be > 0.25, got {v}");
  }

  #[test]
  fn steps_jump_end() {
    let e = Easing::Steps {
      count: 4,
      position: StepPosition::JumpEnd,
    };
    assert_eq!(e.evaluate(0.0), 0.0);
    assert_eq!(e.evaluate(0.1), 0.0);
    assert!((e.evaluate(0.3) - 0.25).abs() < 1e-9);
    assert_eq!(e.evaluate(1.0), 1.0);
  }
}
