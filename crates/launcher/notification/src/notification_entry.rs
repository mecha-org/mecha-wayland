use std::time::Duration;

use animation::{Animated, AnimationConfig, Easing};
use assets::BakedFont;
use taffy::prelude::*;
use taffy::{Layout, Style};
use ui::Point;

use ui::widgets::{Div, Text};
use ui::{Render, RenderCommand, WidgetList};
use utils::{Color, Rect};

use interactivity::pointer::MouseButton;
use interactivity::{DragState, InteractivityState, SwipeDirection};

pub type CardContent = (Div<()>, Div<(Text, Text)>);

// Layout constants
pub const ENTRY_HEIGHT: f32 = 80.0;
pub const CARD_RADIUS: f32 = 12.0;
pub const ENTRY_INSET: f32 = 4.0;
pub const ICON_SIZE: f32 = 44.0;
pub const ICON_RADIUS: f32 = ICON_SIZE / 2.0;
pub const CARD_H_PAD: f32 = 16.0;
pub const CARD_GAP: f32 = 14.0;
pub const TEXT_GAP: f32 = 4.0;

// Gesture thresholds
pub const OPTIONS_THRESHOLD: f32 = 140.0;
pub const FLING_OFFSCREEN_DISTANCE: f32 = 500.0;
pub const DISMISS_SIGNAL: f32 = 200.0;
pub const DRAG_THRESHOLD: f32 = 20.0;
const DRAG_CAP: f32 = 200.0;

// Z-ordering
pub const BG_Z: f32 = 0.35;

// Label rendering
pub const LABEL_OPACITY: f32 = 0.9;
pub const LABEL_FADE_RANGE: f32 = 30.0;
pub const LABEL_MIN_PADDING: f32 = 10.0;

// Animation durations
pub const SPRING_BACK_MS: u64 = 250;
pub const DISMISS_MS: u64 = 200;
pub const RECYCLE_MS: u64 = 300;

// Color palette
pub const CARD_COLOR: Color = Color::rgb(0.22, 0.22, 0.27);
pub const CARD_BORDER_COLOR: Color = Color::rgb(0.35, 0.35, 0.40);
pub const CARD_BORDER_WIDTH: f32 = 1.5;
pub const SELECTED_BORDER_COLOR: Color = Color::rgb(1.0, 0.8, 0.2);
pub const SELECTED_BORDER_WIDTH: f32 = 3.0;
pub const FLASH_COLOR: Color = Color::rgb(0.37, 0.37, 0.42);
pub const BODY_COLOR: Color = Color::rgb(0.7, 0.7, 0.75);
pub const OPTIONS_COLOR: Color = Color::rgb(0.18, 0.45, 0.75);
pub const DISMISS_BG_COLOR: Color = Color::rgb(0.75, 0.18, 0.18);

#[derive(Clone, Copy, PartialEq)]
pub enum EntryPhase {
    Idle,
    Animating,
    Swapping,
}

#[derive(Clone, Copy, PartialEq)]
enum BgLabel {
    None,
    Options,
    Dismiss,
}

#[ui::widget]
pub struct NotificationEntry<T: WidgetList> {
    #[widget(child)]
    pub card: Div<T>,
    pub swipe_offset: Animated<f32>,
    pub phase: EntryPhase,
    pub bg_color: Color,
    pub font: Option<&'static BakedFont>,
    pub bounds: Option<Rect>,
    bg_label: BgLabel,
    pub last_offset: f32,
    flash_frames: u8,
    gesture_active: bool,
    selection_handled: bool,
    prev_drag_state: Option<DragState>,
}

