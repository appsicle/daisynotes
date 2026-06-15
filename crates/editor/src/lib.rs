//! daisynotes-editor — the page itself: multi-paragraph rich-text layout,
//! rendering, input (including IME), caret and selection, clipboard,
//! margin annotations, and the end-of-entry coda.
//!
//! Owns pixels and input for one document. Knows nothing about entries,
//! storage, the network, or the agent — the app maps agent output into
//! presentation-neutral [`Annotation`]s and a coda string.

mod anim;
mod element;
mod layout;
mod notes;
mod overlays;
mod policy;
mod runs;

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, ClipboardEntry, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable,
    Image, ImageFormat, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollWheelEvent, SharedString, UTF16Selection, Window, div, prelude::*, px, size,
};
use daisynotes_commands as cmd;
use daisynotes_core::{
    DocFragment, Document, ImageBlock, ListAttr, ListKind, MAX_LIST_INDENT, StyleToggle, Voice,
};

use crate::anim::CaretSpring;
use crate::layout::{IMAGE_VMARGIN, ParaRec, Snapshot};
use crate::notes::NoteSlot;
use crate::policy::{PendingStyle, Selection};

pub use crate::notes::{Annotation, AnnotationTone};

/// The list kind a typed line prefix triggers, if it is exactly `- `, `* `,
/// or `<digits>. ` and nothing else.
fn marker_kind(prefix: &str) -> Option<ListKind> {
    if prefix == "- " || prefix == "* " {
        return Some(ListKind::Bullet);
    }
    if let Some(num) = prefix.strip_suffix(". ")
        && !num.is_empty()
        && num.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(ListKind::Number);
    }
    None
}

/// One outdent step: drop a level, or clear the list at the top level.
fn outdent(attr: ListAttr) -> Option<ListAttr> {
    if attr.indent > 0 {
        Some(ListAttr {
            indent: attr.indent - 1,
            ..attr
        })
    } else {
        None
    }
}

/// The first image entry on the clipboard, if any.
fn first_clipboard_image(item: &ClipboardItem) -> Option<Arc<Image>> {
    item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(Arc::new(image.clone())),
        ClipboardEntry::String(_) => None,
    })
}

/// The MIME string for a GPUI image format (also used as the blob's `mime`).
pub fn mime_of(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Webp => "image/webp",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Svg => "image/svg+xml",
        ImageFormat::Bmp => "image/bmp",
        ImageFormat::Tiff => "image/tiff",
    }
}

/// The GPUI image format for a stored MIME string (the inverse of [`mime_of`]),
/// used by the app to rebuild a decode source from a blob.
pub fn format_of(mime: &str) -> ImageFormat {
    match mime {
        "image/jpeg" => ImageFormat::Jpeg,
        "image/webp" => ImageFormat::Webp,
        "image/gif" => ImageFormat::Gif,
        "image/svg+xml" => ImageFormat::Svg,
        "image/bmp" => ImageFormat::Bmp,
        "image/tiff" => ImageFormat::Tiff,
        _ => ImageFormat::Png,
    }
}

/// Smallest an image may be resized to (display width, px).
const IMAGE_MIN_W: f32 = 48.0;
/// Half-extent of a resize handle's clickable area (px).
const IMAGE_HANDLE_HIT: f32 = 11.0;

/// Delay before the format pill blooms over a freshly settled selection.
const PILL_DELAY: Duration = Duration::from_millis(120);

/// How long the dismissed-card recede animation runs before the editor
/// emits [`EditorEvent::AnnotationDismissed`].
const CARD_RECEDE: Duration = Duration::from_millis(180);

/// What the editor tells the app. `Edited` fires on every document
/// mutation — autosave and agent triggers hang off it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEvent {
    /// The document changed (text, styles, or voice — including undo/redo).
    Edited,
    /// The selection or caret moved.
    SelectionChanged,
    /// The entry voice (family/size/weight) changed.
    VoiceChanged,
    /// The user dismissed a margin note.
    AnnotationDismissed {
        /// The id of the dismissed [`Annotation`].
        id: u64,
    },
    /// The editor's vertical scroll offset changed.
    ScrollChanged,
    /// An image was pasted; the app persists its bytes as a blob.
    ImagePasted {
        /// Content-hash id (blob key and GPUI image id).
        id: u64,
        /// MIME type, e.g. `"image/png"`.
        mime: SharedString,
        /// The encoded image bytes.
        bytes: Vec<u8>,
    },
    /// A styled paste re-referenced existing image blocks (by id). The app
    /// reloads decode sources from the store so the pasted images render; their
    /// blobs already persist, so no bytes travel on the clipboard.
    ImagesReferenced,
}

/// How a drag extends the selection, set by the initiating click count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Granularity {
    Char,
    Word,
    Paragraph,
}

/// The open margin-note card.
struct OpenCard {
    id: u64,
    pinned: bool,
}

/// A dismissed card mid-recede, kept just long enough to animate out.
struct ClosingCard {
    id: u64,
    tone: AnnotationTone,
    body: SharedString,
    position: Point<Pixels>,
}

/// The end-of-entry response block.
struct Coda {
    body: SharedString,
    since: Instant,
}

/// The rich-text editor entity: one centered column, one document.
pub struct Editor {
    doc: Document,
    focus_handle: FocusHandle,
    sel: Selection,
    goal_x: Option<f32>,
    marked: Option<Range<usize>>,
    pending: Option<PendingStyle>,

    // PERFORMANCE INVARIANT: `text` mirrors the rope and is patched
    // incrementally on the typing path; `plain_text()` is only called on
    // wholesale changes (undo/redo, entry switch). `paras` carries shaped
    // lines across edits so a keystroke re-shapes only its own paragraph.
    text: String,
    paras: Vec<ParaRec>,
    snapshot: Option<Rc<Snapshot>>,
    coda_shaped: Option<(u64, Vec<gpui::WrappedLine>)>,

    scroll: f32,
    spring: CaretSpring,
    spring_primed: bool,
    last_frame: Option<Instant>,
    blink_reset: Instant,

    is_selecting: bool,
    granularity: Granularity,
    drag_origin: Range<usize>,
    drag_point: Option<Point<Pixels>>,

    last_edit: Option<Instant>,
    date_label: Option<SharedString>,

    notes: Vec<NoteSlot>,
    hovered_dot: Option<u64>,
    card: Option<OpenCard>,
    card_hovered: bool,
    closing_card: Option<ClosingCard>,

    coda: Option<Coda>,

    pill_shown: bool,
    pill_token: u64,
    /// Which format-pill dropdown is open, if any.
    pill_menu: Option<PillMenu>,

