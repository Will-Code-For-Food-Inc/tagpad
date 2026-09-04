//! Drawing, and nothing else.
//!
//! Every decision this file could make is already made in `tagpad_core`: what a
//! press means, when partition mode opens, where the cursor lands. What is left
//! is layout and colour, which is exactly the amount a front end should own.

use imgui::{Condition, StyleColor, Ui};
use tagpad_core::{Action, Mode, Session};

/// Small-count division without a lossy cast at every call site.
fn ratio(done: usize, total: usize) -> f32 {
    let total = u16::try_from(total).unwrap_or(u16::MAX).max(1);
    let done = u16::try_from(done).unwrap_or(u16::MAX);
    f32::from(done) / f32::from(total)
}

const FACE: [&str; 4] = ["A", "B", "X", "Y"];
const GROUP_COLOURS: [[f32; 4]; 4] = [
    [0.15, 0.42, 0.38, 1.0], // teal
    [0.66, 0.32, 0.18, 1.0], // clay
    [0.23, 0.21, 0.52, 1.0], // indigo
    [0.54, 0.42, 0.09, 1.0], // amber
];

/// Draw one frame. Returns an action if the user clicked something -- pointer
/// input routes through the same `Action` type as a button press, so a click
/// and a press cannot come to mean different things.
pub fn draw(ui: &Ui, session: &Session, pad_name: Option<&str>) -> Option<Action> {
    let mut action = None;
    let view = session.view();
    let [w, h] = ui.io().display_size;

    ui.window("tagpad")
        .position([0.0, 0.0], Condition::Always)
        .size([w, h], Condition::Always)
        .title_bar(false)
        .resizable(false)
        .movable(false)
        .bring_to_front_on_focus(false)
        .build(|| {
            // Header: position, progress, which pad is attached.
            ui.text_disabled(format!("{} / {}", view.position + 1, view.total));
            ui.same_line();
            // Card counts are small enough that the f32 conversion is exact; the cast
            // is narrowing only in principle.
            let frac = ratio(view.done, view.total);
            imgui::ProgressBar::new(frac)
                .size([w - 320.0, 6.0])
                .overlay_text("")
                .build(ui);
            ui.same_line();
            match pad_name {
                Some(name) => ui.text_disabled(name),
                None => ui.text_disabled("no pad - keyboard"),
            }
            ui.separator();
            ui.spacing();

            ui.text_disabled(&view.card.id);
            ui.same_line();
            ui.text(&view.card.question);
            if matches!(view.mode, Mode::Verdict) {
                if let Some(flag) = &view.card.flag {
                    ui.same_line();
                    ui.text_colored(GROUP_COLOURS[3], flag);
                }
            } else {
                ui.same_line();
                ui.text_colored(GROUP_COLOURS[3], "assign each item to a group");
            }
            ui.spacing();

            match view.mode {
                Mode::Partition { cursor, assigned } => {
                    action = partition_items(ui, view.card.items.as_slice(), *cursor, assigned)
                        .or(action);
                }
                Mode::Verdict => {
                    for item in &view.card.items {
                        ui.bullet_text(item);
                    }
                }
            }

            // Controls pinned to the bottom, so they sit in the same place on
            // every card and the eye never has to hunt for them.
            let controls_h = 78.0;
            ui.set_cursor_pos([ui.cursor_pos()[0], h - controls_h]);
            ui.separator();
            match view.mode {
                Mode::Partition { assigned, .. } => {
                    action = partition_controls(ui, assigned, w).or(action);
                }
                Mode::Verdict => {
                    action = verdict_controls(ui, session, w).or(action);
                }
            }
        });

    action
}

fn partition_items(ui: &Ui, items: &[String], cursor: usize, assigned: &[usize]) -> Option<Action> {
    let mut action = None;
    for (n, item) in items.iter().enumerate() {
        let group = assigned.get(n).copied().unwrap_or(0);
        let colour = GROUP_COLOURS
            .get(group)
            .copied()
            .unwrap_or(GROUP_COLOURS[0]);
        let tint = ui.push_style_color(StyleColor::Button, colour);
        if ui.button(format!("{}##g{n}", group + 1)) {
            action = Some(Action::Select(n));
        }
        tint.pop();
        ui.same_line();
        if n == cursor {
            ui.text_colored(GROUP_COLOURS[2], item);
        } else {
            ui.text(item);
        }
    }
    action
}

fn partition_controls(ui: &Ui, assigned: &[usize], w: f32) -> Option<Action> {
    let mut action = None;
    let width = (w - 60.0) / 6.0;
    for (g, face) in FACE.iter().enumerate() {
        let colour = GROUP_COLOURS.get(g).copied().unwrap_or(GROUP_COLOURS[0]);
        let tint = ui.push_style_color(StyleColor::Button, colour);
        if ui.button_with_size(format!("{}  {face}", g + 1), [width, 46.0]) {
            action = Some(Action::Assign(g));
        }
        tint.pop();
        ui.same_line();
    }
    let groups: std::collections::BTreeSet<_> = assigned.iter().collect();
    if ui.button_with_size(
        format!(
            "Confirm  {} group{}",
            groups.len(),
            if groups.len() == 1 { "" } else { "s" }
        ),
        [width, 46.0],
    ) {
        action = Some(Action::Confirm);
    }
    ui.same_line();
    if ui.button_with_size("Cancel", [width, 46.0]) {
        action = Some(Action::Cancel);
    }
    action
}

fn verdict_controls(ui: &Ui, session: &Session, w: f32) -> Option<Action> {
    let mut action = None;
    let view = session.view();
    let count = view.card.options.len().max(1);
    let width = (w - 40.0) / f32::from(u8::try_from(count).unwrap_or(1).max(1));
    for (n, option) in view.card.options.iter().enumerate() {
        let chosen = view.recorded.is_some_and(|d| d.verdict == option.id);
        let tint = chosen.then(|| ui.push_style_color(StyleColor::Button, GROUP_COLOURS[0]));
        let face = FACE.get(n).copied().unwrap_or("");
        let label = option.hint.as_ref().map_or_else(
            || format!("{face}   {}", option.label),
            |hint| format!("{face}   {}\n{hint}", option.label),
        );
        if ui.button_with_size(format!("{label}##opt{n}"), [width, 46.0]) {
            action = Some(Action::Choose(n));
        }
        if let Some(t) = tint {
            t.pop();
        }
        ui.same_line();
    }
    action
}