impl<T: WidgetList> NotificationEntry<T> {
    pub fn new(card: Div<T>) -> Self {
        let entry_style = Style {
            display: Display::Flex,
            size: Size {
                width: percent(1.0_f32),
                height: length(ENTRY_HEIGHT),
            },
            padding: taffy::Rect {
                left: length(ENTRY_INSET),
                right: length(ENTRY_INSET),
                top: zero(),
                bottom: zero(),
            },
            ..Default::default()
        };

        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style: entry_style,
            card,
            swipe_offset: Animated::static_value(0.0),
            phase: EntryPhase::Idle,
            bg_color: Color::TRANSPARENT,
            font: None,
            bounds: None,
            bg_label: BgLabel::None,
            last_offset: 0.0,
            flash_frames: 0,
            gesture_active: false,
            selection_handled: false,
            prev_drag_state: None,
        }
    }

    pub fn update(&mut self, now: Duration) -> bool {
        if self.flash_frames > 0 {
            self.flash_frames -= 1;
            if self.flash_frames == 0 {
                self.card.color = CARD_COLOR;
            }
        }

        let offset = self.swipe_offset.get(now);
        self.last_offset = offset;
        let animating = self.swipe_offset.is_animating(now);

        if self.phase == EntryPhase::Animating && !animating {
            self.phase = EntryPhase::Swapping;
            self.swipe_offset.animate_to(
                now,
                0.0,
                AnimationConfig::new(Duration::from_millis(RECYCLE_MS), Easing::EaseOut),
            );
            self.bg_label = BgLabel::None;
            self.bg_color = Color::TRANSPARENT;
            return true;
        }
        if self.phase == EntryPhase::Swapping && !animating {
            self.phase = EntryPhase::Idle;
            self.bg_label = BgLabel::None;
            self.bg_color = Color::TRANSPARENT;
            return false;
        }

        if animating || offset.abs() > f32::EPSILON {
            if self.phase == EntryPhase::Idle || self.phase == EntryPhase::Animating {
                let abs = offset.abs();
                if offset < 0.0 {
                    let (label, color) =
                        if self.phase == EntryPhase::Animating || abs > OPTIONS_THRESHOLD {
                            (BgLabel::Dismiss, DISMISS_BG_COLOR)
                        } else {
                            (BgLabel::Options, OPTIONS_COLOR)
                        };
                    self.bg_label = label;
                    self.bg_color = color;
                } else {
                    self.bg_label = BgLabel::None;
                    self.bg_color = Color::TRANSPARENT;
                }
            } else {
                self.bg_label = BgLabel::None;
                self.bg_color = Color::TRANSPARENT;
            }
            return true;
        }

        false
    }

    pub fn set_drag_offset(&mut self, offset: f32) {
        self.swipe_offset = Animated::static_value(offset);
        self.phase = EntryPhase::Idle;
    }

    pub fn dismiss(&mut self, now: Duration, direction: f32) {
        let out = if direction > 0.0 {
            FLING_OFFSCREEN_DISTANCE
        } else {
            -FLING_OFFSCREEN_DISTANCE
        };
        self.swipe_offset.animate_to(
            now,
            out,
            AnimationConfig::new(Duration::from_millis(DISMISS_MS), Easing::EaseIn),
        );
        self.phase = EntryPhase::Animating;
    }

    pub fn spring_back(&mut self, now: Duration) {
        self.swipe_offset.animate_to(
            now,
            0.0,
            AnimationConfig::new(Duration::from_millis(SPRING_BACK_MS), Easing::EaseOut),
        );
        self.phase = EntryPhase::Idle;
    }

    pub fn tap_flash(&mut self) {
        self.card.color = FLASH_COLOR;
        self.flash_frames = 4;
    }

    pub fn toggle_selected(&mut self) {
        if self.card.border_color == SELECTED_BORDER_COLOR {
            self.card.border_color = CARD_BORDER_COLOR;
            self.card.border_thickness = CARD_BORDER_WIDTH;
        } else {
            self.card.border_color = SELECTED_BORDER_COLOR;
            self.card.border_thickness = SELECTED_BORDER_WIDTH;
        }
    }

    pub fn finish_drag(&mut self, now: Duration, dx: f32) {
        if dx > 0.0 {
            self.dismiss(now, DISMISS_SIGNAL);
        } else if dx.abs() > OPTIONS_THRESHOLD {
            self.dismiss(now, -DISMISS_SIGNAL);
        } else {
            println!("Options triggered");
            self.spring_back(now);
        }
    }

    pub fn current_offset(&self) -> f32 {
        self.last_offset
    }

    pub fn handle_gesture(&mut self, now: Duration, interactivity: &InteractivityState) -> bool {
        let bounds = match self.bounds {
            Some(b) => b,
            None => return false,
        };

        let gesture = &interactivity.gesture;
        let mut ch = false;

        if let Some(d) = gesture.drag_data() {
            let state_changed = Some(d.state) != self.prev_drag_state;
            self.prev_drag_state = Some(d.state);

            match d.state {
                DragState::Start if state_changed => {
                    self.gesture_active = false;
                    self.selection_handled = false;
                    if bounds.contains_point(d.start_position) {
                        self.gesture_active = true;
                    }
                }
                DragState::Move if self.gesture_active => {
                    if state_changed || d.delta.x().abs() > f32::EPSILON {
                        self.set_drag_offset(d.total.x().clamp(-DRAG_CAP, DRAG_CAP));
                        ch = true;
                    }
                }
                DragState::End if self.gesture_active => {
                    self.gesture_active = false;

                    if let Some(sd) = gesture.swipe_data() {
                        match sd.direction {
                            SwipeDirection::Left | SwipeDirection::Right => {
                                let dx = sd.end_position.x() - sd.start_position.x();
                                self.dismiss(
                                    now,
                                    if dx > 0.0 {
                                        DISMISS_SIGNAL
                                    } else {
                                        -DISMISS_SIGNAL
                                    },
                                );
                            }
                            SwipeDirection::Up | SwipeDirection::Down => {
                                let o = self.current_offset();
                                if o.abs() >= DRAG_THRESHOLD {
                                    self.finish_drag(now, o);
                                } else {
                                    self.spring_back(now);
                                }
                            }
                        }
                    } else {
                        let o = self.current_offset();
                        let dx = d.total.x();
                        if o.abs() >= DRAG_THRESHOLD || dx.abs() >= DRAG_THRESHOLD {
                            self.finish_drag(now, if o.abs() > dx.abs() { o } else { dx });
                        } else {
                            self.spring_back(now);
                        }
                    }
                    self.selection_handled = false;
                    ch = true;
                }
                DragState::Cancel if self.gesture_active => {
                    self.gesture_active = false;
                    self.selection_handled = false;
                }
                _ => {}
            }
        }

        if interactivity.pointer.just_pressed(MouseButton::Right)
            && bounds.contains_point(interactivity.pointer.position())
        {
            self.toggle_selected();
            ch = true;
        }

        if interactivity.touch.tapped(bounds) {
            self.tap_flash();
            self.spring_back(now);
            ch = true;
        }
        if !self.selection_handled && interactivity.touch.held(bounds) {
            self.toggle_selected();
            self.selection_handled = true;
            ch = true;
        }

        ch
    }
}

