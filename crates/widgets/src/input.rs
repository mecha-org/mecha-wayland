//! `Input`: an editable line of text, driven by two paths that meet
//! in this widget — `InputEdit` from the `text-input` crate's
//! `zwp_text_input_v3` end, and `KeyPress`/`KeyRepeat` from the
//! keyboard path — both readable through its context.
//!
//! The widget owns the committed string, the cursor's byte offset in
//! it, and the preedit the input method is composing; what it shows is
//! the string with the preedit inserted at the cursor. When the
//! `ContentHint::HIDDENTEXT` hint is set (as for a password field) the
//! committed characters are drawn as bullets `•` — the preedit stays
//! verbatim so the IME composition stays readable until it is
//! committed. IME edits arrive
//! as [`InputEdit`] events — one per `zwp_text_input_v3.done` — and are
//! applied in the order the protocol prescribes: the preedit replaced
//! by the cursor, the requested surroundings deleted, then the commit
//! inserted at the cursor, or the new preedit shown there. Keyboard
//! edits arrive as [`KeyPress`]/[`KeyRepeat`] carrying a
//! [`KeyMeaning`](interactivity::KeyMeaning) — `Text` inserts the
//! decoded characters, `Backspace`/`Delete`/arrows/`Home`/`End` edit
//! the string or cursor directly. After any keyboard edit the widget
//! emits [`InputFocus { focused: true }`] at itself, which the
//! `text-input` crate turns into a surrounding-text report with `Other`
//! as the cause — keeping the IME's picture of the text true across
//! edits from either path.

use app::{Build, Context, Event, Handle, Widget};
use atlas::{Atlas, FontId};
use bitflags::bitflags;
use geometry::Color;
use interactivity::{Focusable, KeyMeaning, KeyPress, KeyRepeat, Press};
use layout::{ComputedLayout, LayoutStyle, StyleContext, px};
use paint::{Paint, PaintContext, Quad};

use crate::{Div, Text, TextContext, div, text};

bitflags! {
    /// Hints about the kind of text an [`Input`] is editing, mirroring the
    /// `content_hint` bitfield of the `zwp_text_input_v3` protocol. Sent to
    /// the compositor by the `text-input` crate when text-input-v3 is
    /// available; ignored (the widget falls back to plain keyboard input)
    /// when it is not.
    ///
    /// Combine flags with `|`, e.g.
    /// `ContentHint::SENSITIVEDATA | ContentHint::HIDDENTEXT`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct ContentHint: u32 {
        /// No special behavior (the default).
        const NONE = 0x0;
        /// Suggest word completions.
        const COMPLETION = 0x1;
        /// Suggest word corrections.
        const SPELLCHECK = 0x2;
        /// Switch to uppercase letters at the start of a sentence.
        const AUTOCAPITALIZATION = 0x4;
        /// Prefer lowercase letters.
        const LOWERCASE = 0x8;
        /// Prefer uppercase letters.
        const UPPERCASE = 0x10;
        /// Prefer casing for titles and headings (can be language dependent).
        const TITLECASE = 0x20;
        /// Characters should be hidden — the widget draws each
        /// committed character as a bullet `•` (see `is_hidden`).
        const HIDDENTEXT = 0x40;
        /// Typed text should not be stored (sensitive input).
        const SENSITIVEDATA = 0x80;
        /// Just Latin characters should be entered.
        const LATIN = 0x100;
        /// The text input is multiline.
        const MULTILINE = 0x200;
        /// An on-screen way to fill in the input is already provided by
        /// the client.
        const ONSCREENINPUTPROVIDED = 0x400;
        /// Prefer not offering emoji support.
        const NOEMOJI = 0x800;
        /// The text input will display preedit text in place.
        const PREEDITSHOWN = 0x1000;
    }
}

