//! What a tagging task is, independent of how it is drawn or driven.

pub mod input;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A group index. Bounded by the four face buttons, so 0..=3.
pub const GROUPS: usize = 4;

#[derive(Debug, Clone, Deserialize)]
pub struct Option_ {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub hint: Option<String>,
    /// Present when picking this option opens partition mode instead of
    /// recording immediately -- how "split" differs from a plain verdict.
    #[serde(default)]
    pub opens: Option<String>,
}

impl Option_ {
    #[must_use]
    pub fn opens_partition(&self) -> bool {
        self.opens.as_deref() == Some("partition")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Card {
    pub id: String,
    pub question: String,
    pub items: Vec<String>,
    #[serde(default)]
    pub flag: Option<String>,
    pub options: Vec<Option_>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Task {
    pub version: String,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Decision {
    pub verdict: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub groups: Option<Vec<Vec<String>>>,
}

pub type Decisions = BTreeMap<String, Decision>;

#[derive(Debug, Clone, Serialize)]
pub struct Output<'a> {
    pub reference: &'a str,
    pub reviewer: &'a str,
    pub remaining: usize,
    pub verdicts: &'a Decisions,
}

/// Verdict mode reads the option row; partition mode assigns items to groups.
///
/// Modelled as an enum so the assignment state cannot exist while no partition
/// is in progress -- the bug that free-floating state would otherwise invite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Verdict,
    Partition { cursor: usize, assigned: Vec<usize> },
}

/// Collapse an assignment into groups.
///
/// Assignment rather than cutting is what makes non-contiguous groupings free:
/// items 1 and 3 sharing a group costs exactly what an adjacent pair costs.
/// Three of the five partitions in the first human pass were non-contiguous, so
/// this is load-bearing, not a nicety. Groups come back in ascending group order
/// so the output is stable across runs.
#[must_use]
pub fn partition_of(items: &[String], assigned: &[usize]) -> Vec<Vec<String>> {
    let mut by_group: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (n, item) in items.iter().enumerate() {
        let g = assigned.get(n).copied().unwrap_or(0);
        by_group.entry(g).or_default().push(item.clone());
    }
    by_group.into_values().collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn items(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("item{i}")).collect()
    }

    #[test]
    fn non_contiguous_groups_survive() {
        // g38 from the real pass: {0,2} together, {1} apart.
        let parts = partition_of(&items(3), &[0, 1, 0]);
        assert_eq!(parts, vec![vec!["item0", "item2"], vec!["item1"]]);
    }

    #[test]
    fn all_one_group_is_a_single_part() {
        assert_eq!(partition_of(&items(4), &[0, 0, 0, 0]).len(), 1);
    }

    #[test]
    fn missing_assignment_defaults_to_first_group() {
        assert_eq!(partition_of(&items(3), &[1]).len(), 2);
    }
}

/// Move `from` by `d`, clamped into `0..len`, without a cast that can wrap.
fn step(from: usize, d: isize, len: usize) -> usize {
    let last = len.saturating_sub(1);
    if d.is_negative() {
        from.saturating_sub(d.unsigned_abs())
    } else {
        from.saturating_add(d.unsigned_abs()).min(last)
    }
}

/// Returned only if a `Session` is somehow built around an empty task, which
/// `Session::new` refuses. Keeps `card()` total rather than panicking.
static EMPTY_CARD: Card = Card {
    id: String::new(),
    question: String::new(),
    items: Vec::new(),
    flag: None,
    options: Vec::new(),
};

/// The whole interaction, with no opinion about how it is drawn.
///
/// A front end feeds this `Action`s and reads back what to render. Everything
/// that decides *what happens* lives here; a front end decides only what it
/// looks like and which physical input produced the action. That split is what
/// makes `ImGui`, a browser build, or a terminal front end interchangeable rather
/// than three near-copies that drift apart.
#[derive(Debug)]
pub struct Session {
    task: Task,
    decisions: Decisions,
    index: usize,
    mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Pick option 0..n of the current card.
    Choose(usize),
    /// Put the item under the cursor into a group (partition mode only).
    Assign(usize),
    /// Move the cursor to a specific item (partition mode only).
    Select(usize),
    Confirm,
    Cancel,
    /// Move between cards, or between items while partitioning.
    Move(isize),
}

/// What a front end needs in order to draw a frame. Borrowed, so rendering
/// cannot mutate the session by accident.
#[derive(Debug)]
pub struct View<'a> {
    pub card: &'a Card,
    pub position: usize,
    pub total: usize,
    pub done: usize,
    pub recorded: Option<&'a Decision>,
    pub mode: &'a Mode,
}

