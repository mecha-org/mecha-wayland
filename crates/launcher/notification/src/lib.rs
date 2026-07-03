mod notification_entry;

pub use notification_entry::{CardContent, NotificationEntry, PlainNotificationContent};

use std::sync::mpsc::Receiver;
use std::time::Duration;

use animation::{Animated, AnimationConfig, Easing, monotonic_now};
use assets::BakedFont;
use interactivity::{DragState, InteractivityState, SwipeDirection};
use taffy::prelude::*;
use ui::{Point, Render, RenderCommand, Widget, WidgetList, WidgetTree};
use utils::{Color, Rect, Size};

use notification_entry::{CardContent as Card, DISMISS_SIGNAL, DRAG_THRESHOLD};

const DRAG_CAP: f32 = 200.0;
const HEADER_H: f32 = 28.0;
const PAD_X: f32 = 16.0;
const PAD_TOP: f32 = 20.0;
const HDR_GAP: f32 = 20.0;
pub const PANEL_MAX_WIDTH: f32 = 400.0;
pub const PANEL_HEIGHT: f32 = 500.0;

const SLIDE_MS: u64 = 320;
const PANEL_BG: Color = Color::rgb(0.14, 0.14, 0.16);

fn drag_state_changed(prev: &mut Option<DragState>, cur: Option<DragState>) -> bool {
    if *prev == cur {
        return false;
    }
    *prev = cur;
    true
}

type E = NotificationEntry<Card>;

fn set_entry_fonts(e: &mut E, title_font: &'static BakedFont, body_font: &'static BakedFont) {
    e.font = Some(title_font);
    let text_col = &mut e.card.children.1;
    text_col.children.0.font = Some(title_font);
    text_col.children.1.font = Some(body_font);
}

#[derive(Debug, Clone, Copy)]
pub enum NotificationCmd {
    Toggle,
    Open,
    Close,
}

pub struct NotificationUi {
    entries: (E, E, E),
    list_id: Option<taffy::NodeId>,
    root_id: Option<taffy::NodeId>,
    font_header: &'static BakedFont,
    open: bool,
    slide_y: Animated<f32>,
    cmds: Receiver<NotificationCmd>,
    active_entry: Option<usize>,
    hold_triggered: bool,
    prev_drag_state: Option<DragState>,
}

impl NotificationUi {
    fn entry(&self, i: usize) -> &E {
        match i {
            0 => &self.entries.0,
            1 => &self.entries.1,
            _ => &self.entries.2,
        }
    }
    fn entry_mut(&mut self, i: usize) -> &mut E {
        match i {
            0 => &mut self.entries.0,
            1 => &mut self.entries.1,
            _ => &mut self.entries.2,
        }
    }

    fn set_open(&mut self, open: bool, now: Duration) {
        if self.open == open {
            return;
        }
        self.open = open;
        let target = if open { 0.0 } else { PANEL_HEIGHT };
        self.slide_y.animate_to(
            now,
            target,
            AnimationConfig::new(Duration::from_millis(SLIDE_MS), Easing::EaseOut),
        );
    }

    fn toggle_open(&mut self, now: Duration) {
        self.set_open(!self.open, now);
    }

    fn drain_cmds(&mut self, now: Duration) {
        while let Ok(cmd) = self.cmds.try_recv() {
            match cmd {
                NotificationCmd::Toggle => self.toggle_open(now),
                NotificationCmd::Open => self.set_open(true, now),
                NotificationCmd::Close => self.set_open(false, now),
            }
        }
    }

    fn entry_bounds(&self, idx: usize, tree: &WidgetTree, now: Duration) -> Option<Rect> {
        let list_layout = tree.layout(self.list_id?).ok()?;
        let lx = list_layout.location.x;
        let ly = list_layout.location.y;
        let slide = self.slide_y.get(now);

        let entry = self.entry(idx);
        let el = tree.layout(entry.node_id()).ok()?;

        Some(Rect::new(
            lx + el.location.x,
            ly + el.location.y + slide,
            el.size.width,
            el.size.height,
        ))
    }

    fn offset_commands(cmds: &mut [RenderCommand], dy: f32) {
        if dy.abs() < f32::EPSILON {
            return;
        }
        for cmd in cmds.iter_mut() {
            match cmd {
                RenderCommand::DrawQuad { origin, .. }
                | RenderCommand::DrawText { origin, .. }
                | RenderCommand::DrawMonochromeSprite { origin, .. } => {
                    *origin = Point::new(origin.x(), origin.y() + dy);
                }
                RenderCommand::RegisterHitArea { rect, .. } => {
                    *rect = Rect::new(rect.x(), rect.y() + dy, rect.width(), rect.height());
                }
            }
        }
    }
}

