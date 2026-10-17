//! Temperature → fan duty curves.

use anyhow::bail;
use serde::{Deserialize, Serialize};

/// Points `(temperature °C, duty %)`, sorted by temperature. Duty is linearly interpolated
/// between points and held flat before the first and after the last one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct Curve(pub Vec<(f32, u8)>);

impl Curve {
    pub fn new(points: Vec<(f32, u8)>) -> Self {
        Self(points)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.0.is_empty() {
            bail!("a curve needs at least one point");
        }
        for (temp, duty) in &self.0 {
            if !temp.is_finite() || !(-20.0..=150.0).contains(temp) {
                bail!("temperature {temp} out of range (-20..150)");
            }
            if *duty > 100 {
                bail!("duty {duty}% out of range (0..100)");
            }
        }
        for pair in self.0.windows(2) {
            if pair[1].0 <= pair[0].0 {
                bail!("temperatures must be strictly increasing ({} then {})", pair[0].0, pair[1].0);
            }
        }
        Ok(())
    }

    /// Duty requested at `temp`.
    pub fn duty_at(&self, temp: f32) -> f32 {
        let points = &self.0;
        let Some(&(first_t, first_d)) = points.first() else {
            return 100.0;
        };
        if temp <= first_t {
            return first_d as f32;
        }
        for pair in points.windows(2) {
            let ((t0, d0), (t1, d1)) = (pair[0], pair[1]);
            if temp <= t1 {
                let ratio = (temp - t0) / (t1 - t0);
                return d0 as f32 + ratio * (d1 as f32 - d0 as f32);
            }
        }
        points.last().map_or(100.0, |&(_, d)| d as f32)
    }
}

/// Picks the next duty with hysteresis: speeding up is immediate, slowing down only happens
/// once the temperature has dropped `hysteresis` degrees below what the current duty needs.
pub fn next_duty(curve: &Curve, temp: f32, current: Option<u8>, hysteresis: f32) -> u8 {
    let target = curve.duty_at(temp).round().clamp(0.0, 100.0) as u8;
    let Some(current) = current else {
        return target;
    };
    if target >= current {
        return target;
    }
    // Duty we would want if the temperature were `hysteresis` degrees higher.
    let delayed = curve.duty_at(temp + hysteresis).round().clamp(0.0, 100.0) as u8;
    delayed.min(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> Curve {
        Curve::new(vec![(30.0, 30), (40.0, 50), (60.0, 100)])
    }

    #[test]
    fn interpolates_and_clamps() {
        let c = curve();
        assert_eq!(c.duty_at(10.0), 30.0);
        assert_eq!(c.duty_at(30.0), 30.0);
        assert_eq!(c.duty_at(35.0), 40.0);
        assert_eq!(c.duty_at(50.0), 75.0);
        assert_eq!(c.duty_at(90.0), 100.0);
    }

    #[test]
    fn validation() {
        assert!(curve().validate().is_ok());
        assert!(Curve::new(vec![]).validate().is_err());
        assert!(Curve::new(vec![(40.0, 50), (30.0, 60)]).validate().is_err());
        assert!(Curve::new(vec![(40.0, 120)]).validate().is_err());
    }

    #[test]
    fn hysteresis_delays_slowing_down() {
        let c = curve();
        // Heating to 40°C: immediate 50%.
        assert_eq!(next_duty(&c, 40.0, Some(30), 2.0), 50);
        // Cooling to 39°C: 39+2 = 41°C still wants >= 50%, keep 50%.
        assert_eq!(next_duty(&c, 39.0, Some(50), 2.0), 50);
        // Cooling to 36°C: behaves like 38°C = 46%.
        assert_eq!(next_duty(&c, 36.0, Some(50), 2.0), 46);
        // No hysteresis: follow the curve.
        assert_eq!(next_duty(&c, 36.0, Some(50), 0.0), 42);
        // First iteration: no previous duty.
        assert_eq!(next_duty(&c, 36.0, None, 2.0), 42);
    }
}