impl Session {
    #[must_use]
    /// Fails when the task has no cards. Rejecting that here is what lets
    /// `card()` be infallible everywhere else: the index is clamped on every
    /// move, so if one card exists the current index always addresses one.
    pub fn new(task: Task, decisions: Decisions) -> Option<Self> {
        if task.cards.is_empty() {
            return None;
        }
        Some(Self::new_unchecked(task, decisions))
    }

    fn new_unchecked(task: Task, decisions: Decisions) -> Self {
        // Open on the first unlabelled card: resuming should not make the
        // labeller scroll past work they already did.
        let index = task
            .cards
            .iter()
            .position(|c| !decisions.contains_key(&c.id))
            .unwrap_or(0);
        Self {
            task,
            decisions,
            index,
            mode: Mode::Verdict,
        }
    }

    #[must_use]
    /// The card under the cursor.
    ///
    /// Infallible by construction: `new` rejects an empty task and every move
    /// clamps into range, so `index` always addresses a card. The fallback is
    /// unreachable and exists only to keep this panic-free.
    pub fn card(&self) -> &Card {
        self.task
            .cards
            .get(self.index)
            .or_else(|| self.task.cards.first())
            .unwrap_or(&EMPTY_CARD)
    }

    #[must_use]
    pub fn view(&self) -> View<'_> {
        View {
            card: self.card(),
            position: self.index,
            total: self.task.cards.len(),
            done: self.decisions.len(),
            recorded: self.decisions.get(&self.card().id),
            mode: &self.mode,
        }
    }

    #[must_use]
    pub const fn decisions(&self) -> &Decisions {
        &self.decisions
    }

    #[must_use]
    pub fn output<'a>(&'a self, reviewer: &'a str) -> Output<'a> {
        Output {
            reference: &self.task.version,
            reviewer,
            remaining: self.task.cards.len() - self.decisions.len(),
            verdicts: &self.decisions,
        }
    }

    /// Apply an action. Returns the verdict id if this recorded one, so a front
    /// end can flash and rumble without having to diff the decision map.
    pub fn apply(&mut self, action: Action) -> Option<String> {
        match action {
            Action::Choose(slot) => self.choose(slot),
            Action::Assign(g) => {
                self.assign(g);
                None
            }
            Action::Select(n) => {
                self.select(n);
                None
            }
            Action::Confirm => self.confirm(),
            Action::Cancel => {
                self.mode = Mode::Verdict;
                None
            }
            Action::Move(d) => {
                self.shift(d);
                None
            }
        }
    }

    fn choose(&mut self, slot: usize) -> Option<String> {
        let card = self.card();
        let opt = card.options.get(slot)?;
        if opt.opens_partition() && card.items.len() > 2 {
            // Everything starts in group 0, so "split off the odd one" is a
            // single press rather than a full pass over the items.
            self.mode = Mode::Partition {
                cursor: 0,
                assigned: vec![0; card.items.len()],
            };
            return None;
        }
        let id = opt.id.clone();
        self.record(id.clone(), None);
        Some(id)
    }

    fn assign(&mut self, g: usize) {
        let len = self.card().items.len();
        if let Mode::Partition { cursor, assigned } = &mut self.mode {
            if let Some(slot) = assigned.get_mut(*cursor) {
                *slot = g.min(GROUPS - 1);
            }
            *cursor = (*cursor + 1).min(len.saturating_sub(1));
        }
    }

    fn select(&mut self, n: usize) {
        let len = self.card().items.len();
        if let Mode::Partition { cursor, .. } = &mut self.mode {
            *cursor = n.min(len.saturating_sub(1));
        }
    }

    fn confirm(&mut self) -> Option<String> {
        let Mode::Partition { assigned, .. } = &self.mode else {
            return None;
        };
        let card = self.card();
        let opt_id = card
            .options
            .iter()
            .find(|o| o.opens_partition())?
            .id
            .clone();
        let parts = partition_of(&card.items, assigned);
        // One group is not a partition -- record a plain verdict instead, so a
        // confirm with nothing assigned cannot produce a meaningless "split".
        let groups = if parts.len() > 1 { Some(parts) } else { None };
        self.record(opt_id.clone(), groups);
        Some(opt_id)
    }

    fn shift(&mut self, d: isize) {
        if let Mode::Partition { cursor, .. } = &self.mode {
            let n = step(*cursor, d, self.card().items.len());
            return self.select(n);
        }
        self.index = step(self.index, d, self.task.cards.len());
    }

    fn record(&mut self, verdict: String, groups: Option<Vec<Vec<String>>>) {
        let id = self.card().id.clone();
        self.decisions.insert(id, Decision { verdict, groups });
        self.mode = Mode::Verdict;
        self.advance();
    }

    /// Jump to the next unlabelled card, wrapping to any earlier gap. Keeps a
    /// labeller moving without making them hunt for what they skipped.
    fn advance(&mut self) {
        let next = self
            .task
            .cards
            .get(self.index + 1..)
            .unwrap_or_default()
            .iter()
            .position(|c| !self.decisions.contains_key(&c.id))
            .map(|n| n + self.index + 1);
        self.index = next
            .or_else(|| {
                self.task
                    .cards
                    .iter()
                    .position(|c| !self.decisions.contains_key(&c.id))
            })
            .unwrap_or(self.index);
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
mod session_tests {
    use super::*;

    fn task(n_items: usize) -> Task {
        Task {
            version: "t".into(),
            cards: vec![
                Card {
                    id: "c0".into(),
                    question: "q".into(),
                    items: (0..n_items).map(|i| format!("i{i}")).collect(),
                    flag: None,
                    options: vec![
                        Option_ {
                            id: "same".into(),
                            label: "Same".into(),
                            hint: None,
                            opens: None,
                        },
                        Option_ {
                            id: "split".into(),
                            label: "Split".into(),
                            hint: None,
                            opens: Some("partition".into()),
                        },
                    ],
                },
                Card {
                    id: "c1".into(),
                    question: "q".into(),
                    items: vec!["a".into(), "b".into()],
                    flag: None,
                    options: vec![Option_ {
                        id: "same".into(),
                        label: "Same".into(),
                        hint: None,
                        opens: None,
                    }],
                },
            ],
        }
    }

    #[test]
    fn two_item_split_records_without_partitioning() {
        let mut s = Session::new(task(2), Decisions::new()).unwrap();
        assert_eq!(s.apply(Action::Choose(1)).as_deref(), Some("split"));
        assert_eq!(s.decisions()["c0"].groups, None);
    }

    #[test]
    fn three_items_open_partition_mode() {
        let mut s = Session::new(task(3), Decisions::new()).unwrap();
        assert_eq!(s.apply(Action::Choose(1)), None);
        assert!(matches!(s.view().mode, Mode::Partition { .. }));
    }

    #[test]
    fn non_contiguous_partition_round_trips() {
        // The g38 shape from the first human pass: items 0 and 2 together.
        let mut s = Session::new(task(3), Decisions::new()).unwrap();
        s.apply(Action::Choose(1));
        s.apply(Action::Assign(0)); // i0 -> group 0, advances
        s.apply(Action::Assign(1)); // i1 -> group 1, advances
        s.apply(Action::Assign(0)); // i2 -> group 0
        s.apply(Action::Confirm);
        let groups = s.decisions()["c0"]
            .groups
            .clone()
            .expect("partition recorded");
        assert_eq!(groups, vec![vec!["i0", "i2"], vec!["i1"]]);
    }

    #[test]
    fn confirming_one_group_records_a_plain_verdict() {
        let mut s = Session::new(task(3), Decisions::new()).unwrap();
        s.apply(Action::Choose(1));
        s.apply(Action::Confirm);
        assert_eq!(s.decisions()["c0"].groups, None);
    }

    #[test]
    fn cancel_leaves_no_decision() {
        let mut s = Session::new(task(3), Decisions::new()).unwrap();
        s.apply(Action::Choose(1));
        s.apply(Action::Cancel);
        assert!(s.decisions().is_empty());
        assert!(matches!(s.view().mode, Mode::Verdict));
    }

    #[test]
    fn recording_advances_to_the_next_unlabelled_card() {
        let mut s = Session::new(task(2), Decisions::new()).unwrap();
        s.apply(Action::Choose(0));
        assert_eq!(s.card().id, "c1");
    }

    #[test]
    fn resuming_opens_on_the_first_gap() {
        let mut d = Decisions::new();
        d.insert(
            "c0".into(),
            Decision {
                verdict: "same".into(),
                groups: None,
            },
        );
        let s = Session::new(task(2), d).unwrap();
        assert_eq!(s.card().id, "c1");
    }
}