    /// Decode sources for embedded images, keyed by content-hash id. The app
    /// fills these from blobs on entry open and from pastes; the element
    /// decodes them lazily through GPUI's image cache.
    image_sources: HashMap<u64, Arc<Image>>,
    /// The paragraph offset of the currently selected image (click to select);
    /// drives the resize handles and image-aware Delete.
    selected_image: Option<usize>,
    /// An in-progress corner-handle resize: `(paragraph offset, live width px)`.
    image_drag: Option<(usize, f32)>,

    autoscroll_to: Option<usize>,
}

/// The two dropdowns the format pill can open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PillMenu {
    /// The font-family chooser.
    Family,
    /// The font-size chooser.
    Size,
}

impl Editor {
    /// Build an editor over `document`.
    pub fn new(document: Document, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = document.plain_text();
        let paras = layout::reuse_paragraphs(Vec::new(), &text);
        Self {
            doc: document,
            focus_handle: cx.focus_handle(),
            sel: Selection::caret(0),
            goal_x: None,
            marked: None,
            pending: None,
            text,
            paras,
            snapshot: None,
            coda_shaped: None,
            scroll: 0.0,
            spring: CaretSpring::resting(0.0, 0.0),
            spring_primed: false,
            last_frame: None,
            blink_reset: Instant::now(),
            is_selecting: false,
            granularity: Granularity::Char,
            drag_origin: 0..0,
            drag_point: None,
            last_edit: None,
            date_label: None,
            notes: Vec::new(),
            hovered_dot: None,
            card: None,
            card_hovered: false,
            closing_card: None,
            coda: None,
            pill_shown: false,
            pill_token: 0,
            pill_menu: None,
            image_sources: HashMap::new(),
            selected_image: None,
            image_drag: None,
            autoscroll_to: None,
        }
    }

    /// Swap in a different document (entry switch): selection, scroll,
    /// annotations, and coda all reset; the first frame renders the new
    /// entry whole.
    pub fn replace_document(&mut self, doc: Document, cx: &mut Context<Self>) {
        self.doc = doc;
        self.text = self.doc.plain_text();
        self.paras = layout::reuse_paragraphs(Vec::new(), &self.text);
        self.snapshot = None;
        self.coda_shaped = None;
        self.sel = Selection::caret(0);
        self.goal_x = None;
        self.marked = None;
        self.pending = None;
        self.scroll = 0.0;
        self.spring_primed = false;
        self.blink_reset = Instant::now();
        self.is_selecting = false;
        self.drag_point = None;
        self.last_edit = None;
        self.notes.clear();
        self.hovered_dot = None;
        self.card = None;
        self.card_hovered = false;
        self.closing_card = None;
        self.coda = None;
        self.hide_pill(cx);
        self.image_sources.clear();
        self.selected_image = None;
        self.image_drag = None;
        self.autoscroll_to = None;
        cx.emit(EditorEvent::SelectionChanged);
        cx.emit(EditorEvent::ScrollChanged);
        cx.notify();
    }

    /// Provide decode sources for the document's image blocks (the app calls
    /// this right after [`Editor::replace_document`], from stored blobs).
    pub fn set_image_sources(&mut self, sources: HashMap<u64, Arc<Image>>, cx: &mut Context<Self>) {
        self.image_sources = sources;
        cx.notify();
    }

    /// The live document.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// A cheap snapshot of the document (for the agent / autosave).
    pub fn snapshot(&self) -> Document {
        self.doc.clone()
    }

    /// The selection as a forward byte range (collapsed = caret).
    pub fn selection(&self) -> Range<usize> {
        self.sel.range()
    }

    /// Set the selection programmatically and scroll the caret into view.
    pub fn select(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let start = self.doc.clamp(range.start);
        let end = self.doc.clamp(range.end);
        let (start, end) = if start <= end { (start, end) } else { (end, start) };
        self.set_selection(
            Selection {
                anchor: start,
                head: end,
            },
            cx,
        );
        self.autoscroll_to = Some(end);
        cx.notify();
    }

    /// The tertiary date label above the first line. The space it occupies
    /// is reserved whether or not a label is present — nothing shifts.
    pub fn set_date_label(&mut self, label: Option<SharedString>, cx: &mut Context<Self>) {
        if self.date_label != label {
            self.date_label = label;
            cx.notify();
        }
    }

    /// The entry voice.
    pub fn voice(&self) -> Voice {
        self.doc.voice()
    }

    /// Whether the page is scrolled away from the top (drives the topbar hairline).
    pub fn is_scrolled(&self) -> bool {
        self.scroll > 0.5
    }

    /// Replace all margin annotations. Each range is re-anchored through
    /// the document so it tracks subsequent edits.
    pub fn set_annotations(&mut self, notes: Vec<Annotation>, cx: &mut Context<Self>) {
        for slot in self.notes.drain(..) {
            self.doc.release_anchor(slot.anchor);
        }
        let now = Instant::now();
        for ann in notes {
            let anchor = self.doc.anchor(ann.range.clone());
            self.notes.push(NoteSlot {
                ann,
                anchor,
                appeared: now,
                withering: None,
                last_center: None,
                last_rects: Vec::new(),
            });
        }
        if let Some(card) = &self.card
            && !self.notes.iter().any(|slot| slot.ann.id == card.id)
        {
            self.card = None;
        }
        cx.notify();
    }

    /// Add one margin annotation, anchored at its range. Reactions bloom on
    /// the text itself; notes appear as a quiet citation caret that reveals
    /// its card on hover.
    pub fn add_annotation(&mut self, note: Annotation, cx: &mut Context<Self>) {
        let anchor = self.doc.anchor(note.range.clone());
        self.notes.push(NoteSlot {
            ann: note,
            anchor,
            appeared: Instant::now(),
            withering: None,
            last_center: None,
            last_rects: Vec::new(),
        });
        cx.notify();
    }

    /// Set (or clear) the end-of-entry coda. Setting starts the divider
    /// draw-in and the word-by-word reveal.
    pub fn set_coda(&mut self, body: Option<SharedString>, cx: &mut Context<Self>) {
        self.coda = body.map(|body| Coda {
            body,
            since: Instant::now(),
        });
        self.coda_shaped = None;
        cx.notify();
    }

    // ── Selection & caret internals ────────────────────────────────────────

    fn set_selection(&mut self, sel: Selection, cx: &mut Context<Self>) {
        // Any caret/selection activity deselects an image.
        self.selected_image = None;
        if sel == self.sel {
            return;
        }
        self.sel = sel;
        self.blink_reset = Instant::now();
        if let Some(pending) = self.pending
            && pending.at != sel.head
        {
            self.pending = None;
        }
        if sel.is_empty() || self.is_selecting {
            self.hide_pill(cx);
        } else if !self.pill_shown {
            self.schedule_pill(cx);
        }
        cx.emit(EditorEvent::SelectionChanged);
        cx.notify();
    }

