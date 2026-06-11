//! Agent orchestration — the soul. Feeds the per-entry trigger engines from
//! editor edits, runs gated considerations through the api worker, applies
//! decisions (silence, anchored margin notes, or an end-of-entry response),
//! and keeps the dismissal memory honest.
//!
//! Invariants: the writing is never interrupted — every failure path ends in
//! a resting orb and a `tracing` event. With no API key, an installed
//! on-device model thinks instead; with neither, Muse sleeps until one
//! arrives. Notes are located against the *current* text at apply time,
//! never against the snapshot the model read.

use std::time::Duration;

use gpui::{Context, SharedString};
use muse_agent::{
    AgentDecision, DocSnapshot, NoteKind, NoteRecord, TriggerEngine, build_request,
    dismissal_digest, locate_quote, parse_decision,
};
use muse_api::{ApiError, ClaudeReply};
use muse_editor::{Annotation, AnnotationTone};
use muse_topbar::OrbState;

use crate::persistence::now_ms;
use crate::workspace::Workspace;

/// How long the orb holds its "has a note" state before resting again.
const ORB_NOTE_LINGER: Duration = Duration::from_secs(8);

/// A consideration failure from whichever brain ran it.
enum BrainError {
    Cloud(ApiError),
    Local(muse_local::LocalError),
}

/// The agent's tones mirror the note kinds one to one.
fn tone_for(kind: NoteKind) -> AnnotationTone {
    match kind {
        NoteKind::Insight => AnnotationTone::Insight,
        NoteKind::Question => AnnotationTone::Question,
        NoteKind::Encouragement => AnnotationTone::Encouragement,
        NoteKind::Correction => AnnotationTone::Correction,
        NoteKind::Reference => AnnotationTone::Reference,
    }
}

impl Workspace {
    /// Whether any brain can think right now: the cloud (key present) or an
    /// installed on-device model.
    pub(crate) fn brain_available(&self) -> bool {
        !self.key_missing || muse_local::installed_model().is_some()
    }

    /// Whether Muse is allowed to read along right now.
    fn gate(&self) -> bool {
        !self.muted && self.brain_available()
    }

    /// Every document mutation: dirty the autosave and feed the trigger
    /// engine its delta and paragraph-completion signal.
    pub(crate) fn on_edited(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        self.schedule_autosave(cx);

        let (len_chars, paragraph_completed) = {
            let editor = self.editor.read(cx);
            let doc = editor.document();
            let head = editor.selection().end;
            // Cheap heuristic: the char immediately before the caret is a
            // newline ⇒ a paragraph just closed.
            let para = head > 0 && doc.slice(head.saturating_sub(1)..head) == "\n";
            (doc.rope().len_chars(), para)
        };
        let delta = len_chars.abs_diff(self.last_len_chars);
        self.last_len_chars = len_chars;

        let chattiness = self.chattiness;
        self.engines
            .entry(self.current_entry.clone())
            .or_insert_with(|| TriggerEngine::new(chattiness))
            .note_edit(now_ms(), len_chars, delta, paragraph_completed);
    }

    /// The 500ms poll: if the current entry's engine says go (and the gate
    /// is open), mark it considered and run one consideration.
    pub(crate) fn agent_tick(&mut self, cx: &mut Context<Self>) {
        if self.considering || !self.gate() {
            return;
        }
        let now = now_ms();
        let fire = self
            .engines
            .get(&self.current_entry)
            .is_some_and(|engine| engine.poll(now));
        if !fire {
            return;
        }
        if let Some(engine) = self.engines.get_mut(&self.current_entry) {
            engine.mark_considered(now);
        }
        self.run_consideration(cx);
    }

