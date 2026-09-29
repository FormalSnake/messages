//! The trackpad reply gesture from Messages on macOS: two fingers dragged to
//! the right over a message pull the bubble along, and letting go past
//! `REPLY_AT` replies to it.
//!
//! GPUI hands a two-finger drag over as scroll wheel events. macOS brackets
//! them with Started and Ended phases, then keeps sending momentum as Moved;
//! Wayland and X11 send nothing but Moved. So a gesture also starts after a
//! gap of `QUIET` in the stream and ends when the stream goes quiet that long,
//! and whatever follows an end without such a gap is momentum and ignored.

use std::time::{Duration, Instant};

use gpui_kit::TouchPhase;

/// How far the bubble has to travel before letting go replies.
pub const REPLY_AT: f32 = 56.;
pub const QUIET: Duration = Duration::from_millis(120);
/// Travel before the gesture commits to an axis; a vertical scroll that
/// wobbles sideways never moves the bubble.
const LOCK_AFTER: f32 = 8.;
/// Past `REPLY_AT` the bubble moves this fraction of the fingers' travel.
const RESISTANCE: f32 = 0.3;
const MAX_OFFSET: f32 = 96.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Undecided,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Not this gesture's event: let the thread scroll with it.
    Pass,
    /// The bubble moved: repaint and keep the event from the list.
    Track,
    Release { reply: bool },
}

pub struct Swipe {
    axis: Axis,
    travel: (f32, f32),
    last: Option<Instant>,
    done: bool,
}

impl Default for Swipe {
    fn default() -> Self {
        Swipe { axis: Axis::Undecided, travel: (0., 0.), last: None, done: true }
    }
}

impl Swipe {
    pub fn feed(&mut self, dx: f32, dy: f32, phase: TouchPhase, now: Instant) -> Step {
        let fresh = phase == TouchPhase::Started || self.last.is_none_or(|last| now - last >= QUIET);
        self.last = Some(now);
        if fresh {
            *self = Swipe { axis: Axis::Undecided, travel: (0., 0.), last: Some(now), done: false };
        }
        if self.done {
            return if self.axis == Axis::Horizontal { Step::Track } else { Step::Pass };
        }
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.done = true;
            return match self.axis {
                Axis::Horizontal => Step::Release { reply: phase == TouchPhase::Ended && self.travel.0 >= REPLY_AT },
                _ => Step::Pass,
            };
        }
        self.travel.0 += dx;
        self.travel.1 += dy;
        if self.axis == Axis::Undecided && self.travel.0.abs().max(self.travel.1.abs()) >= LOCK_AFTER {
            self.axis = if self.travel.0 > 2. * self.travel.1.abs() { Axis::Horizontal } else { Axis::Vertical };
        }
        if self.axis == Axis::Horizontal { Step::Track } else { Step::Pass }
    }

    /// Called once the stream may have gone quiet; ends a gesture no Ended
    /// phase will close.
    pub fn expire(&mut self, now: Instant) -> Option<bool> {
        let quiet = self.last.is_some_and(|last| now - last >= QUIET);
        if self.done || self.axis != Axis::Horizontal || !quiet {
            return None;
        }
        self.done = true;
        Some(self.travel.0 >= REPLY_AT)
    }

    pub fn active(&self) -> bool {
        !self.done && self.axis == Axis::Horizontal
    }

    /// Where the bubble sits, following the fingers up to `REPLY_AT` and
    /// dragging behind them after.
    pub fn offset(&self) -> f32 {
        if !self.active() {
            return 0.;
        }
        let x = self.travel.0.max(0.);
        if x <= REPLY_AT { x } else { (REPLY_AT + (x - REPLY_AT) * RESISTANCE).min(MAX_OFFSET) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn a_rightward_swipe_past_the_threshold_replies_on_ended() {
        let start = Instant::now();
        let mut swipe = Swipe::default();
        assert_eq!(swipe.feed(0., 0., TouchPhase::Started, start), Step::Pass);
        assert_eq!(swipe.feed(10., 1., TouchPhase::Moved, at(start, 8)), Step::Track);
        swipe.feed(60., 0., TouchPhase::Moved, at(start, 16));
        assert!(swipe.offset() > REPLY_AT && swipe.offset() < 70.);
        assert_eq!(swipe.feed(0., 0., TouchPhase::Ended, at(start, 24)), Step::Release { reply: true });
        assert_eq!(swipe.offset(), 0.);
    }

    #[test]
    fn a_short_swipe_springs_back_without_replying() {
        let start = Instant::now();
        let mut swipe = Swipe::default();
        swipe.feed(20., 0., TouchPhase::Moved, start);
        assert_eq!(swipe.feed(0., 0., TouchPhase::Ended, at(start, 8)), Step::Release { reply: false });
    }

    #[test]
    fn vertical_and_leftward_scrolls_pass_through() {
        let start = Instant::now();
        let mut swipe = Swipe::default();
        swipe.feed(3., 12., TouchPhase::Moved, start);
        assert_eq!(swipe.feed(40., 0., TouchPhase::Moved, at(start, 8)), Step::Pass);
        let mut swipe = Swipe::default();
        assert_eq!(swipe.feed(-30., 0., TouchPhase::Moved, at(start, 500)), Step::Pass);
    }

    #[test]
    fn momentum_after_ended_is_swallowed_until_a_gap() {
        let start = Instant::now();
        let mut swipe = Swipe::default();
        swipe.feed(80., 0., TouchPhase::Moved, start);
        swipe.feed(0., 0., TouchPhase::Ended, at(start, 8));
        assert_eq!(swipe.feed(-15., 0., TouchPhase::Moved, at(start, 24)), Step::Track);
        assert_eq!(swipe.offset(), 0.);
        assert_eq!(swipe.feed(0., 20., TouchPhase::Moved, at(start, 400)), Step::Pass);
    }

    #[test]
    fn a_stream_without_phases_ends_when_it_goes_quiet() {
        let start = Instant::now();
        let mut swipe = Swipe::default();
        swipe.feed(70., 0., TouchPhase::Moved, start);
        assert_eq!(swipe.expire(at(start, 60)), None);
        assert_eq!(swipe.expire(at(start, 130)), Some(true));
        assert_eq!(swipe.expire(at(start, 260)), None);
    }
}
