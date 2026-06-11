//! Local trigger heuristics — decides *when* a consideration should run.
//!
//! Pure bookkeeping over injected timestamps: no clocks, no threads, no model
//! calls. The app feeds [`TriggerEngine::note_edit`] from the document event
//! stream, polls on a timer, and calls [`TriggerEngine::mark_considered`]
//! whenever it actually runs a consideration.

use serde::{Deserialize, Serialize};

const HOUR_MS: u64 = 3_600_000;

/// How often Muse is allowed to consider speaking on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chattiness {
    /// Never automatic — only an explicit MuseNow ([`TriggerEngine::force`]).
    Quiet,
    /// The default: speaks rarely, after real pauses.
    #[default]
    Occasional,
    /// Looser thresholds, still budgeted.
    Chatty,
}

/// Threshold set for one chattiness level. `None` means auto never fires.
struct Thresholds {
    /// Minimum quiet time since the last edit.
    idle_ms: u64,
    /// Minimum accumulated character delta since the last consideration
    /// (bypassed when a paragraph was completed).
    accum_chars: usize,
    /// Minimum time since the last consideration.
    gap_ms: u64,
    /// Minimum entry length before Muse reads at all.
    floor_chars: usize,
    /// Maximum automatic considerations per sliding hour.
    budget_per_hour: usize,
}

impl Chattiness {
    fn thresholds(self) -> Option<Thresholds> {
        match self {
            Self::Quiet => None,
            Self::Occasional => Some(Thresholds {
                idle_ms: 1_800,
                accum_chars: 12,
                gap_ms: 30_000,
                floor_chars: 60,
                budget_per_hour: 8,
            }),
            Self::Chatty => Some(Thresholds {
                idle_ms: 1_200,
                accum_chars: 6,
                gap_ms: 18_000,
                floor_chars: 40,
                budget_per_hour: 14,
            }),
        }
    }
}

/// Decides when the app should snapshot the entry and run a consideration.
///
/// All time flows in through `now_ms` (unix millis or any monotonic
/// millisecond clock — only differences matter), so the engine is fully
/// deterministic under test. One engine per open entry.
#[derive(Debug, Clone)]
pub struct TriggerEngine {
    chattiness: Chattiness,
    last_edit_ms: Option<u64>,
    doc_len_chars: usize,
    accum_delta_chars: usize,
    paragraph_completed: bool,
    last_consideration_ms: Option<u64>,
    /// Timestamps of considerations within the sliding hour (budget window).
    considerations: Vec<u64>,
    forced: bool,
}

impl TriggerEngine {
    /// A fresh engine with no edit history.
    #[must_use]
    pub fn new(chattiness: Chattiness) -> Self {
        Self {
            chattiness,
            last_edit_ms: None,
            doc_len_chars: 0,
            accum_delta_chars: 0,
            paragraph_completed: false,
            last_consideration_ms: None,
            considerations: Vec::new(),
            forced: false,
        }
    }

    /// Change the chattiness level. Accumulated state and the hourly budget
    /// ledger carry over.
    pub fn set_chattiness(&mut self, c: Chattiness) {
        self.chattiness = c;
    }

    /// Record one edit event. `delta_chars` is the magnitude of the change
    /// (insertions and deletions both count); `doc_len_chars` is the entry
    /// length after the edit. Call with a zero delta on entry open so the
    /// engine knows the document length.
    pub fn note_edit(
        &mut self,
        now_ms: u64,
        doc_len_chars: usize,
        delta_chars: usize,
        paragraph_completed: bool,
    ) {
        self.last_edit_ms = Some(now_ms);
        self.doc_len_chars = doc_len_chars;
        self.accum_delta_chars = self.accum_delta_chars.saturating_add(delta_chars);
        self.paragraph_completed |= paragraph_completed;
    }

