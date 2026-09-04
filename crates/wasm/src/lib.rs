//! C ABI over `tagpad-core`, for driving the session from JavaScript.
//!
//! Deliberately no wasm-bindgen. The surface is four verbs and JSON in and out,
//! which is small enough that hand-written glue is less machinery than a code
//! generator plus its CLI -- and it keeps the artifact a single file with no
//! build tooling on the far side.
//!
//! Convention: every call that returns data writes JSON into a thread-local
//! buffer and returns its pointer; the caller then reads `result_len()`. The
//! buffer lives until the next call that writes it, which is safe because
//! JavaScript is single-threaded and copies the bytes out immediately.

use std::cell::RefCell;
use tagpad_core::{Action, Decisions, Session, Task};

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Hand JavaScript a buffer it can write UTF-8 into. Paired with `dealloc`.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
/// `ptr` must come from `alloc` with the same `len`, and must not be reused.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
    }
}

fn put(json: String) -> *const u8 {
    RESULT.with(|r| {
        let mut r = r.borrow_mut();
        *r = json.into_bytes();
        r.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn result_len() -> usize {
    RESULT.with(|r| r.borrow().len())
}

/// # Safety
/// `ptr`/`len` must describe valid UTF-8 that JavaScript still owns.
unsafe fn str_from(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
}

/// Create the session. Returns 1 on success, 0 if the task JSON was unusable --
/// a front end that cannot parse its task should say so, not draw an empty card.
///
/// # Safety
/// Both pointer/length pairs must describe valid UTF-8 buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn session_new(
    task_ptr: *const u8,
    task_len: usize,
    saved_ptr: *const u8,
    saved_len: usize,
) -> u32 {
    let Ok(task) = serde_json::from_str::<Task>(&unsafe { str_from(task_ptr, task_len) }) else {
        return 0;
    };
    // Restoring is best-effort: corrupt saved progress must not stop a labeller
    // from starting fresh.
    let saved = serde_json::from_str::<Decisions>(&unsafe { str_from(saved_ptr, saved_len) })
        .unwrap_or_default();
    // An empty task is a load failure, not a session with nothing in it.
    let Some(session) = Session::new(task, saved) else {
        return 0;
    };
    SESSION.with(|s| *s.borrow_mut() = Some(session));
    1
}

/// Apply an action. `kind` mirrors `Action`; `arg` is its payload where one
/// applies. Returns a pointer to JSON: `{"recorded": "<verdict>"|null}` so the
/// front end knows when to flash and rumble without diffing state itself.
#[unsafe(no_mangle)]
pub extern "C" fn session_apply(kind: u32, arg: i32) -> *const u8 {
    let action = match kind {
        0 => Action::Choose(arg.max(0) as usize),
        1 => Action::Assign(arg.max(0) as usize),
        2 => Action::Select(arg.max(0) as usize),
        3 => Action::Confirm,
        4 => Action::Cancel,
        5 => Action::Move(arg as isize),
        _ => return put("{\"recorded\":null}".into()),
    };
    let recorded = SESSION.with(|s| s.borrow_mut().as_mut().and_then(|s| s.apply(action)));
    put(serde_json::json!({ "recorded": recorded }).to_string())
}

/// Everything needed to draw one frame.
#[unsafe(no_mangle)]
pub extern "C" fn session_view() -> *const u8 {
    let json = SESSION.with(|s| {
        let b = s.borrow();
        let Some(session) = b.as_ref() else {
            return "null".to_string();
        };
        let v = session.view();
        let (mode, cursor, assigned) = match v.mode {
            tagpad_core::Mode::Verdict => ("verdict", 0usize, Vec::new()),
            tagpad_core::Mode::Partition { cursor, assigned } => {
                ("partition", *cursor, assigned.clone())
            }
        };
        serde_json::json!({
            "card": {
                "id": v.card.id, "question": v.card.question, "items": v.card.items,
                "flag": v.card.flag,
                "options": v.card.options.iter().map(|o| serde_json::json!({
                    "id": o.id, "label": o.label, "hint": o.hint,
                    "opens": o.opens_partition(),
                })).collect::<Vec<_>>(),
            },
            "position": v.position, "total": v.total, "done": v.done,
            "recorded": v.recorded,
            "mode": mode, "cursor": cursor, "assigned": assigned,
        })
        .to_string()
    });
    put(json)
}

/// The output blob, and what gets persisted between sessions.
#[unsafe(no_mangle)]
pub extern "C" fn session_output() -> *const u8 {
    let json = SESSION.with(|s| {
        let b = s.borrow();
        b.as_ref()
            .map(|s| serde_json::to_string_pretty(&s.output("human")).unwrap_or_default())
            .unwrap_or_else(|| "null".into())
    });
    put(json)
}

#[unsafe(no_mangle)]
pub extern "C" fn session_decisions() -> *const u8 {
    let json = SESSION.with(|s| {
        let b = s.borrow();
        b.as_ref()
            .map(|s| serde_json::to_string(s.decisions()).unwrap_or_default())
            .unwrap_or_else(|| "{}".into())
    });
    put(json)
}

// ---- input -----------------------------------------------------------------
//
// The host's entire job: copy this frame's raw button and axis values in. Edge
// detection, deadzones, and which button means which action all happen in Rust,
// so the browser build and the native build cannot drift on what a press is.

thread_local! {
    static PAD: RefCell<tagpad_core::input::Gamepad> =
        RefCell::new(tagpad_core::input::Gamepad::new());
}

/// Feed one frame. Returns JSON: `{"acted":bool,"assigned":bool,"recorded":...}`
/// -- enough for the host to know whether to redraw, and whether to rumble,
/// without inspecting session state itself.
///
/// # Safety
/// Both pointers must address `len` little-endian f32 values in wasm memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn input_frame(
    buttons: *const f32,
    n_buttons: usize,
    axes: *const f32,
    n_axes: usize,
) -> *const u8 {
    let b = if buttons.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(buttons, n_buttons) }
    };
    let a = if axes.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(axes, n_axes) }
    };
    let pressed = PAD.with(|p| p.borrow_mut().frame(b, a));
    apply_buttons(&pressed)
}