/// The primary purpose of an [`Input`]'s text, mirroring the
/// `content_purpose` enum of the `zwp_text_input_v3` protocol. Sent to
/// the compositor by the `text-input` crate when text-input-v3 is
/// available; ignored (the widget falls back to plain keyboard input)
/// when it is not.
///
/// Combine with [`ContentHint`] for the full effect — e.g. a password
/// field pairs `ContentPurpose::Password` with
/// `ContentHint::SENSITIVEDATA | ContentHint::HIDDENTEXT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ContentPurpose {
    /// Default input, allowing all characters (the default).
    #[default]
    Normal,
    /// Allow only alphabetic characters.
    Alpha,
    /// Allow only digits.
    Digits,
    /// Input a number (including decimal separator and sign).
    Number,
    /// Input a phone number.
    Phone,
    /// Input a URL.
    Url,
    /// Input an email address.
    Email,
    /// Input a name of a person.
    Name,
    /// Input a password (combine with `ContentHint::SENSITIVEDATA`).
    Password,
    /// Input is a numeric password (combine with `ContentHint::SENSITIVEDATA`).
    Pin,
    /// Input a date.
    Date,
    /// Input a time.
    Time,
    /// Input a date and time.
    Datetime,
    /// Input for a terminal.
    Terminal,
}

/// One applied edit: everything a single `zwp_text_input_v3.done`
/// carried, emitted at the focused [`Input`] by the `text-input` crate.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InputEdit {
    /// Bytes to delete before the cursor.
    pub delete_before: u32,
    /// Bytes to delete after the cursor.
    pub delete_after: u32,
    /// The commit string: it lands at the cursor (after the requested
    /// surroundings are deleted) and the cursor moves to its end. `None`
    /// when this done carried no commit. Not exclusive with `preedit`:
    /// a single `done` may carry both, the commit inserted first, then
    /// the preedit shown at the new cursor.
    pub commit: Option<String>,
    /// The preedit the input method is composing, shown at the cursor
    /// until a commit replaces it.
    pub preedit: Option<String>,
}
impl Event for InputEdit {}

/// A focus change: emitted at an [`Input`] by the `text-input` crate
/// when it gains or loses the text focus. The widget shows its caret
/// only while focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFocus {
    pub focused: bool,
}
impl Event for InputFocus {}

pub struct Input {
    content: Handle<Text>,
    /// A thin absolutely-positioned bar overlaid on `content` at the
    /// text cursor's x; the static caret this widget shows.
    caret: Handle<Div>,
    font: FontId,
    px: u16,
    /// The caret's colour; the bar is shown in this colour while
    /// focused. Set at build from `InputBuilder::caret` (which
    /// defaults to `InputBuilder::color`).
    caret_color: Color,
    string: String,
    /// The cursor's byte offset in `string`.
    cursor: usize,
    /// The preedit shown at the cursor; never part of `string`.
    preedit: Option<String>,
    /// Whether this widget holds the text focus; the caret is shown
    /// only while this is `true`.
    focused: bool,
    /// What kind of text this field edits, for an input method that
    /// speaks text-input-v3. Read by the `text-input` crate on focus
    /// and turned into a `set_content_type` request; ignored entirely
    /// when text-input-v3 is unavailable, in which case the widget is
    /// driven only by `KeyPress`/`KeyRepeat`.
    content_hint: ContentHint,
    /// The primary purpose of this field's text. See `content_hint`.
    content_purpose: ContentPurpose,
    /// Whether a hidden field (`HIDDENTEXT`) is currently showing its
    /// real text. `false` (the default) keeps the masking on; a user
    /// toggling a "show password" button sets it `true` to reveal the
    /// typed text. Has no effect when `HIDDENTEXT` is not set.
    unmasked: bool,
}

impl Input {
    /// The committed text, without any preedit.
    pub fn text(&self) -> &str {
        &self.string
    }

    /// The cursor's byte offset in the text.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The preedit shown at the cursor, if any.
    pub fn preedit(&self) -> Option<&str> {
        self.preedit.as_deref()
    }

    /// The content hints for this field, sent to the input method when
    /// text-input-v3 is available.
    pub fn content_hint(&self) -> ContentHint {
        self.content_hint
    }