    /// Snapshot the entry, ask whichever brain is awake (cloud when a key
    /// exists, otherwise the on-device model), and apply the reply when it
    /// lands. The await lives on a oneshot that always resolves — never
    /// hangs, and never times out: local replies may take several seconds
    /// (the first loads the model). The `considering` flag holds either way.
    fn run_consideration(&mut self, cx: &mut Context<Self>) {
        self.considering = true;
        self.set_orb(OrbState::Reading, cx);

        let entry_id = self.current_entry.clone();
        let text = self.editor.read(cx).document().plain_text();
        let register_hint = self.read_setting(&format!("register.{entry_id}"));
        let last_response = self.read_setting(&format!("response.{entry_id}"));
        let dismissed_digests = self.store.dismissed_digests(&entry_id).unwrap_or_else(|err| {
            tracing::error!(%err, "failed to read dismissal memory");
            Vec::new()
        });
        let snapshot = DocSnapshot {
            entry_id: entry_id.clone(),
            text,
            register_hint,
            active_notes: self.notes.clone(),
            dismissed_digests,
            last_response,
        };

        self.set_orb(OrbState::Thinking, cx);
        let request = build_request(&snapshot);
        if !self.key_missing {
            let receiver = self.api.request(request);
            cx.spawn(async move |this, cx| {
                let outcome = receiver.await.unwrap_or(Err(ApiError::Channel));
                this.update(cx, |this, cx| {
                    this.apply_consideration(&entry_id, outcome.map_err(BrainError::Cloud), cx);
                })
                .ok();
            })
            .detach();
        } else {
            let receiver = self.local.request(request);
            cx.spawn(async move |this, cx| {
                let outcome = receiver
                    .await
                    .unwrap_or(Err(muse_local::LocalError::Channel));
                this.update(cx, |this, cx| {
                    this.apply_consideration(&entry_id, outcome.map_err(BrainError::Local), cx);
                })
                .ok();
            })
            .detach();
        }
    }

    /// One reply, one decision. Errors rest the orb; only a missing key has
    /// any UI beyond that, and only once.
    fn apply_consideration(
        &mut self,
        entry_id: &str,
        outcome: Result<ClaudeReply, BrainError>,
        cx: &mut Context<Self>,
    ) {
        self.considering = false;
        let reply = match outcome {
            Ok(reply) => reply,
            Err(BrainError::Cloud(ApiError::MissingKey)) => {
                // No key: the cloud sleeps silently; saving a key in
                // Settings wakes it without a relaunch.
                self.key_missing = true;
                self.set_orb(OrbState::Resting, cx);
                return;
            }
            Err(BrainError::Cloud(err)) => {
                tracing::warn!(%err, "consideration failed; staying quiet");
                self.set_orb(OrbState::Resting, cx);
                return;
            }
            Err(BrainError::Local(err)) => {
                tracing::warn!(%err, "local consideration failed; staying quiet");
                self.set_orb(OrbState::Resting, cx);
                return;
            }
        };

        match parse_decision(&reply) {
            AgentDecision::Pass { reason } => {
                tracing::debug!(reason, "muse passed");
                self.set_orb(OrbState::Resting, cx);
            }
            AgentDecision::Notes(drafts) => {
                if entry_id == self.current_entry {
                    self.apply_notes(entry_id, drafts, cx);
                } else {
                    tracing::debug!("entry changed mid-consideration; dropping notes");
                    self.set_orb(OrbState::Resting, cx);
                }
            }
            AgentDecision::Respond { body, register } => {
                self.persist_setting(&format!("response.{entry_id}"), &body);
                if let Some(register) = register {
                    self.persist_setting(&format!("register.{entry_id}"), &register);
                }
                if entry_id == self.current_entry {
                    self.editor.update(cx, |editor, cx| {
                        editor.set_coda(Some(SharedString::from(body)), cx);
                    });
                }
                self.set_orb(OrbState::Resting, cx);
            }
        }
    }

    /// Anchor surviving drafts against the *current* text and bloom them.
    fn apply_notes(
        &mut self,
        entry_id: &str,
        drafts: Vec<muse_agent::NoteDraft>,
        cx: &mut Context<Self>,
    ) {
        let text = self.editor.read(cx).document().plain_text();
        let dismissed = self.store.dismissed_digests(entry_id).unwrap_or_default();
        let mut added = false;
        for draft in drafts {
            let digest = dismissal_digest(&draft.quote, draft.kind);
            if dismissed.iter().any(|d| d == &digest) {
                tracing::debug!("note dropped: previously dismissed");
                continue;
            }
            let Some(range) = locate_quote(&text, &draft.quote, &draft.prefix, &draft.suffix)
            else {
                tracing::debug!("note dropped: quote no longer present");
                continue;
            };
            let id = self.next_note_id;
            self.next_note_id += 1;
            let record = NoteRecord {
                id,
                quote: draft.quote,
                kind: draft.kind,
                body: draft.body,
                emoji: draft.emoji,
            };
            self.editor.update(cx, |editor, cx| {
                editor.add_annotation(
                    Annotation {
                        id,
                        range,
                        tone: tone_for(record.kind),
                        body: SharedString::from(record.body.clone()),
                        emoji: record.emoji.clone().map(SharedString::from),
                    },
                    cx,
                );
            });
            self.notes.push(record);
            added = true;
        }
        if added {
            self.save_notes_for(entry_id);
            self.set_orb(OrbState::HasNote, cx);
            self.schedule_orb_rest(cx);
        } else {
            self.set_orb(OrbState::Resting, cx);
        }
    }