    /// Should the app run a consideration right now?
    ///
    /// Pure read — call it as often as convenient. Returns `true` until
    /// [`Self::mark_considered`] resets the state, so the app should mark
    /// immediately when it acts.
    #[must_use]
    pub fn poll(&self, now_ms: u64) -> bool {
        if self.forced {
            // MuseNow bypasses every threshold except an empty page.
            return self.doc_len_chars > 0;
        }
        let Some(t) = self.chattiness.thresholds() else {
            return false;
        };
        let Some(last_edit) = self.last_edit_ms else {
            return false;
        };
        if now_ms.saturating_sub(last_edit) < t.idle_ms {
            return false;
        }
        if self.accum_delta_chars < t.accum_chars && !self.paragraph_completed {
            return false;
        }
        if let Some(last) = self.last_consideration_ms
            && now_ms.saturating_sub(last) < t.gap_ms
        {
            return false;
        }
        if self.doc_len_chars < t.floor_chars {
            return false;
        }
        let spent = self
            .considerations
            .iter()
            .filter(|&&ts| now_ms.saturating_sub(ts) < HOUR_MS)
            .count();
        spent < t.budget_per_hour
    }

    /// Record that a consideration ran: resets the accumulated delta and the
    /// paragraph flag, clears any pending [`Self::force`], and charges the
    /// hourly budget.
    pub fn mark_considered(&mut self, now_ms: u64) {
        self.accum_delta_chars = 0;
        self.paragraph_completed = false;
        self.forced = false;
        self.last_consideration_ms = Some(now_ms);
        self.considerations.push(now_ms);
        self.considerations
            .retain(|&ts| now_ms.saturating_sub(ts) < HOUR_MS);
    }

