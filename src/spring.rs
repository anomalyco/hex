//! Frame-rate independent spring physics for the dictation HUD and Dot
//! scope's simulation.

const CRITICAL_DAMPING_TOLERANCE: f32 = 1e-4;

/// Duration + bounce, SwiftUI mapping (`ω = 2π / duration`, `ζ = 1 − bounce`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub duration: f32,
    pub bounce: f32,
}

impl Spec {
    pub const fn new(duration: f32, bounce: f32) -> Self {
        Self { duration, bounce }
    }

    /// Constructs a critically damped spring (`bounce = 0`) from its angular frequency `ω` (rad/s).
    pub fn from_omega(omega: f32) -> Self {
        Self {
            duration: std::f32::consts::TAU / omega.max(0.1),
            bounce: 0.0,
        }
    }

    pub fn omega(self) -> f32 {
        std::f32::consts::TAU / self.duration.max(0.01)
    }

    pub fn zeta(self) -> f32 {
        if self.bounce >= 0.0 {
            (1.0 - self.bounce).max(0.0)
        } else {
            1.0 / (1.0 + self.bounce).max(0.01)
        }
    }
}

/// Short opacity-only crossfade preserved when Reduce Motion is enabled.
pub const REDUCED_FADE: Spec = Spec::new(0.16, 0.0);

/// Exact analytical damped harmonic oscillator state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub target: f32,
}

impl Spring {
    pub const fn at(x: f32) -> Self {
        Self {
            x,
            v: 0.0,
            target: x,
        }
    }

    /// Snaps immediately to `x` and clears velocity (for first paint or Reduce Motion).
    pub fn jump(&mut self, x: f32) {
        self.x = x;
        self.v = 0.0;
        self.target = x;
    }

    /// Retargets to `target` and advances by `dt` seconds using the closed-form state propagator.
    pub fn step_to(&mut self, target: f32, spec: Spec, dt: f32) {
        self.target = target;
        self.step(spec, dt);
    }

    /// Advances the spring toward `self.target` by `dt` seconds.
    ///
    /// Solved analytically across underdamped, critically damped, and overdamped regimes
    /// so trajectories are independent of frame rate and unconditionally stable.
    fn step(&mut self, spec: Spec, dt: f32) {
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }
        let omega = spec.omega();
        let zeta = spec.zeta();
        let x0 = self.x - self.target;
        let v0 = self.v;

        if zeta < 1.0 - CRITICAL_DAMPING_TOLERANCE {
            let decay = zeta * omega;
            let omega_d = omega * (1.0 - zeta * zeta).sqrt();
            let exp = (-decay * dt).exp();
            let (sin, cos) = (omega_d * dt).sin_cos();
            let sin_over_d = sin / omega_d;
            self.x = self.target + exp * (x0 * (cos + decay * sin_over_d) + v0 * sin_over_d);
            self.v = exp * (-x0 * omega * omega * sin_over_d + v0 * (cos - decay * sin_over_d));
        } else if zeta > 1.0 + CRITICAL_DAMPING_TOLERANCE {
            let root = (zeta * zeta - 1.0).sqrt();
            let root_sum = zeta + root;
            let slow_root = -omega / root_sum;
            let fast_root = -omega * root_sum;
            let denom = slow_root - fast_root;
            let slow_exp = (slow_root * dt).exp();
            let fast_exp = (fast_root * dt).exp();
            let p00 = (-fast_root * slow_exp + slow_root * fast_exp) / denom;
            let p01 = (slow_exp - fast_exp) / denom;
            let p10 = slow_root * fast_root * (fast_exp - slow_exp) / denom;
            let p11 = (slow_root * slow_exp - fast_root * fast_exp) / denom;
            self.x = self.target + p00 * x0 + p01 * v0;
            self.v = p10 * x0 + p11 * v0;
        } else {
            let exp = (-omega * dt).exp();
            let c = v0 + omega * x0;
            self.x = self.target + (x0 + c * dt) * exp;
            self.v = (v0 - omega * c * dt) * exp;
        }
    }

    /// Tests whether both displacement and ω-scaled velocity are within `eps` of `self.target`.
    pub fn settled(&self, spec: Spec, eps: f32) -> bool {
        let omega = spec.omega();
        (self.x - self.target).abs() <= eps && self.v.abs() <= eps * omega
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAP: Spec = Spec::new(0.25, 0.0);
    const SETTLE: Spec = Spec::new(0.3, 0.0);
    const MORPH: Spec = Spec::new(0.3, 0.16);

    #[test]
    fn spec_maps_duration_and_bounce_like_swiftui() {
        assert!((SNAP.omega() - std::f32::consts::TAU / 0.25).abs() < 1e-4);
        assert_eq!(SNAP.zeta(), 1.0);
        assert!((MORPH.zeta() - 0.84).abs() < 1e-5);
        assert!(Spec::new(0.3, -0.2).zeta() > 1.0);
    }

    #[test]
    fn step_preserves_semigroup_across_damping_regimes() {
        for spec in [SNAP, MORPH, Spec::new(0.25, 0.15), Spec::new(0.35, -0.25)] {
            let mut split = Spring {
                x: -2.5,
                v: 4.0,
                target: 6.0,
            };
            let mut direct = split;
            split.step(spec, 0.013);
            split.step(spec, 0.021);
            direct.step(spec, 0.034);

            assert!(
                (split.x - direct.x).abs() < 2e-4,
                "position mismatch for {spec:?}: {} vs {}",
                split.x,
                direct.x
            );
            assert!(
                (split.v - direct.v).abs() < 2e-4,
                "velocity mismatch for {spec:?}: {} vs {}",
                split.v,
                direct.v
            );
        }
    }

    #[test]
    fn trajectories_match_at_60_120_and_240_hz() {
        let simulate = |hz: usize, spec: Spec| {
            let mut s = Spring::at(0.0);
            let steps = hz / 2; // 0.5 seconds
            let dt = 1.0 / hz as f32;
            for _ in 0..steps {
                s.step_to(1.0, spec, dt);
            }
            (s.x, s.v)
        };
        for spec in [SNAP, SETTLE, MORPH] {
            let s60 = simulate(60, spec);
            let s120 = simulate(120, spec);
            let s240 = simulate(240, spec);
            assert!((s60.0 - s120.0).abs() < 1e-4);
            assert!((s120.0 - s240.0).abs() < 1e-4);
            assert!((s60.1 - s120.1).abs() < 1e-3);
        }
    }

    #[test]
    fn retargeting_mid_flight_preserves_position_and_velocity() {
        let mut s = Spring::at(0.0);
        s.step_to(1.0, SETTLE, 0.08);
        let pos_before = s.x;
        let vel_before = s.v;
        assert!(pos_before > 0.1 && pos_before < 0.9);
        assert!(vel_before > 0.5);

        // Retargeting back to 0.0 keeps momentum: the next millisecond still moves forward.
        s.step_to(0.0, SETTLE, 0.001);
        assert!(s.x > pos_before);
    }

    #[test]
    fn settling_requires_both_low_displacement_and_low_velocity() {
        let moving = Spring {
            x: 1.0,
            v: 1.0,
            target: 1.0,
        };
        assert!(!moving.settled(SETTLE, 0.001));

        let resting = Spring {
            x: 1.0002,
            v: 0.001,
            target: 1.0,
        };
        assert!(resting.settled(SETTLE, 0.001));
    }
}