impl WidgetList for NotificationUi {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<taffy::NodeId> {
        self.entries.0.build_children(tree);
        self.entries.1.build_children(tree);
        self.entries.2.build_children(tree);

        let list = tree
            .new_with_children(
                taffy::Style {
                    display: taffy::Display::Flex,
                    flex_direction: taffy::FlexDirection::Column,
                    size: taffy::Size {
                        width: length(PANEL_MAX_WIDTH),
                        height: Dimension::auto(),
                    },
                    min_size: taffy::Size {
                        width: length(0.0_f32),
                        height: Dimension::auto(),
                    },
                    max_size: taffy::Size {
                        width: percent(1.0_f32),
                        height: Dimension::auto(),
                    },
                    gap: taffy::Size {
                        width: length(0.0_f32),
                        height: length(12.0_f32),
                    },
                    padding: taffy::Rect {
                        left: length(PAD_X),
                        right: length(PAD_X),
                        top: length(0.0_f32),
                        bottom: length(0.0_f32),
                    },
                    ..taffy::Style::default()
                },
                &[
                    self.entries.0.node_id(),
                    self.entries.1.node_id(),
                    self.entries.2.node_id(),
                ],
            )
            .unwrap();

        self.list_id = Some(list);

        let root = tree
            .new_with_children(
                taffy::Style {
                    display: taffy::Display::Flex,
                    flex_direction: taffy::FlexDirection::Column,
                    align_items: Some(AlignItems::Center),
                    size: taffy::Size {
                        width: percent(1.0_f32),
                        height: percent(1.0_f32),
                    },
                    padding: taffy::Rect {
                        left: length(0.0_f32),
                        right: length(0.0_f32),
                        top: length(PAD_TOP + HEADER_H + HDR_GAP),
                        bottom: length(20.0_f32),
                    },
                    ..taffy::Style::default()
                },
                &[list],
            )
            .unwrap();

        self.root_id = Some(root);
        vec![root]
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        let now = monotonic_now();
        self.drain_cmds(now);

        self.entries.0.update(now);
        self.entries.1.update(now);
        self.entries.2.update(now);

        let slide = self.slide_y.get(now);
        if !self.open && !self.slide_y.is_animating(now) && slide >= PANEL_HEIGHT - 0.5 {
            return vec![];
        }

        let (surf_w, surf_h) = self
            .root_id
            .and_then(|id| tree.layout(id).ok())
            .map(|l| (l.size.width.max(1.0), l.size.height.max(1.0)))
            .unwrap_or((PANEL_MAX_WIDTH, PANEL_HEIGHT));

        let list_layout = self
            .list_id
            .and_then(|id| tree.layout(id).ok())
            .map(|l| (l.location.x, l.location.y, l.size.width, l.size.height));

        let (list_x, list_y, list_w, _list_h) =
            list_layout.unwrap_or((0.0, 0.0, PANEL_MAX_WIDTH, 0.0));
        let col_x = parent_abs.x() + list_x;
        let col_w = list_w.max(1.0);

        let mut cmds = Vec::new();

        cmds.push(RenderCommand::DrawQuad {
            color: PANEL_BG,
            border_color: Color::TRANSPARENT,
            origin: parent_abs,
            z: 0.01,
            size: Size::new(surf_w, surf_h),
            border_radius: 0.0,
            border_thickness: 0.0,
        });

        cmds.push(RenderCommand::DrawQuad {
            color: PANEL_BG,
            border_color: Color::TRANSPARENT,
            origin: Point::new(col_x, parent_abs.y()),
            z: 0.02,
            size: Size::new(col_w, PANEL_HEIGHT),
            border_radius: 16.0,
            border_thickness: 0.0,
        });

        cmds.push(RenderCommand::DrawText {
            font: self.font_header,
            text: "Notifications".to_string(),
            origin: Point::new(
                col_x + PAD_X,
                parent_abs.y() + PAD_TOP + self.font_header.ascent,
            ),
            z: 0.95,
            color: Color::WHITE,
        });

        let list_origin = Point::new(parent_abs.x() + list_x, parent_abs.y() + list_y);
        for entry in [
            &mut self.entries.0,
            &mut self.entries.1,
            &mut self.entries.2,
        ]
        .iter_mut()
        {
            let el = tree.layout(entry.node_id()).unwrap();
            let offset = entry.last_offset;
            let card_id = entry.card.node_id();

            let entry_pos = Point::new(
                list_origin.x() + el.location.x,
                list_origin.y() + el.location.y,
            );
            cmds.extend(entry.render(el, entry_pos));

            let card_layout = tree.layout(card_id).unwrap();
            let card_pos = Point::new(entry_pos.x() + offset, entry_pos.y());
            cmds.extend(entry.card.render_node(card_layout, tree, card_pos));
        }

        Self::offset_commands(&mut cmds, slide);
        cmds
    }

