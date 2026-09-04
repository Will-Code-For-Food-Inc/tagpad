//! Input, as a pure state machine.
//!
//! The Gamepad API has no press events -- a host hands over a snapshot of
//! button values, and a press is only observable as a difference between two
//! frames. That difference is *logic*, not platform glue, so it lives here
//! rather than in each front end.
//!
//! The payoff is that a browser and a native build cannot disagree about what
//! a press is, or which button means which action. A host's whole job becomes
//! "copy in the current button values"; everything after that is shared, and
//! testable without a window or a physical controller.

use crate::{Action, Mode};

/// Physical positions, not vendor labels: index 0 is A on Xbox and Cross on
/// `PlayStation`. Binding by position keeps one code path for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    South,
    East,
    West,
    North,
    L1,
    R1,
    L2,
    R2,
    Select,
    Start,
    L3,
    R3,
    Up,
    Down,
    Left,
    Right,
}

impl Button {
    pub const ALL: [Self; 16] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::L1,
        Self::R1,
        Self::L2,
        Self::R2,
        Self::Select,
        Self::Start,
        Self::L3,
        Self::R3,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
    ];
}

/// Analog triggers report a float rather than a boolean, and engines disagree
/// about whether a trigger is a button or an axis at all -- so treat anything
/// past a threshold as pressed rather than trusting a `pressed` flag.
const PRESS_THRESHOLD: f32 = 0.5;
/// Generous: this is menu navigation, not aiming.
const STICK_DEADZONE: f32 = 0.6;

#[derive(Debug, Default)]
pub struct Gamepad {
    down: [bool; 16],
}

impl Gamepad {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one frame of raw values. Returns the buttons that went down *this*
    /// frame; a held button fires once, not sixty times a second.
    ///
    /// `axes` is the left stick, which doubles as the d-pad: pads without a
    /// real d-pad are common and a labeller should not have to know which kind
    /// they are holding.
    pub fn frame(&mut self, buttons: &[f32], axes: &[f32]) -> Vec<Button> {
        let mut now = [false; 16];
        for (i, slot) in now.iter_mut().enumerate() {
            *slot = buttons.get(i).is_some_and(|v| *v > PRESS_THRESHOLD);
        }
        if let Some(y) = axes.get(1) {
            if *y < -STICK_DEADZONE {
                now[12] = true;
            }
            if *y > STICK_DEADZONE {
                now[13] = true;
            }
        }
        // Zip rather than index: all three arrays are the same fixed length, and
        // saying so with iterators means no bounds check can fail here.
        let pressed = Button::ALL
            .iter()
            .zip(now.iter())
            .zip(self.down.iter())
            .filter_map(|((button, &is_down), &was_down)| (is_down && !was_down).then_some(*button))
            .collect();
        self.down = now;
        pressed
    }

    /// Forget held state -- call on disconnect so a reconnect does not swallow
    /// the first press of a button that was down when the pad vanished.
    pub const fn reset(&mut self) {
        self.down = [false; 16];
    }
}

/// What a button means, given what the session is currently doing.
///
/// One table, both front ends. A press means the same thing everywhere, and
/// changing a binding changes it everywhere at once.
#[must_use]
pub const fn action_for(button: Button, mode: &Mode) -> Option<Action> {
    use Button::{Down, East, L1, North, R1, Select, South, Start, Up, West};
    match mode {
        Mode::Partition { .. } => match button {
            South => Some(Action::Assign(0)),
            East => Some(Action::Assign(1)),
            West => Some(Action::Assign(2)),
            North => Some(Action::Assign(3)),
            Start => Some(Action::Confirm),
            Select => Some(Action::Cancel),
            L1 | Up => Some(Action::Move(-1)),
            R1 | Down => Some(Action::Move(1)),
            _ => None,
        },
        Mode::Verdict => match button {
            South => Some(Action::Choose(0)),
            East => Some(Action::Choose(1)),
            North => Some(Action::Choose(2)),
            L1 | Up => Some(Action::Move(-1)),
            R1 | Down => Some(Action::Move(1)),
            _ => None,
        },
    }
}

/// Keyboard equivalent. Keys arrive already lowercased, as a browser reports
/// them; native hosts normalise to the same names.
pub fn action_for_key(key: &str, mode: &Mode) -> Option<Action> {
    match mode {
        Mode::Partition { .. } => match key {
            // Pointer taps route through this table too, so a tap and a
            // keypress cannot diverge in meaning.
            k if k.starts_with("select:") => k[7..].parse::<usize>().ok().map(Action::Select),
            "1" => Some(Action::Assign(0)),
            "2" => Some(Action::Assign(1)),
            "3" => Some(Action::Assign(2)),
            "4" => Some(Action::Assign(3)),
            "enter" => Some(Action::Confirm),
            "escape" => Some(Action::Cancel),
            "arrowup" | "arrowleft" => Some(Action::Move(-1)),
            "arrowdown" | "arrowright" => Some(Action::Move(1)),
            _ => None,
        },
        Mode::Verdict => match key {
            "k" => Some(Action::Choose(0)),
            "s" => Some(Action::Choose(1)),
            "u" => Some(Action::Choose(2)),
            "arrowleft" => Some(Action::Move(-1)),
            "arrowright" => Some(Action::Move(1)),
            _ => None,
        },
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn held(i: usize) -> Vec<f32> {
        let mut v = vec![0.0; 16];
        v[i] = 1.0;
        v
    }

    #[test]
    fn a_held_button_fires_once() {
        let mut pad = Gamepad::new();
        assert_eq!(pad.frame(&held(0), &[]), vec![Button::South]);
        assert_eq!(pad.frame(&held(0), &[]), vec![]);
        assert_eq!(pad.frame(&[0.0; 16], &[]), vec![]);
        assert_eq!(pad.frame(&held(0), &[]), vec![Button::South]);
    }

    #[test]
    fn analog_triggers_count_as_pressed_past_the_threshold() {
        let mut pad = Gamepad::new();
        let mut v = vec![0.0; 16];
        v[6] = 0.4;
        assert_eq!(pad.frame(&v, &[]), vec![]);
        v[6] = 0.9;
        assert_eq!(pad.frame(&v, &[]), vec![Button::L2]);
    }

    #[test]
    fn the_left_stick_acts_as_the_dpad() {
        let mut pad = Gamepad::new();
        assert_eq!(pad.frame(&[0.0; 16], &[0.0, -0.9]), vec![Button::Up]);
        assert_eq!(pad.frame(&[0.0; 16], &[0.0, -0.3]), vec![]);
        assert_eq!(pad.frame(&[0.0; 16], &[0.0, 0.9]), vec![Button::Down]);
    }

    #[test]
    fn reset_lets_a_still_held_button_fire_again() {
        let mut pad = Gamepad::new();
        pad.frame(&held(0), &[]);
        pad.reset();
        assert_eq!(pad.frame(&held(0), &[]), vec![Button::South]);
    }

    #[test]
    fn short_button_arrays_do_not_panic() {
        let mut pad = Gamepad::new();
        assert_eq!(pad.frame(&[1.0], &[]), vec![Button::South]);
    }

    #[test]
    fn south_means_assign_while_partitioning_and_choose_otherwise() {
        let part = Mode::Partition {
            cursor: 0,
            assigned: vec![0],
        };
        assert_eq!(action_for(Button::South, &part), Some(Action::Assign(0)));
        assert_eq!(
            action_for(Button::South, &Mode::Verdict),
            Some(Action::Choose(0))
        );
    }
}
