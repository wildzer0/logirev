//! Wheel speed and smooth scrolling: the arithmetic, kept apart from the
//! Windows hook (`ui::windows::scroll`) so it can be tested anywhere.
//!
//! Wheel movement is measured in units of 1/120 of a notch. With a speed of
//! 1.5 every notch becomes 180 units; with smooth scrolling those units are
//! handed out over about a tenth of a second in shrinking steps, which
//! programs draw as a glide instead of a jump.

/// Below this many units left, smooth scrolling sends the rest at once.
const FINISH: f32 = 6.0;
/// Share of what is left sent at each step of smooth scrolling.
const STEP_SHARE: f32 = 0.3;

/// Turns wheel movements into the movements to send.
#[derive(Debug, Default)]
pub struct Wheel {
    /// Units still to send while gliding: `[vertical, horizontal]`.
    remaining: [f32; 2],
    /// Fractions of a unit not sent yet (kept so nothing is lost).
    carry: [f32; 2],
}

impl Wheel {
    /// A wheel movement of `delta` units on `axis` (0 = vertical,
    /// 1 = horizontal). Returns what to send right away.
    pub fn push(&mut self, axis: usize, delta: i32, percent: u16, smooth: bool) -> Option<i32> {
        let scaled = delta as f32 * f32::from(percent) / 100.0;
        if smooth {
            self.remaining[axis] += scaled;
            None
        } else {
            self.emit(axis, scaled)
        }
    }

    /// One step of gliding; call every ~10 ms while [`Wheel::gliding`].
    /// Returns `(axis, units)` to send.
    pub fn tick(&mut self) -> Vec<(usize, i32)> {
        let mut out = Vec::new();
        for axis in 0..2 {
            let left = self.remaining[axis];
            if left == 0.0 {
                continue;
            }
            let step = if left.abs() <= FINISH {
                left
            } else {
                left * STEP_SHARE
            };
            self.remaining[axis] -= step;
            let units = if self.remaining[axis] == 0.0 {
                // Last step: round what is left so the total is exact.
                let total = self.carry[axis] + step;
                self.carry[axis] = 0.0;
                Some(total.round() as i32).filter(|u| *u != 0)
            } else {
                self.emit(axis, step)
            };
            if let Some(units) = units {
                out.push((axis, units));
            }
        }
        out
    }

    /// Whether some smooth movement is still to be sent.
    pub fn gliding(&self) -> bool {
        self.remaining.iter().any(|r| *r != 0.0)
    }

    /// Whole units of `amount` plus the carried fraction.
    fn emit(&mut self, axis: usize, amount: f32) -> Option<i32> {
        let total = self.carry[axis] + amount;
        let units = total.trunc();
        self.carry[axis] = total - units;
        (units != 0.0).then_some(units as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_without_losing_fractions() {
        let mut w = Wheel::default();
        assert_eq!(w.push(0, 120, 150, false), Some(180));
        // 0.25 x 1 unit = 0.25 each time: one unit every fourth call.
        let sent: i32 = (0..8).filter_map(|_| w.push(1, 1, 25, false)).sum();
        assert_eq!(sent, 2);
    }

    #[test]
    fn glides_the_whole_amount() {
        let mut w = Wheel::default();
        assert_eq!(w.push(0, -120, 100, true), None);
        let mut total = 0;
        let mut steps = 0;
        while w.gliding() {
            total += w.tick().iter().map(|(_, u)| u).sum::<i32>();
            steps += 1;
            assert!(steps < 50);
        }
        assert_eq!(total, -120);
        assert!(steps > 3, "spread over several steps");
    }
}