    fn on_event(&mut self, interactivity: &InteractivityState, tree: &mut WidgetTree) -> bool {
        let now = monotonic_now();

        if !self.open && !self.slide_y.is_animating(now) {
            return false;
        }

        let mut ch = false;
        let gesture = &interactivity.gesture;
        let dd = gesture.drag_data();
        let cur_state = dd.map(|d| d.state);

        if drag_state_changed(&mut self.prev_drag_state, cur_state) {
            match cur_state {
                Some(DragState::Start) => {
                    let d = dd.unwrap();
                    for i in 0..3 {
                        if let Some(bounds) = self.entry_bounds(i, tree, now) {
                            if bounds.contains_point(d.start_position) {
                                self.active_entry = Some(i);
                                self.hold_triggered = false;
                                break;
                            }
                        }
                    }
                }
                Some(DragState::Move) => {
                    if let Some(idx) = self.active_entry {
                        let d = dd.unwrap();
                        self.entry_mut(idx)
                            .set_drag_offset(d.total.x().clamp(-DRAG_CAP, DRAG_CAP));
                        ch = true;
                    }
                }
                Some(DragState::End) => {
                    let idx = self.active_entry.take();

                    if let Some(sd) = gesture.swipe_data() {
                        if let Some(i) = idx {
                            match sd.direction {
                                SwipeDirection::Left | SwipeDirection::Right => {
                                    let dx = sd.end_position.x() - sd.start_position.x();
                                    self.entry_mut(i).dismiss(
                                        now,
                                        if dx > 0.0 {
                                            DISMISS_SIGNAL
                                        } else {
                                            -DISMISS_SIGNAL
                                        },
                                    );
                                }
                                SwipeDirection::Up | SwipeDirection::Down => {
                                    let o = self.entry(i).current_offset();
                                    if o.abs() >= DRAG_THRESHOLD {
                                        self.entry_mut(i).finish_drag(now, o);
                                    } else {
                                        self.entry_mut(i).spring_back(now);
                                    }
                                }
                            }
                            ch = true;
                        }
                    } else if let Some(i) = idx {
                        let d = dd.unwrap();
                        let o = self.entry(i).current_offset();
                        let dx = d.total.x();
                        if o.abs() >= DRAG_THRESHOLD || dx.abs() >= DRAG_THRESHOLD {
                            self.entry_mut(i)
                                .finish_drag(now, if o.abs() > dx.abs() { o } else { dx });
                        } else {
                            self.entry_mut(i).spring_back(now);
                        }
                        ch = true;
                    }

                    self.hold_triggered = false;
                }
                Some(DragState::Cancel) => {
                    self.active_entry = None;
                    self.hold_triggered = false;
                }
                _ => {}
            }
        } else if cur_state == Some(DragState::Move) {
            if let Some(idx) = self.active_entry {
                let d = dd.unwrap();
                self.entry_mut(idx)
                    .set_drag_offset(d.total.x().clamp(-DRAG_CAP, DRAG_CAP));
                ch = true;
            }
        }

        for i in 0..3 {
            if let Some(bounds) = self.entry_bounds(i, tree, now) {
                if interactivity.touch.tapped(bounds) {
                    self.entry_mut(i).tap_flash();
                    self.entry_mut(i).spring_back(now);
                    ch = true;
                }
                if !self.hold_triggered && interactivity.touch.held(bounds) {
                    self.entry_mut(i).trigger_hold(tree);
                    self.hold_triggered = true;
                    ch = true;
                }
            }
        }

        ch
    }

    fn wants_input(&self) -> bool {
        self.open
    }
}

pub fn create_notification_ui(
    fh: &'static BakedFont,
    ft: &'static BakedFont,
    fb: &'static BakedFont,
    cmds: Receiver<NotificationCmd>,
) -> NotificationUi {
    let mk = |c: Color, t: &str, b: &str| -> E {
        let card = notification_entry::PlainNotificationContent::new(c, t, b);
        let mut e = NotificationEntry::new(card);
        set_entry_fonts(&mut e, ft, fb);
        e
    };
    NotificationUi {
        entries: (
            mk(
                Color::rgb(0.29, 0.56, 0.85),
                "Message",
                "Hey, how are you doing today?",
            ),
            mk(
                Color::rgb(0.29, 0.72, 0.45),
                "System Update",
                "A new system update is available",
            ),
            mk(
                Color::rgb(0.90, 0.55, 0.20),
                "Reminder",
                "Meeting in 10 minutes",
            ),
        ),
        list_id: None,
        root_id: None,
        font_header: fh,
        open: false,
        slide_y: Animated::static_value(PANEL_HEIGHT),
        cmds,
        active_entry: None,
        hold_triggered: false,
        prev_drag_state: None,
    }
}