    /// The content purpose for this field, sent to the input method when
    /// text-input-v3 is available.
    pub fn content_purpose(&self) -> ContentPurpose {
        self.content_purpose
    }

    /// Whether this field is currently masking its committed text —
    /// each character is drawn as a bullet `•`. Tied to
    /// `ContentHint::HIDDENTEXT`, the protocol's "characters should be
    /// hidden" flag, so a password field
    /// (`.content_hint(ContentHint::HIDDENTEXT | ..)`) masks its text
    /// while an ordinary field shows it verbatim. The preedit the IME
    /// is composing is still shown as-is: it is temporary, and the user
    /// needs to read it until it is committed — at which point it joins
    /// `string` and is masked like the rest.
    ///
    /// Honors `unmasked`: a user-driven "show password" toggle sets
    /// that flag `true` (via [`InputContext::set_hidden`]) to reveal
    /// the real text even though the field is, by content hint, hidden.
    /// The default is `false` (masked).
    pub fn is_hidden(&self) -> bool {
        self.content_hint.contains(ContentHint::HIDDENTEXT) && !self.unmasked
    }

    /// What the widget shows: the string with the preedit at the cursor.
    /// When `is_hidden`, each committed character is replaced by a bullet
    /// `•`; the preedit is shown verbatim so the IME composition stays
    /// readable until it is committed.
    fn display(&self) -> String {
        if self.is_hidden() {
            let before = mask(&self.string[..self.cursor]);
            let after = mask(&self.string[self.cursor..]);
            match &self.preedit {
                Some(preedit) => format!("{before}{preedit}{after}"),
                None => format!("{before}{after}"),
            }
        } else {
            match &self.preedit {
                Some(preedit) => format!(
                    "{}{preedit}{}",
                    &self.string[..self.cursor],
                    &self.string[self.cursor..]
                ),
                None => self.string.clone(),
            }
        }
    }

    /// One edit, in the protocol's order: the preedit is replaced by the
    /// cursor (it never was in `string`), the requested surroundings go,
    /// then the commit lands at the cursor with the cursor at its end,
    /// and the new preedit is shown at the cursor. Commit and preedit
    /// are not exclusive: one `done` may carry both. Deletion lengths
    /// are bytes, clamped to whole characters.
    fn apply(&mut self, e: &InputEdit) {
        let start = back(&self.string, self.cursor, e.delete_before);
        let end = forward(&self.string, self.cursor, e.delete_after);
        self.string.drain(start..end);
        self.cursor = start;
        if let Some(text) = &e.commit {
            self.string.insert_str(self.cursor, text);
            self.cursor += text.len();
        }
        // The preedit is shown at the cursor; `None` clears any prior
        // preedit. Applied after the commit, per the protocol's order.
        self.preedit = e.preedit.clone();
    }

    /// Apply one key's decoded meaning: `Text` inserts at the cursor
    /// (and clears any preedit — a typed char is independent of the
    /// IME's composition), `Backspace`/`Delete` delete one char
    /// before/after the cursor, `Left`/`Right` move one char,
    /// `Home`/`End` jump to the ends. Returns `true` when the string
    /// or cursor changed.
    fn apply_meaning(&mut self, meaning: &KeyMeaning) -> bool {
        let cursor = self.cursor;
        match meaning {
            KeyMeaning::Text(text) => {
                if text.is_empty() {
                    return false;
                }
                self.string.insert_str(cursor, text);
                self.cursor = cursor + text.len();
                self.preedit = None;
                true
            }
            KeyMeaning::Backspace if cursor > 0 => {
                let start = back(&self.string, cursor, 1);
                self.string.drain(start..cursor);
                self.cursor = start;
                self.preedit = None;
                true
            }
            KeyMeaning::Delete if cursor < self.string.len() => {
                let end = forward(&self.string, cursor, 1);
                self.string.drain(cursor..end);
                self.preedit = None;
                true
            }
            KeyMeaning::Left if cursor > 0 => {
                self.cursor = back(&self.string, cursor, 1);
                self.preedit = None;
                true
            }
            KeyMeaning::Right if cursor < self.string.len() => {
                self.cursor = forward(&self.string, cursor, 1);
                self.preedit = None;
                true
            }
            KeyMeaning::Home if cursor != 0 => {
                self.cursor = 0;
                self.preedit = None;
                true
            }
            KeyMeaning::End if cursor != self.string.len() => {
                self.cursor = self.string.len();
                self.preedit = None;
                true
            }
            _ => false,
        }
    }
}