    /// A dismissed margin note: remember its digest forever, drop the
    /// record, re-persist.
    pub(crate) fn on_annotation_dismissed(&mut self, id: u64, _cx: &mut Context<Self>) {
        let Some(index) = self.notes.iter().position(|note| note.id == id) else {
            return;
        };
        let record = self.notes.remove(index);
        let digest = dismissal_digest(&record.quote, record.kind);
        if let Err(err) = self.store.add_dismissed_digest(&self.current_entry, &digest) {
            tracing::error!(%err, "failed to remember dismissal");
        }
        let entry_id = self.current_entry.clone();
        self.save_notes_for(&entry_id);
    }

    /// On entry open: seed the engine with the document length, re-anchor
    /// persisted notes against the current text (dropping the lost ones),
    /// and restore the coda.
    pub(crate) fn restore_agent_state(&mut self, cx: &mut Context<Self>) {
        let entry_id = self.current_entry.clone();
        let (text, len_chars) = {
            let doc = self.editor.read(cx).document();
            (doc.plain_text(), doc.rope().len_chars())
        };

        let chattiness = self.chattiness;
        self.engines
            .entry(entry_id.clone())
            .or_insert_with(|| TriggerEngine::new(chattiness))
            .note_edit(now_ms(), len_chars, 0, false);
        self.last_len_chars = len_chars;

        self.notes.clear();
        let loaded: Vec<NoteRecord> = match self.store.load_notes(&entry_id) {
            Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_else(|err| {
                tracing::error!(%err, "persisted notes failed to parse");
                Vec::new()
            }),
            Ok(None) => Vec::new(),
            Err(err) => {
                tracing::error!(%err, "failed to load persisted notes");
                Vec::new()
            }
        };
        let loaded_count = loaded.len();
        let mut annotations = Vec::new();
        for record in loaded {
            let Some(range) = locate_quote(&text, &record.quote, "", "") else {
                continue;
            };
            annotations.push(Annotation {
                id: record.id,
                range,
                tone: tone_for(record.kind),
                body: SharedString::from(record.body.clone()),
                emoji: record.emoji.clone().map(SharedString::from),
            });
            self.next_note_id = self.next_note_id.max(record.id + 1);
            self.notes.push(record);
        }
        if self.notes.len() != loaded_count {
            // Some anchors were lost to edits made before this open.
            self.save_notes_for(&entry_id);
        }
        self.editor.update(cx, |editor, cx| {
            editor.set_annotations(annotations, cx);
        });

        let coda = self.read_setting(&format!("response.{entry_id}"));
        self.editor.update(cx, |editor, cx| {
            editor.set_coda(coda.map(SharedString::from), cx);
        });

        self.set_orb(OrbState::Resting, cx);
    }

    /// Persist the current entry's note records as one JSON blob.
    fn save_notes_for(&self, entry_id: &str) {
        match serde_json::to_string(&self.notes) {
            Ok(json) => {
                if let Err(err) = self.store.save_notes(entry_id, &json) {
                    tracing::error!(%err, "failed to persist notes");
                }
            }
            Err(err) => tracing::error!(%err, "failed to serialize notes"),
        }
    }

    /// Set the orb, invalidating any pending state timers.
    fn set_orb(&mut self, state: OrbState, cx: &mut Context<Self>) {
        self.orb_generation = self.orb_generation.wrapping_add(1);
        self.topbar
            .update(cx, |topbar, cx| topbar.set_orb(state, cx));
    }

    /// After a note blooms, let the orb rest again once the moment passes.
    fn schedule_orb_rest(&mut self, cx: &mut Context<Self>) {
        let generation = self.orb_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ORB_NOTE_LINGER).await;
            this.update(cx, |this, cx| {
                if this.orb_generation == generation {
                    this.set_orb(OrbState::Resting, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Read a setting as an option, logging failures.
    fn read_setting(&self, key: &str) -> Option<String> {
        self.store.setting(key).unwrap_or_else(|err| {
            tracing::error!(%err, key, "failed to read setting");
            None
        })
    }
}
