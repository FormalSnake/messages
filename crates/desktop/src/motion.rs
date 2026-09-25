//! Durations and easing curves, ported from `apps/desktop/src/ui/motion.tsx`.
//!
//! gpui-pre's `Animation` (`elements/animation.rs`) only ships linear, quadratic,
//! ease-in-out and a quint ease-out; it has no cubic-bezier curve, which is what
//! the TS app's easings are. `cubic_bezier` below solves one, the same curve CSS
//! `cubic-bezier()` and gpuix's `MotionEase` describe.

use std::time::Duration;

/// Everything stays under 300ms: in a chat client motion is feedback, never a show.
pub const DURATION_FAST: Duration = Duration::from_millis(120);
/// Enter transitions and small reveals.
pub const DURATION_BASE: Duration = Duration::from_millis(180);
/// The details panel sliding in.
pub const DURATION_PANEL: Duration = Duration::from_millis(280);

/// A strong ease-out: fast start, long settle. Fades and small reveals use it.
pub const EASE_OUT: (f32, f32, f32, f32) = (0.23, 1.0, 0.32, 1.0);
pub const EASE_IN_OUT: (f32, f32, f32, f32) = (0.77, 0.0, 0.175, 1.0);
/// The iOS sheet curve, for anything that travels a long way. A strong
/// ease-out covers most of a 280px slide in its first few frames, which reads
/// as a jump and a crawl however high the frame rate.
pub const EASE_DRAWER: (f32, f32, f32, f32) = (0.32, 0.72, 0.0, 1.0);

/// Builds a `Fn(f32) -> f32` for `Animation::with_easing` from the four
/// control points of a `cubic-bezier(x1, y1, x2, y2)` curve. Solves for the
/// bezier parameter `t` at a given `x` by bisection (8 iterations is enough
/// for the sub-pixel precision an animation needs), then evaluates `y` at `t`.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 {
    move |x: f32| {
        let bezier = |t: f32, p1: f32, p2: f32| {
            let mt = 1.0 - t;
            3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t
        };

        let mut lo = 0.0f32;
        let mut hi = 1.0f32;
        let mut t = x;
        for _ in 0..8 {
            let guess = bezier(t, x1, x2);
            if guess < x {
                lo = t;
            } else {
                hi = t;
            }
            t = (lo + hi) / 2.0;
        }
        bezier(t, y1, y2)
    }
}