/// Return in a single-line [`Input`]: the user is done with the text.
/// Emitted at the input instead of inserting a line break; read the text
/// from the widget. A `MULTILINE` input inserts the break instead. (A line
/// break committed by an input method is text, and goes in either way.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputSubmit;
impl Event for InputSubmit {}

/// A key arrived: apply it, push the display, and tell the
/// `text-input` crate the text changed by emitting `InputFocus` — the
/// same signal a press-driven cursor move sends, which `on_focus`
/// turns into a surrounding-text report with `Other` as the cause, so
/// the IME's picture of the text stays true across keyboard edits.
fn on_key(ctx: &mut Context<'_, Input>, meaning: &KeyMeaning) {
    if *meaning == KeyMeaning::Enter {
        if ctx.me().content_hint.contains(ContentHint::MULTILINE) {
            on_key(ctx, &KeyMeaning::Text("\n".into()));
        } else {
            ctx.emit(InputSubmit, ctx.handle());
        }
        return;
    }
    if ctx.me().apply_meaning(meaning) {
        sync(ctx);
        ctx.emit(InputFocus { focused: true }, ctx.handle());
    }
}

/// `n` bytes below `index`, never splitting a character.
fn back(s: &str, index: usize, n: u32) -> usize {
    let mut i = index.saturating_sub(n as usize);
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// `n` bytes above `index`, never splitting a character.
fn forward(s: &str, index: usize, n: u32) -> usize {
    let mut i = (index + n as usize).min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// The bullet used to mask hidden characters (U+2022).
const MASK_CHAR: char = '•';

/// Replace each character of `s` with a bullet `•`, keeping the same
/// character count so hit-testing and measuring land on the same glyph
/// grid the displayed text shows. One character in → one bullet out.
fn mask(s: &str) -> String {
    s.chars().map(|_| MASK_CHAR).collect()
}

/// Convert a byte offset into a `mask` of `real` back into a byte
/// offset into `real`. `mask` replaces each character with one `•`
/// (3 bytes), so the offset into the masked string is a multiple of
/// `MASK_CHAR.len_utf8()`; dividing yields the character count, and
/// the matching byte offset in `real` is the start of that character.
/// Clamps to `real.len()` for offsets past the end.
fn unmask_offset(real: &str, masked_offset: usize) -> usize {
    let char_count = masked_offset / MASK_CHAR.len_utf8();
    real.char_indices()
        .nth(char_count)
        .map(|(i, _)| i)
        .unwrap_or(real.len())
}

pub fn input(font: FontId) -> InputBuilder {
    InputBuilder {
        font,
        px: 16,
        text: String::new(),
        // A sensible default so an empty input still has a width; a
        // caller who passes `.style(..)` replaces it wholesale — the
        // same "last write wins" rule `DivBuilder::style` follows.
        style: LayoutStyle::default().min_width(px(20.0)),
        color: Color::WHITE,
        caret: Color::WHITE,
        border: (1.0, Color::rgb(0.5, 0.5, 0.5)),
        background: None,
        // No hints: a plain text field. The `text-input` crate turns
        // these into a `set_content_type` request on focus when
        // text-input-v3 is available; with the defaults the compositor
        // raises a generic keyboard, and without text-input-v3 the
        // widget is driven only by `KeyPress`/`KeyRepeat`.
        content_hint: ContentHint::NONE,
        content_purpose: ContentPurpose::Normal,
    }
}

pub struct InputBuilder {
    font: FontId,
    px: u16,
    text: String,
    style: LayoutStyle,
    color: Color,
    caret: Color,
    border: (f32, Color),
    background: Option<Color>,
    content_hint: ContentHint,
    content_purpose: ContentPurpose,
}

impl InputBuilder {
    /// Font size in pixels; default 16.
    pub fn size(mut self, px: u16) -> Self {
        self.px = px;
        self
    }
    /// Initial content; default empty. The cursor starts at the end.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }
    /// The container's layout style. Replaces the default
    /// (`min_width(20px)`); a caller who wants a fill-width input with
    /// a min width writes
    /// `.style(LayoutStyle::default().fill().min_width(..))`.
    pub fn style(mut self, style: LayoutStyle) -> Self {
        self.style = style;
        self
    }
    /// Text colour; default white. Also sets the caret colour unless
    /// `.caret(..)` is called after.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self.caret = color;
        self
    }
    /// Caret colour, independent of `.color`; defaults to `.color`.
    pub fn caret(mut self, color: Color) -> Self {
        self.caret = color;
        self
    }
    /// Container border width and colour; default `1.0` of mid-grey.
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.border = (width, color);
        self
    }
    /// Container background colour; default transparent.
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }
    /// Content hints for the input method (text-input-v3
    /// `content_hint`), e.g. `ContentHint::SENSITIVEDATA |
    /// ContentHint::HIDDENTEXT` for a password field. Default
    /// `ContentHint::NONE`. Combined with `.content_purpose(..)` and
    /// sent on focus by the `text-input` crate; ignored when
    /// text-input-v3 is unavailable, in which case the widget falls
    /// back to plain keyboard input.
    pub fn content_hint(mut self, hint: ContentHint) -> Self {
        self.content_hint = hint;
        self
    }
    /// The primary purpose of the field's text (text-input-v3
    /// `content_purpose`), e.g. `ContentPurpose::Email` or
    /// `ContentPurpose::Password`. Default `ContentPurpose::Normal`.
    /// Combined with `.content_hint(..)` and sent on focus by the
    /// `text-input` crate; ignored when text-input-v3 is unavailable.
    pub fn content_purpose(mut self, purpose: ContentPurpose) -> Self {
        self.content_purpose = purpose;
        self
    }
}