    /// MuseNow: the next [`Self::poll`] returns `true` regardless of
    /// thresholds, budget, or chattiness — as long as the entry is non-empty.
    pub fn force(&mut self) {
        self.forced = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive an engine to the brink of firing at Occasional defaults.
    fn primed_occasional() -> TriggerEngine {
        let mut e = TriggerEngine::new(Chattiness::Occasional);
        e.note_edit(1_000, 120, 50, false);
        e
    }

    #[test]
    fn occasional_fires_when_all_thresholds_hold() {
        let e = primed_occasional();
        assert!(e.poll(1_000 + 1_800));
    }

    #[test]
    fn occasional_waits_for_idle() {
        let e = primed_occasional();
        assert!(!e.poll(1_000 + 1_799));
        assert!(e.poll(1_000 + 1_800));
    }

    #[test]
    fn occasional_needs_enough_delta() {
        let mut e = TriggerEngine::new(Chattiness::Occasional);
        e.note_edit(1_000, 120, 11, false);
        assert!(!e.poll(10_000));
        e.note_edit(2_000, 121, 1, false);
        assert!(e.poll(10_000));
    }

    #[test]
    fn paragraph_completion_bypasses_delta_threshold() {
        let mut e = TriggerEngine::new(Chattiness::Occasional);
        e.note_edit(1_000, 120, 1, true);
        assert!(e.poll(10_000));
    }

    #[test]
    fn doc_floor_blocks_short_entries() {
        let mut e = TriggerEngine::new(Chattiness::Occasional);
        e.note_edit(1_000, 59, 50, true);
        assert!(!e.poll(10_000));
        e.note_edit(2_000, 60, 1, false);
        assert!(e.poll(10_000));
    }

    #[test]
    fn minimum_gap_between_considerations() {
        let mut e = primed_occasional();
        e.mark_considered(4_000);
        e.note_edit(5_000, 200, 50, false);
        // Idle satisfied at 6_800, but the 30s gap since 4_000 is not.
        assert!(!e.poll(20_000));
        assert!(e.poll(4_000 + 30_000));
    }

    #[test]
    fn mark_considered_resets_accumulation() {
        let mut e = primed_occasional();
        assert!(e.poll(10_000));
        e.mark_considered(10_000);
        // Plenty of time passes, but no new delta has accumulated.
        assert!(!e.poll(100_000));
        e.note_edit(100_000, 200, 12, false);
        assert!(e.poll(110_000));
    }

    #[test]
    fn budget_exhaustion_and_window_slide() {
        let mut e = TriggerEngine::new(Chattiness::Occasional);
        // Spend all 8 automatic considerations, spaced past the 30s gap.
        for i in 0..8u64 {
            let t = i * 35_000;
            e.note_edit(t, 500, 100, true);
            assert!(e.poll(t + 2_000), "consideration {i} should fire");
            e.mark_considered(t + 2_000);
        }
        // Budget spent: even a perfect setup stays quiet…
        e.note_edit(400_000, 600, 100, true);
        assert!(!e.poll(500_000));
        // …until the first consideration (at 2_000) leaves the sliding hour.
        assert!(e.poll(2_000 + HOUR_MS));
    }

    #[test]
    fn chatty_uses_looser_thresholds() {
        let mut e = TriggerEngine::new(Chattiness::Chatty);
        e.note_edit(1_000, 40, 6, false);
        assert!(!e.poll(1_000 + 1_199));
        assert!(e.poll(1_000 + 1_200));

        // 5 chars is not enough even for chatty.
        let mut e = TriggerEngine::new(Chattiness::Chatty);
        e.note_edit(1_000, 40, 5, false);
        assert!(!e.poll(10_000));

        // 39 chars of document is below the chatty floor.
        let mut e = TriggerEngine::new(Chattiness::Chatty);
        e.note_edit(1_000, 39, 10, false);
        assert!(!e.poll(10_000));

        // Gap is 18s, not 30s.
        let mut e = TriggerEngine::new(Chattiness::Chatty);
        e.note_edit(1_000, 100, 10, false);
        e.mark_considered(4_000);
        e.note_edit(5_000, 110, 10, false);
        assert!(!e.poll(4_000 + 17_999));
        assert!(e.poll(4_000 + 18_000));
    }

    #[test]
    fn chatty_budget_is_fourteen_per_hour() {
        let mut e = TriggerEngine::new(Chattiness::Chatty);
        for i in 0..14u64 {
            let t = i * 20_000;
            e.note_edit(t, 500, 100, true);
            assert!(e.poll(t + 1_500), "consideration {i} should fire");
            e.mark_considered(t + 1_500);
        }
        e.note_edit(400_000, 600, 100, true);
        assert!(!e.poll(500_000));
        assert!(e.poll(1_500 + HOUR_MS));
    }

    #[test]
    fn quiet_never_fires_automatically() {
        let mut e = TriggerEngine::new(Chattiness::Quiet);
        e.note_edit(1_000, 10_000, 1_000, true);
        assert!(!e.poll(u64::MAX));
    }

    #[test]
    fn force_works_at_every_level_but_needs_words() {
        for level in [
            Chattiness::Quiet,
            Chattiness::Occasional,
            Chattiness::Chatty,
        ] {
            let mut e = TriggerEngine::new(level);
            e.force();
            // Empty document: even force stays quiet.
            assert!(!e.poll(0), "{level:?}: forced poll on empty doc");
            e.note_edit(0, 1, 1, false);
            assert!(e.poll(0), "{level:?}: forced poll on non-empty doc");
            // Force survives until marked…
            assert!(e.poll(1));
            e.mark_considered(1);
            // …and is consumed by mark_considered.
            assert!(!e.poll(2));
        }
    }

    #[test]
    fn set_chattiness_applies_new_thresholds() {
        let mut e = TriggerEngine::new(Chattiness::Quiet);
        e.note_edit(1_000, 120, 50, false);
        assert!(!e.poll(10_000));
        e.set_chattiness(Chattiness::Occasional);
        assert!(e.poll(10_000));
        e.set_chattiness(Chattiness::Quiet);
        assert!(!e.poll(10_000));
    }

    #[test]
    fn chattiness_serde_round_trip() {
        let json = serde_json::to_string(&Chattiness::Occasional).expect("serialize");
        assert_eq!(json, "\"occasional\"");
        let back: Chattiness = serde_json::from_str("\"chatty\"").expect("deserialize");
        assert_eq!(back, Chattiness::Chatty);
        assert_eq!(Chattiness::default(), Chattiness::Occasional);
    }
}
