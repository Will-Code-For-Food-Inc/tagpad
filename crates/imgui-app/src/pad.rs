//! Reading a controller on Linux, without libudev.
//!
//! `evdev` is pure Rust and talks to `/dev/input/event*` directly. Steam Input
//! presents its virtual controller as an evdev device like any other, so this
//! works on a Deck with a custom action set as well as with a pad plugged
//! straight in.
//!
//! This file does no interpretation: it produces the same 16 button values and
//! axis values the browser hands to `tagpad_core::input::Gamepad`. Edge
//! detection, deadzones and bindings all happen there, once, for both builds.

use evdev::{AbsoluteAxisCode, Device, EventSummary, KeyCode};

/// Linux button codes in our positional order.
/// Index 0 is the south face button -- A on Xbox, Cross on `PlayStation`.
const BUTTONS: [KeyCode; 12] = [
    KeyCode::BTN_SOUTH,
    KeyCode::BTN_EAST,
    KeyCode::BTN_WEST,
    KeyCode::BTN_NORTH,
    KeyCode::BTN_TL,
    KeyCode::BTN_TR,
    KeyCode::BTN_TL2,
    KeyCode::BTN_TR2,
    KeyCode::BTN_SELECT,
    KeyCode::BTN_START,
    KeyCode::BTN_THUMBL,
    KeyCode::BTN_THUMBR,
];

const AXIS_MAX: f32 = 32767.0;

/// Stick position as -1.0..=1.0. Axis values are 16-bit, so the conversion is
/// exact -- f32 carries 24 bits of mantissa.
fn axis(raw: i32) -> f32 {
    f32::from(i16::try_from(raw).unwrap_or(if raw < 0 { i16::MIN } else { i16::MAX })) / AXIS_MAX
}

pub struct Pad {
    device: Option<Device>,
    buttons: [f32; 16],
    axes: [f32; 8],
}

impl std::fmt::Debug for Pad {
    // `Device` is not Debug and the raw button array is noise; what a reader
    // wants from a Pad is whether one is attached and what it is.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pad")
            .field("connected", &self.device.is_some())
            .field("name", &self.name())
            .finish_non_exhaustive()
    }
}

impl Pad {
    /// Take the first device that reports a south face button. That key is what
    /// the Linux gamepad spec requires of anything calling itself a gamepad, so
    /// it distinguishes a pad from a keyboard or a touchpad without a database.
    pub fn open() -> Self {
        let device = evdev::enumerate().map(|(_, d)| d).find(|d| {
            d.supported_keys()
                .is_some_and(|k| k.contains(KeyCode::BTN_SOUTH))
        });
        if let Some(d) = &device {
            let _ = d.set_nonblocking(true);
        }
        Self {
            device,
            buttons: [0.0; 16],
            axes: [0.0; 8],
        }
    }

    pub fn name(&self) -> Option<String> {
        self.device
            .as_ref()
            .and_then(|d| d.name().map(ToOwned::to_owned))
    }

    /// Drain pending events into the current button and axis state.
    ///
    /// Non-blocking, so an empty read is the normal case rather than an error.
    /// A device that disappears is dropped rather than retried every frame --
    /// the session stays usable on the keyboard either way.
    pub fn poll(&mut self) -> (&[f32], &[f32]) {
        let mut gone = false;
        if let Some(device) = &mut self.device {
            match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        match event.destructure() {
                            EventSummary::Key(_, code, value) => {
                                if let Some(i) = BUTTONS.iter().position(|b| *b == code)
                                    && let Some(slot) = self.buttons.get_mut(i)
                                {
                                    *slot = if value == 0 { 0.0 } else { 1.0 };
                                }
                            }
                            // The d-pad is a hat, not buttons, on most pads.
                            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_HAT0Y, v) => {
                                if let Some(s) = self.buttons.get_mut(12) {
                                    *s = f32::from(v < 0);
                                }
                                if let Some(s) = self.buttons.get_mut(13) {
                                    *s = f32::from(v > 0);
                                }
                            }
                            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_HAT0X, v) => {
                                if let Some(s) = self.buttons.get_mut(14) {
                                    *s = f32::from(v < 0);
                                }
                                if let Some(s) = self.buttons.get_mut(15) {
                                    *s = f32::from(v > 0);
                                }
                            }
                            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_X, v) => {
                                self.axes[0] = axis(v);
                            }
                            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_Y, v) => {
                                self.axes[1] = axis(v);
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => gone = true,
            }
        }
        if gone {
            self.device = None;
            self.buttons = [0.0; 16];
            self.axes = [0.0; 8];
        }
        (&self.buttons, &self.axes)
    }
}