fn apply_buttons(pressed: &[tagpad_core::input::Button]) -> *const u8 {
    let mut acted = false;
    let mut assigned = false;
    let mut recorded: Option<String> = None;
    SESSION.with(|s| {
        let mut b = s.borrow_mut();
        let Some(session) = b.as_mut() else { return };
        for button in pressed {
            // The mode is re-read per button: a press can change the mode, and
            // the next press in the same frame must be read against the new one.
            let Some(action) = tagpad_core::input::action_for(*button, session.view().mode) else {
                continue;
            };
            acted = true;
            assigned |= matches!(action, tagpad_core::Action::Assign(_));
            if let Some(v) = session.apply(action) {
                recorded = Some(v);
            }
        }
    });
    put(
        serde_json::json!({ "acted": acted, "assigned": assigned, "recorded": recorded })
            .to_string(),
    )
}

/// A key press, named as a browser names it, lowercased.
///
/// # Safety
/// `ptr`/`len` must describe valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn key_frame(ptr: *const u8, len: usize) -> *const u8 {
    let key = unsafe { str_from(ptr, len) };
    let mut acted = false;
    let mut assigned = false;
    let mut recorded: Option<String> = None;
    SESSION.with(|s| {
        let mut b = s.borrow_mut();
        let Some(session) = b.as_mut() else { return };
        if let Some(action) = tagpad_core::input::action_for_key(&key, session.view().mode) {
            acted = true;
            assigned = matches!(action, tagpad_core::Action::Assign(_));
            recorded = session.apply(action);
        }
    });
    put(
        serde_json::json!({ "acted": acted, "assigned": assigned, "recorded": recorded })
            .to_string(),
    )
}

/// Clear held state on disconnect, so a reconnect does not swallow the first
/// press of a button that was down when the pad vanished.
#[unsafe(no_mangle)]
pub extern "C" fn input_reset() {
    PAD.with(|p| p.borrow_mut().reset());
}
