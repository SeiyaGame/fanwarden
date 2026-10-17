//! Target temperature mode: finds the lowest fan duty that holds a temperature.
//!
//! A PI controller in velocity form (each cycle adds a correction to the previous duty), which
//! cannot wind up past the zone limits. Corrections are rate limited, faster up than down, and
//! the rule's curve stays as a safety net when the temperature runs well above the target.

use serde::{Deserialize, Serialize};

use crate::sensors::Source;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reactivity {
    Calm,
    #[default]
    Normal,
    Reactive,
}

/// How fast a source changes: disks take minutes, a CPU seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inertia {
    Slow,
    Fast,
}

impl Inertia {
    pub fn of(source: &Source) -> Self {
        match source {
            Source::Cpu => Inertia::Fast,
            Source::Hwmon { chip, .. } if matches!(chip.as_str(), "k10temp" | "zenpower" | "coretemp") => Inertia::Fast,
            Source::Ipmi(name) if name.to_ascii_uppercase().contains("CPU") => Inertia::Fast,
            _ => Inertia::Slow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gains {
    /// % of duty per °C of error change.
    pub kp: f32,
    /// % of duty per °C of error and per second.
    pub ki: f32,
    /// Errors within ±deadband °C are ignored (sensor resolution, noise).
    pub deadband: f32,
    /// Largest duty change per 10 s, upward and downward.
    pub max_up: f32,
    pub max_down: f32,
    /// °C above the target from which the curve is enforced as a minimum.
    pub emergency_margin: f32,
    /// Time constant of the smoothing applied to readings, in seconds: whole-degree steps
    /// would otherwise kick the proportional term at every change.
    pub smoothing_secs: f32,
}

impl Gains {
    pub fn new(inertia: Inertia, reactivity: Reactivity) -> Self {
        let base = match inertia {
            // Integer temperatures, minutes of thermal inertia: small, patient corrections.
            Inertia::Slow => Gains {
                kp: 4.0,
                ki: 0.02,
                deadband: 0.5,
                max_up: 4.0,
                max_down: 1.0,
                emergency_margin: 4.0,
                smoothing_secs: 60.0,
            },
            // Noisy, fast readings: wider deadband, larger steps.
            Inertia::Fast => Gains {
                kp: 2.0,
                ki: 0.08,
                deadband: 1.5,
                max_up: 10.0,
                max_down: 3.0,
                emergency_margin: 10.0,
                smoothing_secs: 10.0,
            },
        };
        let factor = match reactivity {
            Reactivity::Calm => 0.5,
            Reactivity::Normal => 1.0,
            Reactivity::Reactive => 2.0,
        };
        Gains {
            kp: base.kp * factor,
            ki: base.ki * factor,
            max_up: base.max_up * factor,
            max_down: base.max_down * factor,
            ..base
        }
    }
}

/// State of one target rule between two cycles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Regulator {
    /// Fractional duty, so that corrections below 1% accumulate.
    duty: Option<f32>,
    last_error: f32,
    /// Smoothed temperature.
    filtered: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Output {
    pub duty: u8,
    /// The temperature is far above the target: the curve is enforced.
    pub emergency: bool,
}

impl Regulator {
    /// One control step. `curve_duty` is what the rule's curve asks for at `temp`: starting
    /// point on the first step and minimum in an emergency. `dt` is the time since the last step.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        temp: f32,
        target: f32,
        dt: f32,
        gains: &Gains,
        curve_duty: f32,
        min_duty: f32,
        max_duty: f32,
    ) -> Output {
        let alpha = dt / (gains.smoothing_secs + dt);
        let filtered = self.filtered.map_or(temp, |f| f + alpha * (temp - f));
        self.filtered = Some(filtered);
        let error = filtered - target;
        let effective = if error.abs() <= gains.deadband { 0.0 } else { error };
        let mut duty = match self.duty {
            None => curve_duty,
            Some(previous) => {
                let delta = gains.kp * (effective - self.last_error) + gains.ki * dt * effective;
                let scale = (dt / 10.0).max(0.1);
                previous + delta.clamp(-gains.max_down * scale, gains.max_up * scale)
            }
        };
        self.last_error = effective;
        let emergency = temp - target >= gains.emergency_margin;
        if emergency {
            duty = duty.max(curve_duty);
        }
        let duty = duty.clamp(min_duty, max_duty.max(min_duty));
        self.duty = Some(duty);
        Output { duty: duty.round().clamp(0.0, 100.0) as u8, emergency }
    }

    /// Forget the state (source asleep or missing): the next step starts again from the curve.
    pub fn reset(&mut self) {
        *self = Regulator::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;

    /// A disk whose temperature drifts toward `hot - cooling * duty` with a time constant.
    struct Disk {
        temp: f32,
        hot: f32,
        cooling: f32,
        tau: f32,
    }

    impl Disk {
        fn step(&mut self, duty: f32, dt: f32) {
            let equilibrium = self.hot - self.cooling * duty;
            self.temp += (equilibrium - self.temp) * (dt / self.tau);
        }

        /// Disks report whole degrees.
        fn reading(&self) -> f32 {
            self.temp.round()
        }
    }

    fn curve() -> Curve {
        Curve::new(vec![(33.0, 40), (36.0, 50), (38.0, 60), (40.0, 75), (42.0, 88), (44.0, 100)])
    }

    /// Runs `hours` of 10 s cycles, returns (temperatures, duties).
    fn run(disk: &mut Disk, regulator: &mut Regulator, gains: &Gains, hours: f32) -> (Vec<f32>, Vec<u8>) {
        let (mut temps, mut duties) = (Vec::new(), Vec::new());
        for _ in 0..(hours * 360.0) as usize {
            let t = disk.reading();
            let out = regulator.update(t, 38.0, 10.0, gains, curve().duty_at(t), 30.0, 100.0);
            disk.step(out.duty as f32, 10.0);
            temps.push(t);
            duties.push(out.duty);
        }
        (temps, duties)
    }

    #[test]
    fn holds_the_target_with_less_air_than_the_curve() {
        // Cool room: 38°C only needs ~35% while the curve would ask for 60%.
        let mut disk = Disk { temp: 41.0, hot: 45.0, cooling: 0.2, tau: 300.0 };
        let gains = Gains::new(Inertia::Slow, Reactivity::Normal);
        let mut regulator = Regulator::default();
        let (temps, duties) = run(&mut disk, &mut regulator, &gains, 4.0);

        let last_hour = temps.len() - 360;
        assert!(temps[last_hour..].iter().all(|t| (37.0..=39.0).contains(t)), "temperature not held");
        let settled = *duties.last().unwrap();
        assert!((30..=45).contains(&settled), "settled at {settled}%");
        let (lo, hi) = duties[last_hour..].iter().fold((100, 0), |(lo, hi), &d| (lo.min(d), hi.max(d)));
        assert!(hi - lo <= 3, "fans keep changing in steady state: {lo}..{hi}%");
    }

    #[test]
    fn speeds_up_when_more_cooling_is_needed() {
        // Hot room: ~70% is needed, more than the curve's 60% at 38°C.
        let mut disk = Disk { temp: 38.0, hot: 52.0, cooling: 0.2, tau: 300.0 };
        let gains = Gains::new(Inertia::Slow, Reactivity::Normal);
        let mut regulator = Regulator::default();
        let (temps, duties) = run(&mut disk, &mut regulator, &gains, 4.0);
        let last_hour = temps.len() - 360;
        assert!(temps[last_hour..].iter().all(|t| (37.0..=39.0).contains(t)));
        assert!(*duties.last().unwrap() >= 62);
    }

    #[test]
    fn emergency_enforces_the_curve() {
        let gains = Gains::new(Inertia::Slow, Reactivity::Normal);
        let mut regulator = Regulator::default();
        regulator.update(38.0, 38.0, 10.0, &gains, 60.0, 30.0, 100.0);
        // A sudden jump to 43°C (5°C over the target, margin 4°C): straight to the curve.
        let out = regulator.update(43.0, 38.0, 10.0, &gains, 94.0, 30.0, 100.0);
        assert!(out.emergency);
        assert_eq!(out.duty, 94);
    }

    #[test]
    fn slows_down_gently_and_respects_limits() {
        let gains = Gains::new(Inertia::Slow, Reactivity::Normal);
        let mut regulator = Regulator::default();
        let first = regulator.update(30.0, 38.0, 10.0, &gains, 70.0, 30.0, 100.0);
        assert_eq!(first.duty, 70);
        // Far below the target: at most max_down (1%) less per 10 s.
        let next = regulator.update(30.0, 38.0, 10.0, &gains, 70.0, 30.0, 100.0);
        assert_eq!(next.duty, 69);
        for _ in 0..200 {
            regulator.update(30.0, 38.0, 10.0, &gains, 70.0, 30.0, 100.0);
        }
        assert_eq!(regulator.update(30.0, 38.0, 10.0, &gains, 70.0, 30.0, 100.0).duty, 30);
    }

    #[test]
    fn deadband_keeps_the_duty_still() {
        let gains = Gains::new(Inertia::Fast, Reactivity::Normal);
        let mut regulator = Regulator::default();
        let first = regulator.update(60.0, 60.0, 10.0, &gains, 50.0, 20.0, 100.0);
        for t in [60.8, 59.3, 61.0, 60.2] {
            assert_eq!(regulator.update(t, 60.0, 10.0, &gains, 50.0, 20.0, 100.0), first);
        }
    }

    #[test]
    fn inertia_by_source() {
        assert_eq!(Inertia::of(&Source::Cpu), Inertia::Fast);
        assert_eq!(Inertia::of(&Source::parse("disks:hdd").unwrap()), Inertia::Slow);
        assert_eq!(Inertia::of(&Source::parse("ipmi:CPU Temp").unwrap()), Inertia::Fast);
        assert_eq!(Inertia::of(&Source::parse("hwmon:nvme/Composite").unwrap()), Inertia::Slow);
    }
}