pub struct PlainNotificationContent;

impl PlainNotificationContent {
    pub fn new(icon_color: Color, title: &str, body: &str) -> Div<CardContent> {
        let icon_style = Style {
            size: Size {
                width: length(ICON_SIZE),
                height: length(ICON_SIZE),
            },
            ..Default::default()
        };
        let mut icon = Div::new(icon_style, ());
        icon.color = icon_color;
        icon.border_radius = ICON_RADIUS;
        icon.z = 1.0;

        let mut title_text = Text::new(Style::default());
        title_text.text = title.to_string();
        title_text.color = Color::WHITE;
        title_text.z = 1.0;

        let mut body_text = Text::new(Style::default());
        body_text.text = body.to_string();
        body_text.color = BODY_COLOR;
        body_text.z = 1.0;

        let text_col_style = Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            gap: Size {
                width: zero(),
                height: length(TEXT_GAP),
            },
            ..Default::default()
        };
        let text_col = Div::new(text_col_style, (title_text, body_text));

        let card_style = Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            size: Size {
                width: percent(1.0_f32),
                height: length(ENTRY_HEIGHT),
            },
            padding: taffy::Rect {
                left: length(CARD_H_PAD),
                right: length(CARD_H_PAD),
                top: zero(),
                bottom: zero(),
            },
            gap: Size {
                width: length(CARD_GAP),
                height: zero(),
            },
            ..Default::default()
        };
        let mut card = Div::new(card_style, (icon, text_col));
        card.color = CARD_COLOR;
        card.border_color = CARD_BORDER_COLOR;
        card.border_radius = CARD_RADIUS;
        card.border_thickness = CARD_BORDER_WIDTH;
        card.z = 0.5;

        card
    }
}

impl<T: WidgetList> Render for NotificationEntry<T> {
    fn render(&self, layout: &Layout, abs_pos: Point) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(3);

        if let Some(font) = self.font {
            if self.bg_label != BgLabel::None {
                let bw = layout.size.width - ENTRY_INSET * 2.0;
                let bg_origin = Point::new(abs_pos.x() + ENTRY_INSET, abs_pos.y());
                cmds.push(RenderCommand::DrawQuad {
                    color: self.bg_color,
                    border_color: Color::TRANSPARENT,
                    origin: bg_origin,
                    z: BG_Z,
                    size: utils::Size::new(bw, ENTRY_HEIGHT),
                    border_radius: CARD_RADIUS,
                    border_thickness: 0.0,
                });

                let label_str = match self.bg_label {
                    BgLabel::Options => "Options",
                    BgLabel::Dismiss => "Dismiss",
                    BgLabel::None => unreachable!(),
                };
                let label_w = font.measure_width(label_str);
                let gap_width = ENTRY_INSET + self.last_offset.abs();
                let fade = ((gap_width - label_w) / LABEL_FADE_RANGE).clamp(0.0, 1.0);
                let alpha = fade * LABEL_OPACITY;
                // label above bg quad, only when gap fits label + padding
                if alpha > 0.01 && gap_width > label_w + LABEL_MIN_PADDING {
                    let gap_right = abs_pos.x() + layout.size.width;
                    let gap_left = gap_right - gap_width;
                    let ox = gap_left + (gap_width - label_w) / 2.0;
                    let oy = abs_pos.y() + ENTRY_HEIGHT / 2.0 + font.ascent / 2.0;
                    cmds.push(RenderCommand::DrawText {
                        font,
                        text: label_str.to_string(),
                        origin: Point::new(ox, oy),
                        z: BG_Z + 0.1,
                        color: Color::rgba(1.0, 1.0, 1.0, alpha),
                    });
                }
            }
        }

        let id: u64 = self.node_id.into();
        cmds.push(RenderCommand::RegisterHitArea {
            id,
            rect: utils::Rect::new(
                abs_pos.x(),
                abs_pos.y(),
                layout.size.width,
                layout.size.height,
            ),
        });

        cmds
    }
}