    fn caret_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.goal_x = None;
        let offset = self.doc.clamp(offset);
        self.set_selection(Selection::caret(offset), cx);
        self.autoscroll_to = Some(offset);
    }

    fn head_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.goal_x = None;
        let offset = self.doc.clamp(offset);
        let anchor = self.sel.anchor;
        self.set_selection(
            Selection {
                head: offset,
                anchor,
            },
            cx,
        );
        self.autoscroll_to = Some(offset);
    }

    fn vertical_move(&mut self, dir: f32, extend: bool, cx: &mut Context<Self>) {
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        let (x, y) = snap.caret_point(self.sel.head);
        let goal = self.goal_x.unwrap_or(x);
        let target_y = y + dir * snap.line_height + snap.line_height * 0.5;
        let offset = if target_y < 0.0 {
            0
        } else {
            snap.offset_at((goal, target_y))
        };
        if extend {
            let anchor = self.sel.anchor;
            self.set_selection(
                Selection {
                    head: offset,
                    anchor,
                },
                cx,
            );
        } else {
            self.set_selection(Selection::caret(offset), cx);
        }
        // Preserve the goal column across consecutive vertical moves.
        self.goal_x = Some(goal);
        self.autoscroll_to = Some(offset);
    }

    /// The image paragraph whose displayed bounds contain `content`, if any.
    fn image_hit(snap: &Snapshot, content: (f32, f32)) -> Option<usize> {
        let (x, y) = content;
        snap.paras.iter().find_map(|para| {
            let img = para.image.as_ref()?;
            let top = para.y + IMAGE_VMARGIN;
            (x >= 0.0 && x <= img.w && y >= top && y <= top + img.h)
                .then_some(para.span.range.start)
        })
    }

    /// The selected image's current display width if `content` is on one of
    /// its four corner handles (used to start a resize).
    fn image_handle_hit(&self, snap: &Snapshot, para: usize, content: (f32, f32)) -> Option<f32> {
        let placed = snap.paras.iter().find(|p| p.span.range.start == para)?;
        let img = placed.image.as_ref()?;
        let top = placed.y + IMAGE_VMARGIN;
        let (x, y) = content;
        let corners = [
            (0.0, top),
            (img.w, top),
            (0.0, top + img.h),
            (img.w, top + img.h),
        ];
        corners
            .iter()
            .any(|(hx, hy)| (x - hx).abs() <= IMAGE_HANDLE_HIT && (y - hy).abs() <= IMAGE_HANDLE_HIT)
            .then_some(img.w)
    }

    /// The visual row containing `offset` (falls back to the logical
    /// paragraph before the first layout).
    fn visual_row(&self, offset: usize) -> Range<usize> {
        match &self.snapshot {
            Some(snap) => snap.visual_row_range(offset),
            None => self.doc.paragraph_range_at(offset),
        }
    }

    // ── Edits ──────────────────────────────────────────────────────────────

    /// The single mutation path for text edits: undo-group bookkeeping,
    /// pending-style consumption, incremental cache patching, caret
    /// placement, and event emission.
    fn edit_replace(&mut self, range: Range<usize>, new_text: &str, cx: &mut Context<Self>) {
        let start = self.doc.clamp(range.start);
        let end = self.doc.clamp(range.end);
        let range = if start <= end { start..end } else { end..start };
        if range.is_empty() && new_text.is_empty() {
            return;
        }
        // Typing into an image's own (empty) paragraph would paint the text
        // under the image. Keep the image a block on its own line: append a
        // newline so the image slides down to the next paragraph, while the
        // caret stays right after the typed text (not the synthetic newline).
        let image_split = range.is_empty()
            && !new_text.is_empty()
            && !new_text.contains('\n')
            && self.doc.image_at_offset(range.start).is_some();
        let split_text;
        let insert_text: &str = if image_split {
            split_text = format!("{new_text}\n");
            &split_text
        } else {
            new_text
        };

        let now = Instant::now();
        if policy::should_break_undo_group(self.last_edit.map(|at| now.duration_since(at))) {
            self.doc.break_undo_group();
        }
        // A newline is a paragraph boundary: close the typing run.
        if insert_text.contains('\n') {
            self.doc.break_undo_group();
        }
        let pending = self
            .pending
            .take()
            .filter(|p| range.is_empty() && p.at == range.start && !new_text.is_empty());

        if range.is_empty() {
            self.doc.insert(range.start, insert_text);
        } else if new_text.is_empty() {
            self.doc.delete(range.clone());
        } else {
            self.doc.replace(range.clone(), insert_text);
        }
        self.text.replace_range(range.clone(), insert_text);
        let caret = range.start + new_text.len();

        if let Some(pending) = pending {
            let inserted = range.start..caret;
            let continuation = self.doc.spans().style_at(range.start);
            for toggle in pending.toggles_against(continuation) {
                self.doc.toggle_style(inserted.clone(), toggle);
            }
        }

        self.after_doc_change();
        self.marked = None;
        self.last_edit = Some(now);
        self.hide_pill(cx);
        self.set_selection(Selection::caret(caret), cx);
        self.goal_x = None;
        self.autoscroll_to = Some(caret);
        // A typed space may complete a `- ` / `1. ` list marker.
        if new_text == " " && range.is_empty() {
            self.try_list_trigger(cx);
        }
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    /// After a typed space, turn a leading `- ` / `* ` / `N. ` marker on an
    /// otherwise-plain line into a real list paragraph (the marker text is
    /// consumed; the attribute renders the bullet/number instead).
    fn try_list_trigger(&mut self, cx: &mut Context<Self>) {
        let caret = self.sel.head;
        let line = self.doc.paragraph_range_at(caret);
        if self.doc.para_attr(line.start).is_some() {
            return;
        }
        let prefix = self.doc.slice(line.start..caret);
        let Some(kind) = marker_kind(&prefix) else {
            return;
        };
        self.doc.break_undo_group();
        self.edit_replace(line.start..caret, "", cx);
        // Fold the list attribute into the marker-delete's undo group so one
        // Cmd-Z restores the `- `/`N. ` marker and clears the bullet together.
        self.doc
            .set_para_list_grouped(line.start, Some(ListAttr { kind, indent: 0 }));
        cx.notify();
    }

    /// Re-derive the paragraph list, carrying shaped lines across for
    /// untouched paragraphs.
    fn after_doc_change(&mut self) {
        self.paras = layout::reuse_paragraphs(std::mem::take(&mut self.paras), &self.text);
    }

    /// Full cache rebuild for wholesale changes (undo/redo).
    fn rebuild_text_full(&mut self) {
        self.text = self.doc.plain_text();
        self.after_doc_change();
    }

    fn apply_style_toggle(&mut self, toggle: StyleToggle, cx: &mut Context<Self>) {
        let range = self.sel.range();
        if range.is_empty() {
            let base = self.doc.style_for_insertion(self.sel.head);
            self.pending = Some(PendingStyle::stage(self.pending, self.sel.head, base, toggle));
            cx.notify();
            return;
        }
        let version = self.doc.version();
        self.doc.toggle_style(range, toggle);
        if self.doc.version() != version {
            cx.emit(EditorEvent::Edited);
            cx.notify();
        }
    }

    fn apply_voice(&mut self, voice: Voice, cx: &mut Context<Self>) {
        if voice == self.doc.voice() {
            return;
        }
        self.doc.set_voice(voice);
        cx.emit(EditorEvent::VoiceChanged);
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    // ── Format pill ────────────────────────────────────────────────────────

    fn hide_pill(&mut self, cx: &mut Context<Self>) {
        // Bump the token so any in-flight bloom timer is stale.
        self.pill_token = self.pill_token.wrapping_add(1);
        if self.pill_shown {
            self.pill_shown = false;
            self.pill_menu = None;
            cx.notify();
        }
    }

    /// Which pill dropdown is open (read by the overlay renderer).
    pub(crate) fn pill_menu(&self) -> Option<PillMenu> {
        self.pill_menu
    }

    /// Open `kind`'s dropdown, or close it if it's already open.
    pub(crate) fn toggle_pill_menu(&mut self, kind: PillMenu, cx: &mut Context<Self>) {
        self.pill_menu = if self.pill_menu == Some(kind) { None } else { Some(kind) };
        cx.notify();
    }

    /// Close any open pill dropdown.
    pub(crate) fn close_pill_menu(&mut self, cx: &mut Context<Self>) {
        if self.pill_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Pick a font family from the pill's family dropdown, then close it.
    pub(crate) fn choose_family(&mut self, family: daisynotes_core::FontFamily, cx: &mut Context<Self>) {
        let voice = Voice { family, ..self.doc.voice() };
        self.apply_voice(voice, cx);
        self.close_pill_menu(cx);
    }

    /// Pick a base size from the pill's size dropdown, then close it.
    pub(crate) fn choose_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let voice = Voice { size, ..self.doc.voice() };
        self.apply_voice(voice, cx);
        self.close_pill_menu(cx);
    }

    /// Bloom the pill once the selection has been stable for a beat.
    fn schedule_pill(&mut self, cx: &mut Context<Self>) {
        self.pill_token = self.pill_token.wrapping_add(1);
        let token = self.pill_token;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PILL_DELAY).await;
            this.update(cx, |editor, cx| {
                if editor.pill_token == token
                    && !editor.sel.is_empty()
                    && !editor.is_selecting
                    && !editor.pill_shown
                {
                    editor.pill_shown = true;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // ── Margin notes ───────────────────────────────────────────────────────

    pub(crate) fn set_card_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.card_hovered != hovered {
            self.card_hovered = hovered;
            self.close_card_if_unattended(cx);
            cx.notify();
        }
    }

    fn close_card_if_unattended(&mut self, cx: &mut Context<Self>) {
        if let Some(card) = &self.card
            && !card.pinned
            && !self.card_hovered
            && self.hovered_dot != Some(card.id)
        {
            self.card = None;
            cx.notify();
        }
    }

    pub(crate) fn dismiss_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(idx) = self.notes.iter().position(|slot| slot.ann.id == id) else {
            return;
        };
        let slot = self.notes.remove(idx);
        self.doc.release_anchor(slot.anchor);
        // A note recedes as a fading card; a pure reaction just pops away —
        // it has no body, so a card would only flash an empty shell.
        if slot.ann.emoji.is_none() {
            let position = self
                .snapshot
                .as_ref()
                .and_then(|snap| {
                    slot.last_center
                        .map(|center| snap.to_window((center.0, center.1)))
                })
                .unwrap_or_default();
            self.closing_card = Some(ClosingCard {
                id,
                tone: slot.ann.tone,
                body: slot.ann.body.clone(),
                position,
            });
        }
        self.card = None;
        self.hovered_dot = None;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CARD_RECEDE).await;
            this.update(cx, |editor, cx| {
                if editor
                    .closing_card
                    .as_ref()
                    .is_some_and(|closing| closing.id == id)
                {
                    editor.closing_card = None;
                }
                cx.emit(EditorEvent::AnnotationDismissed { id });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // ── Mouse (called from the element's window listeners) ────────────────

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        // A click on an annotation marker: a pure reaction dismisses (like
        // un-reacting in iMessage); a note pins (or unpins) its card.
        if let Some(id) = Self::dot_hit(&snap, event.position) {
            let is_reaction = self
                .notes
                .iter()
                .find(|slot| slot.ann.id == id)
                .is_some_and(|slot| slot.ann.emoji.is_some());
            if is_reaction {
                self.dismiss_note(id, cx);
                return;
            }
            match &mut self.card {
                Some(card) if card.id == id => card.pinned = !card.pinned,
                _ => {
                    self.card = Some(OpenCard { id, pinned: true });
                }
            }
            self.close_card_if_unattended(cx);
            cx.notify();
            return;
        }
        if self.card.is_some() {
            self.card = None;
            cx.notify();
        }
        let content = snap.to_content(event.position);
        // A corner handle of the selected image starts a resize.
        if let Some(para) = self.selected_image
            && let Some(width) = self.image_handle_hit(&snap, para, content)
        {
            self.image_drag = Some((para, width));
            cx.notify();
            return;
        }
        // Clicking an image selects it (and nothing else).
        if let Some(para) = Self::image_hit(&snap, content) {
            self.selected_image = Some(para);
            self.is_selecting = false;
            self.hide_pill(cx);
            cx.notify();
            return;
        }
        self.selected_image = None;
        let offset = snap.offset_at(content);
        match event.click_count {
            1 => {
                if event.modifiers.shift {
                    let anchor = self.sel.anchor;
                    self.set_selection(
                        Selection {
                            head: offset,
                            anchor,
                        },
                        cx,
                    );
                    self.drag_origin = anchor..anchor;
                } else {
                    self.set_selection(Selection::caret(offset), cx);
                    self.drag_origin = offset..offset;
                }
                self.granularity = Granularity::Char;
            }
            2 => {
                let range = self.doc.word_range_at(offset);
                self.drag_origin = range.clone();
                self.granularity = Granularity::Word;
                self.set_selection(
                    Selection {
                        anchor: range.start,
                        head: range.end,
                    },
                    cx,
                );
            }
            _ => {
                let range = self.doc.paragraph_range_at(offset);
                self.drag_origin = range.clone();
                self.granularity = Granularity::Paragraph;
                self.set_selection(
                    Selection {
                        anchor: range.start,
                        head: range.end,
                    },
                    cx,
                );
            }
        }
        self.goal_x = None;
        // Mouse-initiated caret placement snaps instantly: the next layout
        // pass resets the spring at the new caret instead of animating.
        self.spring_primed = false;
        self.is_selecting = true;
        self.drag_point = Some(event.position);
        self.hide_pill(cx);
        cx.notify();
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A resize in progress: the live width follows the cursor's x,
        // clamped to a sane minimum and the column width. Height tracks the
        // natural aspect in layout.
        if let Some((para, _)) = self.image_drag {
            if let Some(snap) = self.snapshot.clone() {
                let (x, _) = snap.to_content(event.position);
                let width = x.clamp(IMAGE_MIN_W, snap.wrap_width);
                self.image_drag = Some((para, width));
                cx.notify();
            }
            return;
        }
        if self.is_selecting && event.dragging() {
            self.drag_point = Some(event.position);
            self.extend_to_point(event.position, cx);
            return;
        }
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        let hovered = Self::dot_hit(&snap, event.position);
        if hovered != self.hovered_dot {
            self.hovered_dot = hovered;
            if let Some(id) = hovered {
                // Pure reactions carry no message; only notes reveal a card.
                let is_note = self
                    .notes
                    .iter()
                    .find(|slot| slot.ann.id == id)
                    .is_some_and(|slot| slot.ann.emoji.is_none());
                let already_open = self.card.as_ref().is_some_and(|card| card.id == id);
                if is_note && !already_open {
                    self.card = Some(OpenCard { id, pinned: false });
                }
            }
            self.close_card_if_unattended(cx);
            cx.notify();
        }
    }

    pub(crate) fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Commit a resize: write the chosen width onto the block (undoable).
        if let Some((para, width)) = self.image_drag.take() {
            if let Some(mut block) = self.doc.image_at(para) {
                block.width = width.round().max(1.0) as u32;
                self.doc.set_image(para, Some(block));
            }
            cx.notify();
            return;
        }
        if !self.is_selecting {
            return;
        }
        self.is_selecting = false;
        self.drag_point = None;
        if !self.sel.is_empty() {
            self.schedule_pill(cx);
        }
        cx.notify();
    }

    pub(crate) fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line_height = self
            .snapshot
            .as_ref()
            .map_or(26.4, |snap| snap.line_height);
        let delta = event.delta.pixel_delta(px(line_height));
        self.scroll_by(-f32::from(delta.y), cx);
    }

    pub(crate) fn scroll_by(&mut self, dy: f32, cx: &mut Context<Self>) {
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        let max = snap.max_scroll(f32::from(snap.bounds.size.height));
        let next = (self.scroll + dy).clamp(0.0, max);
        if (next - self.scroll).abs() > f32::EPSILON {
            self.scroll = next;
            self.hide_pill(cx);
            cx.emit(EditorEvent::ScrollChanged);
            cx.notify();
        }
    }

    /// Extend the selection to the pointer with the active granularity.
    pub(crate) fn extend_to_point(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        let offset = snap.offset_at(snap.to_content(position));
        let origin = self.drag_origin.clone();
        let sel = match self.granularity {
            Granularity::Char => Selection {
                head: offset,
                anchor: self.sel.anchor,
            },
            Granularity::Word => {
                let unit = self.doc.word_range_at(offset);
                Self::unit_extend(&origin, unit, offset)
            }
            Granularity::Paragraph => {
                let unit = self.doc.paragraph_range_at(offset);
                Self::unit_extend(&origin, unit, offset)
            }
        };
        // Drag selection is mouse-driven: keep the caret spring snapped.
        self.spring_primed = false;
        self.set_selection(sel, cx);
    }

    /// Word/paragraph drags select the union of the originally clicked unit
    /// and the unit under the pointer.
    fn unit_extend(origin: &Range<usize>, unit: Range<usize>, offset: usize) -> Selection {
        if offset < origin.start {
            Selection {
                anchor: origin.end,
                head: unit.start,
            }
        } else {
            Selection {
                anchor: origin.start,
                head: unit.end.max(origin.end),
            }
        }
    }

    /// The annotation (if any) under a window position: a note is hit anywhere
    /// over its highlighted text; a reaction within the circle around its
    /// margin marker.
    fn dot_hit(snap: &Snapshot, position: Point<Pixels>) -> Option<u64> {
        const REACH: f32 = 14.0;
        let (cx, cy) = snap.to_content(position);
        for dot in &snap.dots {
            if dot.rects.is_empty() {
                let center = snap.to_window((dot.center.0, dot.center.1));
                let dx = f32::from(position.x - center.x);
                let dy = f32::from(position.y - center.y);
                if dx.abs() <= REACH && dy.abs() <= REACH {
                    return Some(dot.id);
                }
            } else {
                for r in &dot.rects {
                    if cx >= r.x - 2.0 && cx <= r.x + r.w + 2.0 && cy >= r.y && cy <= r.y + r.h {
                        return Some(dot.id);
                    }
                }
            }
        }
        None
    }

    // ── Action handlers ────────────────────────────────────────────────────

    fn on_bold(&mut self, _: &cmd::Bold, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_style_toggle(StyleToggle::Bold, cx);
    }

    fn on_italic(&mut self, _: &cmd::Italic, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_style_toggle(StyleToggle::Italic, cx);
    }

    fn on_underline(&mut self, _: &cmd::Underline, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_style_toggle(StyleToggle::Underline, cx);
    }

    fn on_strikethrough(&mut self, _: &cmd::Strikethrough, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_style_toggle(StyleToggle::Strike, cx);
    }

    fn on_set_ink(&mut self, action: &cmd::SetInk, _: &mut Window, cx: &mut Context<Self>) {
        let toggle = match action.ink {
            None | Some(0) => StyleToggle::Ink(None),
            Some(1) => StyleToggle::Ink(Some(daisynotes_core::Ink::Rose)),
            Some(2) => StyleToggle::Ink(Some(daisynotes_core::Ink::Lavender)),
            Some(3) => StyleToggle::Ink(Some(daisynotes_core::Ink::Moss)),
            Some(_) => return,
        };
        self.apply_style_toggle(toggle, cx);
    }

    fn on_set_family(&mut self, action: &cmd::SetFamily, _: &mut Window, cx: &mut Context<Self>) {
        let family = match action.family {
            0 => daisynotes_core::FontFamily::Literata,
            1 => daisynotes_core::FontFamily::Inter,
            2 => daisynotes_core::FontFamily::Quattro,
            3 => daisynotes_core::FontFamily::Mono,
            _ => return,
        };
        let voice = Voice {
            family,
            ..self.doc.voice()
        };
        self.apply_voice(voice, cx);
    }

    fn on_set_weight(&mut self, action: &cmd::SetWeight, _: &mut Window, cx: &mut Context<Self>) {
        let voice = Voice {
            weight: action.weight.clamp(300, 700),
            ..self.doc.voice()
        };
        self.apply_voice(voice, cx);
    }

    fn on_increase_size(&mut self, _: &cmd::IncreaseSize, _: &mut Window, cx: &mut Context<Self>) {
        let voice = self.doc.voice();
        let size = policy::step_size_up(voice.size);
        self.apply_voice(Voice { size, ..voice }, cx);
    }

    fn on_decrease_size(&mut self, _: &cmd::DecreaseSize, _: &mut Window, cx: &mut Context<Self>) {
        let voice = self.doc.voice();
        let size = policy::step_size_down(voice.size);
        self.apply_voice(Voice { size, ..voice }, cx);
    }

    fn on_undo(&mut self, _: &cmd::Undo, _: &mut Window, cx: &mut Context<Self>) {
        let before = self.sel;
        if let Some(outcome) = self.doc.undo() {
            self.apply_history_outcome(outcome, before, cx);
        }
    }

    fn on_redo(&mut self, _: &cmd::Redo, _: &mut Window, cx: &mut Context<Self>) {
        let before = self.sel;
        if let Some(outcome) = self.doc.redo() {
            self.apply_history_outcome(outcome, before, cx);
        }
    }

    fn apply_history_outcome(
        &mut self,
        outcome: daisynotes_core::UndoOutcome,
        before: Selection,
        cx: &mut Context<Self>,
    ) {
        self.rebuild_text_full();
        self.marked = None;
        self.pending = None;
        if outcome.caret == (0..0) {
            // Voice-only outcome: leave the caret where it was (clamped).
            let sel = Selection {
                head: self.doc.clamp(before.head),
                anchor: self.doc.clamp(before.anchor),
            };
            self.set_selection(sel, cx);
        } else {
            let start = self.doc.clamp(outcome.caret.start);
            let end = self.doc.clamp(outcome.caret.end);
            self.set_selection(
                Selection {
                    anchor: start,
                    head: end,
                },
                cx,
            );
        }
        self.autoscroll_to = Some(self.sel.head);
        self.last_edit = None;
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    fn on_copy(&mut self, _: &cmd::Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let range = self.sel.range();
        if range.is_empty() {
            return false;
        }
        // The whole selection as a portable fragment (text + styles + lists +
        // images). Adding a formatting feature needs no change here.
        let fragment = self.doc.slice_fragment(range);
        self.write_clipboard(&fragment, cx);
        true
    }

    /// Write the fragment to the system clipboard. On macOS this fans out to
    /// several flavors (our lossless fragment, RTF for Notes/native apps, plain
    /// text); elsewhere it falls back to gpui's clipboard with the fragment JSON
    /// as metadata.
    fn write_clipboard(&self, fragment: &DocFragment, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            let _ = cx;
            daisynotes_clipboard::write_fragment(fragment);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let item = match fragment.to_json() {
                Some(json) => ClipboardItem::new_string_with_metadata(fragment.text.clone(), json),
                None => ClipboardItem::new_string(fragment.text.clone()),
            };
            cx.write_to_clipboard(item);
        }
    }

    /// Read the system clipboard as a [`Paste`]. On macOS this reads the rich
    /// flavors (fragment → RTF → plain); elsewhere it reconstructs a fragment or
    /// plain text from gpui's clipboard item.
    fn read_clipboard(&self, cx: &mut Context<Self>) -> daisynotes_clipboard::Paste {
        #[cfg(target_os = "macos")]
        {
            let _ = cx;
            daisynotes_clipboard::read()
        }
        #[cfg(not(target_os = "macos"))]
        {
            use daisynotes_clipboard::Paste;
            let Some(item) = cx.read_from_clipboard() else {
                return Paste::Empty;
            };
            let Some(text) = item.text() else {
                return Paste::Empty;
            };
            match item
                .metadata()
                .and_then(|meta| DocFragment::from_json(meta))
                .filter(|fragment| fragment.text == text)
            {
                Some(fragment) => Paste::Fragment(fragment),
                None => Paste::Plain(text),
            }
        }
    }

    fn on_cut(&mut self, _: &cmd::Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.copy_selection(cx) {
            self.doc.break_undo_group();
            self.edit_replace(self.sel.range(), "", cx);
        }
    }

    fn on_paste(&mut self, _: &cmd::Paste, _window: &mut Window, cx: &mut Context<Self>) {
        // A raw image on the clipboard (e.g. a screenshot) embeds as a block.
        if let Some(item) = cx.read_from_clipboard()
            && let Some(image) = first_clipboard_image(&item)
        {
            self.paste_image(image, cx);
            return;
        }

        use daisynotes_clipboard::Paste;
        let range = self.sel.range();
        let (end, has_images) = match self.read_clipboard(cx) {
            // Our own fragment, or rich text from another app via RTF: one
            // splice applies text + styles + lists + images as a single undo
            // group, feature-agnostically.
            Paste::Fragment(fragment) | Paste::External(fragment) => {
                if fragment.text.is_empty() && self.sel.is_empty() {
                    return;
                }
                self.doc.break_undo_group();
                let has_images = !fragment.images.is_empty();
                (self.doc.splice_fragment(range, &fragment), has_images)
            }
            Paste::Plain(text) => {
                if text.is_empty() && self.sel.is_empty() {
                    return;
                }
                self.doc.break_undo_group();
                self.doc.replace(range.clone(), &text);
                (range.start + text.len(), false)
            }
            Paste::Empty => return,
        };
        // Wholesale change: rebuild the cache and park the caret past the paste.
        self.rebuild_text_full();
        self.marked = None;
        self.pending = None;
        self.set_selection(Selection::caret(self.doc.clamp(end)), cx);
        self.last_edit = None;
        self.autoscroll_to = Some(self.sel.head);
        self.hide_pill(cx);
        cx.emit(EditorEvent::Edited);
        cx.notify();
        // Re-reference any pasted images so the app loads their blobs to render.
        if has_images {
            cx.emit(EditorEvent::ImagesReferenced);
        }
    }

    /// Embed a pasted image on its own paragraph, leaving the caret on a fresh
    /// line below it. The bytes are handed to the app (via `ImagePasted`) to
    /// persist as a blob; the decode source is held in `image_sources`.
    fn paste_image(&mut self, image: Arc<Image>, cx: &mut Context<Self>) {
        let id = image.id();
        let mime = SharedString::from(mime_of(image.format()));
        // Natural dimensions resolve at layout time (decoding may only happen
        // during prepaint); 0 means "unknown", so layout uses a default aspect
        // until the bytes decode, then the real size.
        let (w, h) = (0u32, 0u32);
        let bytes = image.bytes.clone();
        self.image_sources.insert(id, image);

        self.doc.break_undo_group();
        // Move onto a fresh line if the current one already has text.
        let line = self.doc.paragraph_range_at(self.sel.head);
        if line.start != line.end {
            self.edit_replace(self.sel.head..self.sel.head, "\n", cx);
        }
        // Open a line below for the caret; the image lives on the line above.
        let image_line = self.sel.head;
        self.edit_replace(image_line..image_line, "\n", cx);
        // Fold the image block into the newline-insert's undo group so one
        // Cmd-Z removes the image and the line it created together.
        self.doc
            .set_image_grouped(image_line, Some(ImageBlock { id, w, h, width: 0 }));
        self.after_doc_change();
        cx.emit(EditorEvent::ImagePasted { id, mime, bytes });
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    fn on_select_all(&mut self, _: &cmd::SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.set_selection(
            Selection {
                anchor: 0,
                head: self.doc.len(),
            },
            cx,
        );
        self.goal_x = None;
    }

    /// Remove a selected image and its line, undoably, parking the caret where
    /// it was. Used by Delete/Backspace while an image is selected.
    fn remove_image_at(&mut self, para: usize, cx: &mut Context<Self>) {
        self.selected_image = None;
        self.image_drag = None;
        self.doc.break_undo_group();
        self.doc.remove_image(para);
        self.rebuild_text_full();
        self.set_selection(Selection::caret(self.doc.clamp(para)), cx);
        self.hide_pill(cx);
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    fn on_backspace(&mut self, _: &cmd::Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(para) = self.selected_image {
            self.remove_image_at(para, cx);
            return;
        }
        let range = self.sel.range();
        // At the very start of a list item, Backspace outdents (then drops the
        // list) rather than merging the line into the one above.
        if range.is_empty() {
            let line = self.doc.paragraph_range_at(range.start);
            if range.start == line.start
                && let Some(attr) = self.doc.para_attr(line.start)
            {
                self.doc.set_para_list(line.start, outdent(attr));
                cx.notify();
                return;
            }
        }
        if !range.is_empty() {
            self.edit_replace(range, "", cx);
        } else if range.start > 0 {
            let start = self.doc.prev_grapheme(range.start);
            self.edit_replace(start..range.start, "", cx);
        }
    }

    fn on_indent(&mut self, _: &cmd::Indent, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_list_indent(1, cx);
    }

    fn on_outdent(&mut self, _: &cmd::Outdent, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_list_indent(-1, cx);
    }

    /// Step the indent of every list paragraph the selection touches by
    /// `delta`, clamped to `0..=MAX_LIST_INDENT`. Plain paragraphs are left
    /// untouched, so Tab/Shift-Tab only ever reshape lists.
    fn adjust_list_indent(&mut self, delta: i32, cx: &mut Context<Self>) {
        let range = self.sel.range();
        let last = self.doc.paragraph_range_at(range.end).start;
        let mut at = self.doc.paragraph_range_at(range.start).start;
        let mut changed = false;
        loop {
            if let Some(attr) = self.doc.para_attr(at) {
                let indent = (i32::from(attr.indent) + delta)
                    .clamp(0, i32::from(MAX_LIST_INDENT)) as u8;
                if indent != attr.indent {
                    self.doc
                        .set_para_list(at, Some(ListAttr { indent, ..attr }));
                    changed = true;
                }
            }
            if at >= last {
                break;
            }
            let line_end = self.doc.paragraph_range_at(at).end;
            if line_end >= self.doc.len() {
                break;
            }
            at = line_end + 1;
        }
        if changed {
            cx.notify();
        }
    }

    fn on_delete(&mut self, _: &cmd::Delete, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(para) = self.selected_image {
            self.remove_image_at(para, cx);
            return;
        }
        let range = self.sel.range();
        if !range.is_empty() {
            self.edit_replace(range, "", cx);
        } else if range.end < self.doc.len() {
            let end = self.doc.next_grapheme(range.end);
            self.edit_replace(range.end..end, "", cx);
        }
    }

    fn on_delete_word_backward(
        &mut self,
        _: &cmd::DeleteWordBackward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.sel.range();
        if !range.is_empty() {
            self.edit_replace(range, "", cx);
            return;
        }
        let start = self.doc.prev_word(range.start);
        if start < range.start {
            self.doc.break_undo_group();
            self.edit_replace(start..range.start, "", cx);
        }
    }

    fn on_delete_to_line_start(
        &mut self,
        _: &cmd::DeleteToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.sel.range();
        if !range.is_empty() {
            self.edit_replace(range, "", cx);
            return;
        }
        let row = self.visual_row(range.start);
        if row.start < range.start {
            self.doc.break_undo_group();
            self.edit_replace(row.start..range.start, "", cx);
        }
    }

    fn on_insert_newline(
        &mut self,
        _: &cmd::InsertNewline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let caret = self.sel.head;
        let line = self.doc.paragraph_range_at(caret);
        if let Some(attr) = self.doc.para_attr(line.start) {
            // Enter on an empty list item exits the list (outdent, then drop).
            if line.start == line.end && self.sel.is_empty() {
                self.doc.set_para_list(line.start, outdent(attr));
                cx.notify();
                return;
            }
            // A non-empty item splits; the continuation inherits the list.
            self.edit_replace(self.sel.range(), "\n", cx);
            let new_start = self.sel.head;
            self.doc.set_para_list(new_start, Some(attr));
            cx.notify();
            return;
        }
        self.edit_replace(self.sel.range(), "\n", cx);
    }

    /// Escape, two-step: first close transient overlays (card, then pill),
    /// then collapse the selection.
    fn on_cancel(&mut self, _: &cmd::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_image.take().is_some() {
            self.image_drag = None;
            cx.notify();
            return;
        }
        if self.card.is_some() {
            self.card = None;
            cx.notify();
            return;
        }
        if self.pill_shown {
            self.hide_pill(cx);
            return;
        }
        if !self.sel.is_empty() {
            let head = self.sel.head;
            self.caret_to(head, cx);
        }
    }

    fn on_move_left(&mut self, _: &cmd::MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let prev = self.doc.prev_grapheme(self.sel.head);
            self.caret_to(prev, cx);
        } else {
            let start = self.sel.range().start;
            self.caret_to(start, cx);
        }
    }

    fn on_move_right(&mut self, _: &cmd::MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let next = self.doc.next_grapheme(self.sel.head);
            self.caret_to(next, cx);
        } else {
            let end = self.sel.range().end;
            self.caret_to(end, cx);
        }
    }

    fn on_move_up(&mut self, _: &cmd::MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical_move(-1.0, false, cx);
    }

    fn on_move_down(&mut self, _: &cmd::MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical_move(1.0, false, cx);
    }

    fn on_move_word_left(&mut self, _: &cmd::MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let prev = self.doc.prev_word(self.sel.head);
        self.caret_to(prev, cx);
    }

    fn on_move_word_right(
        &mut self,
        _: &cmd::MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = self.doc.next_word(self.sel.head);
        self.caret_to(next, cx);
    }

    fn on_move_to_line_start(
        &mut self,
        _: &cmd::MoveToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self.visual_row(self.sel.head);
        self.caret_to(row.start, cx);
    }

    fn on_move_to_line_end(
        &mut self,
        _: &cmd::MoveToLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self.visual_row(self.sel.head);
        self.caret_to(row.end, cx);
    }

    fn on_move_to_start(&mut self, _: &cmd::MoveToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.caret_to(0, cx);
    }

    fn on_move_to_end(&mut self, _: &cmd::MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.caret_to(self.doc.len(), cx);
    }

    fn on_select_left(&mut self, _: &cmd::SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let prev = self.doc.prev_grapheme(self.sel.head);
        self.head_to(prev, cx);
    }

    fn on_select_right(&mut self, _: &cmd::SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let next = self.doc.next_grapheme(self.sel.head);
        self.head_to(next, cx);
    }

    fn on_select_up(&mut self, _: &cmd::SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical_move(-1.0, true, cx);
    }

    fn on_select_down(&mut self, _: &cmd::SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical_move(1.0, true, cx);
    }

    fn on_select_word_left(
        &mut self,
        _: &cmd::SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prev = self.doc.prev_word(self.sel.head);
        self.head_to(prev, cx);
    }

    fn on_select_word_right(
        &mut self,
        _: &cmd::SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = self.doc.next_word(self.sel.head);
        self.head_to(next, cx);
    }

    fn on_select_to_line_start(
        &mut self,
        _: &cmd::SelectToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self.visual_row(self.sel.head);
        self.head_to(row.start, cx);
    }

    fn on_select_to_line_end(
        &mut self,
        _: &cmd::SelectToLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self.visual_row(self.sel.head);
        self.head_to(row.end, cx);
    }

    fn on_select_to_start(
        &mut self,
        _: &cmd::SelectToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.head_to(0, cx);
    }

    fn on_select_to_end(&mut self, _: &cmd::SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.head_to(self.doc.len(), cx);
    }

    // ── UTF-16 offset conversion (IME protocol speaks UTF-16) ─────────────

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.text.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.text.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

impl gpui::EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.text.get(range)?.to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.sel.range()),
            reversed: self.sel.reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.sel.range());
        self.edit_replace(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.sel.range());
        let start = self.doc.clamp(range.start);
        self.edit_replace(range, new_text, cx);
        if new_text.is_empty() {
            self.marked = None;
        } else {
            self.marked = Some(start..start + new_text.len());
        }
        if let Some(new_sel) = new_selected_range_utf16 {
            // The IME's selection is relative to the marked text, so the
            // UTF-16 → UTF-8 conversion must walk the marked text itself.
            let rel_start = utf16_to_utf8_in(new_text, new_sel.start);
            let rel_end = utf16_to_utf8_in(new_text, new_sel.end);
            let head = self.doc.clamp(start + rel_end);
            let anchor = self.doc.clamp(start + rel_start);
            self.set_selection(Selection { head, anchor }, cx);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let snap = self.snapshot.clone()?;
        let range = self.range_from_utf16(&range_utf16);
        let (x, y) = snap.caret_point(range.start);
        let origin = snap.to_window((x, y));
        let (x2, _) = snap.caret_point(range.end);
        let width = (x2 - x).max(2.0);
        Some(Bounds::new(origin, size(px(width), px(snap.line_height))))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let snap = self.snapshot.clone()?;
        let offset = snap.offset_at(snap.to_content(point));
        Some(self.offset_to_utf16(offset))
    }
}

