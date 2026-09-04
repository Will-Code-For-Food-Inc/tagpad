# tagpad

A controller-driven client for labelling datasets. You hand it a task file; it
hands you back judgments.

The bottleneck in every human-labelled dataset is attention, not throughput. A
person can render a few hundred judgments before quality degrades, and most of
that degradation is ergonomic: dense screens, a hand moving between mouse and
keyboard, losing your place. A controller removes all three.

Nothing here is specialised to a domain. Items and verdicts come from JSON, and
labelled output goes back out as JSON.

## Layout

```
crates/core/       domain, session state machine, input state machine.
                   serde only -- no UI, no windowing, no platform.
                   Builds for wasm32-unknown-unknown.
crates/wasm/       C ABI over core, for the browser build. No wasm-bindgen.
crates/imgui-app/  native front end (Steam Deck, Windows). In progress.
web/               the browser front end: shell, glue, app, build script.
```

The split is load-bearing. `core` owns everything that decides *what happens* --
when partition mode opens, what a button press means, when a held button counts
as a new press. A front end decides only what it looks like and which physical
input produced an action. Two front ends, one state machine, no way for them to
disagree about what a label means.

## Build

```sh
cargo test -p tagpad-core                                       # 16 tests, no browser needed
cargo build -p tagpad-wasm --target wasm32-unknown-unknown --release
python3 web/build.py tasks/example.json                         # -> dist/index.html
```

`dist/index.html` is self-contained: the `.wasm` is inlined as base64, so there
is no fetch and no server required. Open it directly.

## Task format

```json
{
  "version": "run-2026-09-04",
  "cards": [
    {
      "id": "alert-pair-118",
      "question": "Do these describe one incident?",
      "items": ["auth failure burst on host A", "credential stuffing against A"],
      "options": [
        { "id": "same",   "label": "Same" },
        { "id": "split",  "label": "Split", "opens": "partition" },
        { "id": "unsure", "label": "Unsure" }
      ]
    }
  ]
}
```

`opens: "partition"` is the only special value: picking that option sends the
labeller into group assignment instead of recording immediately. Everything
else is yours to name.

## Input

| | verdict mode | partition mode |
|---|---|---|
| A / `k` | first option | assign group 1 |
| B / `s` | second option | assign group 2 |
| X | — | assign group 3 |
| Y / `u` | third option | assign group 4 |
| Start / `Enter` | — | confirm |
| Select / `Esc` | — | cancel |
| L1 R1 / arrows | previous, next card | move the cursor |

Pointer taps route through the same binding table as the keyboard, so a tap and
a keypress cannot diverge in meaning.

### Two decisions worth knowing about

**A button means a judgment, not agreement.** A always means *same*; B always
means *split*. Binding the first button to "the system was right" inverts its
meaning depending on what the system proposed, and produces wrong labels
silently through muscle memory. We shipped that bug and caught it in the first
real session.

**Group assignment, not splitting at the gaps.** Breaking up a set walks the
items and takes one face button per item, so non-contiguous groupings cost what
adjacent ones cost. The obvious alternative -- a cursor between items, cutting
the list into runs -- can only express contiguous groups. In the first real
labelling session, three of five partitions were non-contiguous.

## Not built

Multi-labeller trust: several independent labels per item, inter-rater
agreement to catch someone mashing one button through a queue, gold items
salted in to score each labeller. That is a server, and it is most of the work
of any real labelling system. This is the client -- the easy half.

## Checks

```sh
git config core.hooksPath hooks   # once, after cloning
```

`hooks/pre-commit` runs fmt, clippy with `-D warnings`, and the test suite;
`hooks/pre-push` additionally rebuilds the wasm and the single-file web bundle.
Lints are declared in the workspace root so every crate inherits them and a new
crate cannot quietly opt out.

Notable denials: `unwrap_used`, `float_cmp`, `todo`, `dbg_macro`. `clippy::pedantic`
and `clippy::nursery` are on as warnings, and the tree is clean under them.
`indexing_slicing` is a warning too -- a panicking index is a crashed labelling
session, so `Session::card()` is total by construction rather than by luck.

## License

Available under **either** [PolyForm Noncommercial 1.0.0](LICENSE-NONCOMMERCIAL.md)
or [PolyForm Internal Use 1.0.0](LICENSE-INTERNAL-USE.md), at your option — see
[LICENSE.md](LICENSE.md).

Documentation — this README and the notes — is CC BY-NC-SA 4.0 instead; see
[LICENSE-DOCS.md](LICENSE-DOCS.md).

In short: research is welcome, using it at your job is welcome, selling it or a
service built on it is not. The grant on published versions is perpetual and
irrevocable.