impl Build for InputBuilder {
    type Widget = Input;
}

impl Widget for Input {
    type Builder = InputBuilder;

    fn build(
        b: Self::Builder,
        me: app::prelude::Handle<Self>,
        s: &mut app::prelude::Spawner<'_, Self>,
    ) -> Self {
        // A press takes the keyboard focus here.
        *s.component_mut::<Focusable>(me).unwrap() = Focusable(true);
        let mut container = div().border(b.border.0, b.border.1).style(b.style);
        if let Some(bg) = b.background {
            container = container.background(bg);
        }
        let container = s.spawn(me, container);
        // The text and its caret share a line box that starts where the
        // text does, so the caret's offset is the text's own, whatever
        // padding or border the container has.
        let line = s.spawn(
            container,
            div().style(LayoutStyle::default().relative().row().shrink(0.0)),
        );
        let content = s.spawn(line, text(b.font, b.text.clone()).size(b.px).color(b.color));
        // A static 1px caret, sized to the line, positioned over `content`
        // by `sync`'s `set_left`. Absolute so it never enters the flex flow.
        // Built with no paint: `sync` turns it on when the widget is
        // focused, so it is invisible until then. It spans the letters,
        // descenders included.
        let line_height = s.resource::<Atlas>().line(b.font, b.px).extent();
        let caret = s.spawn(
            line,
            div().style(
                LayoutStyle::default()
                    .absolute()
                    .size(px(1.0), px(line_height)),
            ),
        );
        s.on::<InputEdit>(me, |ctx, e| {
            ctx.me().apply(e);
            sync(ctx);
        });
        s.on::<KeyPress>(me, |ctx, e| on_key(ctx, &e.meaning));
        s.on::<KeyRepeat>(me, |ctx, e| on_key(ctx, &e.meaning));
        s.on::<InputFocus>(me, |ctx, e| {
            ctx.me().focused = e.focused;
            sync(ctx);
        });
        // A press on this widget is what focuses it, and where it lands
        // moves the caret: the press position, in window-local pixels, is
        // offset against the text content's layout rect to get an x into
        // the string, hit-tested to the closest glyph boundary. The
        // `InputFocus { focused: true }` that follows tells the
        // `text-input` crate to report the new cursor to the IM.
        s.on::<Press>(me, |ctx, e| {
            let (content, font, font_px, string, hidden) = {
                let me = ctx.me();
                (
                    me.content,
                    me.font,
                    me.px,
                    me.string.clone(),
                    me.is_hidden(),
                )
            };
            let origin = ctx
                .at(content)
                .and_then(|c| c.component::<ComputedLayout>().map(|l| l.rect.x()))
                .unwrap_or(0.0);
            let x = e.position.x - origin;
            // Hit-test against what the user sees: the masked string when
            // hidden, the real string otherwise. The masked byte offset
            // is mapped back to the real string's byte offset so the
            // cursor stays correct in the committed text.
            let offset = {
                let atlas = ctx.resource::<Atlas>();
                if hidden {
                    let masked = mask(&string);
                    let masked_off = crate::text::hit_position(&atlas, font, font_px, &masked, x);
                    unmask_offset(&string, masked_off)
                } else {
                    crate::text::hit_position(&atlas, font, font_px, &string, x)
                }
            };
            ctx.me().cursor = offset;
            sync(ctx);
            ctx.emit(InputFocus { focused: true }, ctx.handle());
        });
        let cursor = b.text.len();
        Input {
            content,
            caret,
            font: b.font,
            px: b.px,
            caret_color: b.caret,
            string: b.text,
            cursor,
            preedit: None,
            focused: false,
            content_hint: b.content_hint,
            content_purpose: b.content_purpose,
            unmasked: false,
        }
    }
}