/// UTF-16 code-unit offset → UTF-8 byte offset within `text`, clamped.
fn utf16_to_utf8_in(text: &str, offset_utf16: usize) -> usize {
    let mut utf16 = 0;
    let mut utf8 = 0;
    for ch in text.chars() {
        if utf16 >= offset_utf16 {
            break;
        }
        utf16 += ch.len_utf16();
        utf8 += ch.len_utf8();
    }
    utf8
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<EditorEvent> for Editor {}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pill = overlays::render_pill(self, window, cx);
        let card = overlays::render_card(self, window, cx);
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .key_context(cmd::EDITOR_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_bold))
            .on_action(cx.listener(Self::on_italic))
            .on_action(cx.listener(Self::on_underline))
            .on_action(cx.listener(Self::on_strikethrough))
            .on_action(cx.listener(Self::on_set_ink))
            .on_action(cx.listener(Self::on_set_family))
            .on_action(cx.listener(Self::on_set_weight))
            .on_action(cx.listener(Self::on_increase_size))
            .on_action(cx.listener(Self::on_decrease_size))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_backspace))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_delete_word_backward))
            .on_action(cx.listener(Self::on_delete_to_line_start))
            .on_action(cx.listener(Self::on_insert_newline))
            .on_action(cx.listener(Self::on_indent))
            .on_action(cx.listener(Self::on_outdent))
            .on_action(cx.listener(Self::on_cancel))
            .on_action(cx.listener(Self::on_move_left))
            .on_action(cx.listener(Self::on_move_right))
            .on_action(cx.listener(Self::on_move_up))
            .on_action(cx.listener(Self::on_move_down))
            .on_action(cx.listener(Self::on_move_word_left))
            .on_action(cx.listener(Self::on_move_word_right))
            .on_action(cx.listener(Self::on_move_to_line_start))
            .on_action(cx.listener(Self::on_move_to_line_end))
            .on_action(cx.listener(Self::on_move_to_start))
            .on_action(cx.listener(Self::on_move_to_end))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_select_up))
            .on_action(cx.listener(Self::on_select_down))
            .on_action(cx.listener(Self::on_select_word_left))
            .on_action(cx.listener(Self::on_select_word_right))
            .on_action(cx.listener(Self::on_select_to_line_start))
            .on_action(cx.listener(Self::on_select_to_line_end))
            .on_action(cx.listener(Self::on_select_to_start))
            .on_action(cx.listener(Self::on_select_to_end))
            .child(element::EditorElement::new(cx.entity()))
            .children(pill)
            .children(card)
    }
}

#[cfg(test)]
mod tests {
    // The pure logic lives in `policy`, `layout`, `runs`, `clipboard`,
    // `anim`, and `notes`, each tested in place. Entity-level tests need
    // gpui's `TestAppContext`, which requires the `test-support` feature
    // this workspace doesn't enable — skipped deliberately (see report).
}