/// Push what the widget shows to its `Text`, and park the caret over
/// it at the text cursor's x — the width of the glyphs before the
/// cursor. The caret is hidden (`Paint::None`) when the widget is not
/// focused.
fn sync(ctx: &mut Context<'_, Input>) {
    let (content, caret, font, font_px, caret_color, focused, before_cursor, display) = {
        let me = ctx.me();
        let hidden = me.is_hidden();
        // The caret sits over the glyphs before the cursor; when the
        // field is hidden those glyphs are bullets, so measure the
        // masked prefix — the real text's widths would misplace it.
        let before_cursor = if hidden {
            mask(&me.string[..me.cursor])
        } else {
            me.string[..me.cursor].to_string()
        };
        (
            me.content,
            me.caret,
            me.font,
            me.px,
            me.caret_color,
            me.focused,
            before_cursor,
            me.display(),
        )
    };
    ctx.at(content).unwrap().set_text(display);
    let x = if focused {
        let atlas = ctx.resource::<Atlas>();
        crate::text::measure(&atlas, font, font_px, &before_cursor)
    } else {
        0.0
    };
    let mut caret = ctx.at(caret).unwrap();
    if focused {
        caret.set_left(px(x));
        caret.set_paint(Paint::Quad(Quad::new(caret_color)));
    } else {
        caret.set_paint(Paint::None);
    }
}

pub trait InputContext {
    /// Replace the content. The cursor moves to the end and any preedit
    /// is dropped.
    fn set_text(&mut self, text: impl Into<String>);

    /// Show or hide the field's text. Only meaningful for a field whose
    /// `ContentHint` includes `HIDDENTEXT` (a password field): `false`
    /// (the default) masks each character as `•`, `true` reveals the
    /// typed text. A no-op for a non-hidden field. Resyncs the display.
    fn set_hidden(&mut self, hidden: bool);
}

impl InputContext for Context<'_, Input> {
    fn set_text(&mut self, text: impl Into<String>) {
        let me = self.me();
        me.string = text.into();
        me.cursor = me.string.len();
        me.preedit = None;
        sync(self);
    }

    fn set_hidden(&mut self, hidden: bool) {
        self.me().unmasked = !hidden;
        sync(self);
    }
}
