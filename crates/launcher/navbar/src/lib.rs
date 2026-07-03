//! Bottom navigation bar with swipe-to-reveal icons (ported from `nav-bar`).
//!
//! Gesture data via `InteractivityState::gesture`; icon activation is reported
//! through [`NavAction`] on a caller-provided channel.

use notification::NotificationCmd;
use std::sync::mpsc::Sender;
use std::time::Duration;

use animation::{monotonic_now, Animated, AnimationConfig, Easing};
use assets::{AtlasId, SpriteRegion};
use interactivity::{DragState, InteractivityState};
use taffy::NodeId;
use ui::{Point, RenderCommand, WidgetList, WidgetTree};
use utils::{Color, Size};

// ── Layout ───────────────────────────────────────────────────────────────────

const HANDLE_WIDTH: f32 = 100.0;
const HANDLE_HEIGHT: f32 = 12.0;
const TRIGGER_BAR_HEIGHT: f32 = 20.0;
const HANDLE_TOP_PAD: f32 = 4.0;
const HANDLE_COLOR: Color = Color::rgba(1.0, 1.0, 1.0, 0.45);

const BTN_SIZE: f32 = 52.0;
const BTN_RADIUS: f32 = 14.0;
const BTN_BG: Color = Color::from_rgb8(42, 42, 48);
const ICON_TINT: Color = Color::WHITE;
const ICON_TO_BTN_RATIO: f32 = 0.55;

const ICON_Y_OFFSETS: [f32; 5] = [60.0, 73.0, 86.0, 73.0, 60.0];
const ICON_SPACING: f32 = 84.0;

const NAV_BG_HEIGHT: f32 = 130.0;
pub const NAV_SURFACE_HEIGHT: u32 = (NAV_BG_HEIGHT + TRIGGER_BAR_HEIGHT) as u32 + 4;
pub const NAV_EXCLUSIVE_ZONE: i32 = TRIGGER_BAR_HEIGHT as i32;

const TRACKING_MS: u64 = 80;
const APPEAR_MS: u64 = 120;
const DISAPPEAR_MS: u64 = 250;
const GROWTH_MS: u64 = 180;
const ICON_GROWTH_MAX: f32 = 1.30;

const RECENTS_DWELL_MS: u64 = 150;

const SWIPE_ZONE_WIDTH: f32 = 120.0;
const SWIPE_ZONE_HEIGHT: f32 = 20.0;
const SWIPE_ACTIVATION_PX: f32 = 12.0;

fn drag_state_changed(prev: &mut Option<DragState>, cur: Option<DragState>) -> bool {
    if *prev == cur {
        return false;
    }
    *prev = cur;
    true
}

// ── Drag state ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct NavDragState {
    finger_x: f32,
    finger_y: f32,
    start_y: f32,
    zone: Option<usize>,
    last_selected: Option<usize>,
    recents_entered_at: Option<Duration>,
}

// ── NavbarUi ─────────────────────────────────────────────────────────────────

pub struct NavbarUi {
    root: Option<NodeId>,
    atlas_id: AtlasId,
    notif_tx: Sender<NotificationCmd>,
    now: Duration,
    drag_active: bool,
    drag: NavDragState,
    drag_offset: Animated<f32>,
    icon_growth: [Animated<f32>; 5],
    prev_drag_state: Option<DragState>,
}

impl NavbarUi {
    pub fn new(notif_tx: Sender<NotificationCmd>) -> Self {
        Self {
            root: None,
            atlas_id: launcher_status_bar::ATLAS.id,
            notif_tx,
            now: Duration::ZERO,
            drag_active: false,
            drag: NavDragState::default(),
            drag_offset: Animated::static_value(0.0_f32),
            icon_growth: [
                Animated::static_value(1.0_f32),
                Animated::static_value(1.0_f32),
                Animated::static_value(1.0_f32),
                Animated::static_value(1.0_f32),
                Animated::static_value(1.0_f32),
            ],
            prev_drag_state: None,
        }
    }

    fn surface_size(&self, tree: &WidgetTree) -> (f32, f32) {
        self.root
            .and_then(|n| tree.layout(n).ok())
            .map(|l| (l.size.width, l.size.height))
            .unwrap_or((0.0, 0.0))
    }

    fn in_swipe_zone(&self, x: f32, y: f32, sw: f32, sh: f32) -> bool {
        if sw == 0.0 || sh == 0.0 {
            return false;
        }
        let zone_left = (sw - SWIPE_ZONE_WIDTH) / 2.0;
        let zone_right = zone_left + SWIPE_ZONE_WIDTH;
        let zone_top = sh - SWIPE_ZONE_HEIGHT;
        x >= zone_left && x <= zone_right && y >= zone_top && y <= sh
    }

    fn icon_center(&self, i: usize, vis: f32, sw: f32, sh: f32) -> (f32, f32) {
        let cx = sw / 2.0 + (i as f32 - 2.0) * ICON_SPACING;
        let rest_y = sh - ICON_Y_OFFSETS[i];
        let hidden_y = sh + BTN_SIZE;
        let cy = hidden_y + (rest_y - hidden_y) * vis;
        (cx, cy)
    }

    fn visibility_at(&self) -> f32 {
        (self.drag_offset.get(self.now) / SWIPE_ACTIVATION_PX).clamp(0.0, 1.0)
    }

    fn icon_sprite(i: usize) -> &'static SpriteRegion {
        match i {
            0 => &launcher_status_bar::UI_NAV_HEART,
            1 => &launcher_status_bar::UI_NAV_SEARCH,
            2 => &launcher_status_bar::UI_NAV_RECENT,
            3 => &launcher_status_bar::UI_NAV_NOTIFICATIONS,
            4 => &launcher_status_bar::UI_NAV_SETTINGS,
            _ => unreachable!(),
        }
    }

    fn sector_for_point(&self, px: f32, py: f32, sw: f32, sh: f32) -> Option<usize> {
        let cx = sw / 2.0;
        let cy = sh;
        let dx = px - cx;
        let dy = cy - py;
        if dy < 4.0 {
            return None;
        }
        let angle = dx.atan2(dy);

        let mut closest: Option<usize> = None;
        let mut closest_dist = f32::MAX;
        for i in 0..5 {
            let (icx, icy) = self.icon_center(i, 1.0, sw, sh);
            let ia = (icx - cx).atan2(cy - icy);
            let ad = (angle - ia).abs();
            let ad = ad.min(std::f32::consts::TAU - ad);
            if ad < closest_dist {
                closest_dist = ad;
                closest = Some(i);
            }
        }
        let max_ad = std::f32::consts::PI / 4.0;
        if closest_dist <= max_ad {
            closest
        } else {
            None
        }
    }

    fn drag_start(&mut self, x: f32, y: f32) {
        self.drag.finger_x = x;
        self.drag.finger_y = y;
        self.drag.start_y = y;
        self.drag.zone = None;
        self.drag.recents_entered_at = None;

        if let Some(prev) = self.drag.last_selected.take() {
            self.icon_growth[prev].animate_to(
                self.now,
                1.0_f32,
                AnimationConfig::new(Duration::from_millis(TRACKING_MS), Easing::EaseInOut),
            );
        }

        self.drag_offset.animate_to(
            self.now,
            0.3_f32,
            AnimationConfig::new(Duration::from_millis(APPEAR_MS), Easing::EaseOut),
        );
    }

    fn drag_move(&mut self, x: f32, y: f32, total_dy: f32, sw: f32, sh: f32) {
        if y < 0.0 {
            self.finish_drag(false);
            return;
        }
        if x < 0.0 || x > sw || y > sh {
            self.cancel_drag();
            return;
        }

        self.drag.finger_x = x;
        self.drag.finger_y = y;
        self.drag_offset
            .set_target(self.now, (-total_dy).max(0.0));
        self.update_growth(sw, sh);
    }

    fn update_growth(&mut self, sw: f32, sh: f32) {
        let fx = self.drag.finger_x;
        let fy = self.drag.finger_y;
        let vis = self.visibility_at();
        let new_zone = self.sector_for_point(fx, fy, sw, sh);

        let zone_target = new_zone.map(|i| {
            let (cx, cy) = self.icon_center(i, vis, sw, sh);
            let dist = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let inner = BTN_SIZE * 0.5;
            let outer = ICON_SPACING * 1.5;
            let proximity = if dist <= inner {
                1.0
            } else if dist >= outer {
                0.0
            } else {
                (outer - dist) / (outer - inner)
            };
            1.0 + proximity * (ICON_GROWTH_MAX - 1.0)
        });

        if new_zone == self.drag.zone {
            if let Some(i) = new_zone {
                self.icon_growth[i].set_target(self.now, zone_target.unwrap());
            }
            return;
        }

        if let Some(old) = self.drag.zone {
            self.icon_growth[old].animate_to(
                self.now,
                1.0_f32,
                AnimationConfig::new(Duration::from_millis(TRACKING_MS), Easing::EaseInOut),
            );
        }
        self.drag.zone = new_zone;
        if let Some(new) = new_zone {
            self.icon_growth[new].animate_to(
                self.now,
                zone_target.unwrap(),
                AnimationConfig::new(Duration::from_millis(GROWTH_MS), Easing::EaseOut),
            );
        }

        if self.drag.zone == Some(2) {
            self.drag.recents_entered_at.get_or_insert(self.now);
        } else {
            self.drag.recents_entered_at = None;
        }
    }

    fn finish_drag(&mut self, extrapolated: bool) {
        self.drag_active = false;

        let target = self.drag.zone.take();

        if let Some(sel) = target {
            let dwell = self
                .drag
                .recents_entered_at
                .map_or(Duration::ZERO, |t| self.now.saturating_sub(t));
            if sel == 2
                && extrapolated
                && dwell < Duration::from_millis(RECENTS_DWELL_MS)
            {
                self.drag.last_selected = None;
            } else {
                self.drag.last_selected = Some(sel);
                self.icon_growth[sel].animate_to(
                    self.now,
                    ICON_GROWTH_MAX,
                    AnimationConfig::new(Duration::from_millis(GROWTH_MS), Easing::EaseOut),
                );
                if sel == 3 {
                    let _ = self.notif_tx.send(NotificationCmd::Toggle);
                }
            };
        }

        self.drag_offset.animate_to(
            self.now,
            0.0_f32,
            AnimationConfig::new(Duration::from_millis(DISAPPEAR_MS), Easing::EaseIn),
        );
    }

    fn cancel_drag(&mut self) {
        self.drag_active = false;

        if let Some(old) = self.drag.zone.take() {
            self.icon_growth[old].animate_to(
                self.now,
                1.0_f32,
                AnimationConfig::new(Duration::from_millis(TRACKING_MS), Easing::EaseInOut),
            );
        }

        self.drag_offset.animate_to(
            self.now,
            0.0_f32,
            AnimationConfig::new(Duration::from_millis(DISAPPEAR_MS), Easing::EaseIn),
        );
    }

    fn is_animating(&self) -> bool {
        self.drag_offset.is_animating(self.now)
            || self.icon_growth.iter().any(|g| g.is_animating(self.now))
    }
}

impl WidgetList for NavbarUi {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        let node = tree
            .new_leaf(taffy::Style {
                size: taffy::Size {
                    width: taffy::Dimension::percent(1.0),
                    height: taffy::Dimension::percent(1.0),
                },
                ..taffy::Style::default()
            })
            .unwrap();
        self.root = Some(node);
        vec![node]
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        self.now = monotonic_now();
        let (sw, sh) = self.surface_size(tree);
        if sw == 0.0 || sh == 0.0 {
            return vec![];
        }

        let vis = self.visibility_at();
        let mut cmds = Vec::new();

        if vis > 0.005 {
            let grad_h = NAV_BG_HEIGHT * vis;
            let g = &launcher_status_bar::UI_NAV_GRADIENT;
            let region_y = (g.h - grad_h).max(0.0);
            let region_h = grad_h.min(g.h);
            cmds.push(RenderCommand::DrawMonochromeSprite {
                atlas_id: self.atlas_id,
                region: SpriteRegion {
                    x: g.x,
                    y: g.y + region_y,
                    w: g.w,
                    h: region_h,
                },
                origin: Point::new(parent_abs.x(), parent_abs.y() + sh - grad_h),
                z: 0.04,
                size: Size::new(sw, grad_h),
                color: Color::rgba(0.0, 0.0, 0.0, vis),
            });

            for i in 0..5 {
                let (cx, cy) = self.icon_center(i, vis, sw, sh);
                let growth = self.icon_growth[i].get(self.now);
                let scale = vis * growth;
                let half = BTN_SIZE * scale / 2.0;

                cmds.push(RenderCommand::DrawQuad {
                    color: BTN_BG,
                    border_color: Color::TRANSPARENT,
                    origin: Point::new(parent_abs.x() + cx - half, parent_abs.y() + cy - half),
                    z: 0.1 + i as f32 * 0.01,
                    size: Size::new(BTN_SIZE * scale, BTN_SIZE * scale),
                    border_radius: BTN_RADIUS * scale,
                    border_thickness: 0.0,
                });

                let icon_scale = scale * ICON_TO_BTN_RATIO;
                let icon_sz = BTN_SIZE * icon_scale;
                let r = Self::icon_sprite(i);
                cmds.push(RenderCommand::DrawMonochromeSprite {
                    atlas_id: self.atlas_id,
                    region: SpriteRegion {
                        x: r.x,
                        y: r.y,
                        w: r.w,
                        h: r.h,
                    },
                    origin: Point::new(
                        parent_abs.x() + cx - icon_sz / 2.0,
                        parent_abs.y() + cy - icon_sz / 2.0,
                    ),
                    z: 0.2 + i as f32 * 0.01,
                    size: Size::new(icon_sz, icon_sz),
                    color: Color {
                        a: vis,
                        ..ICON_TINT
                    },
                });
            }
        }

        let handle_x = (sw - HANDLE_WIDTH) / 2.0;
        let handle_y = sh - TRIGGER_BAR_HEIGHT + HANDLE_TOP_PAD;
        cmds.push(RenderCommand::DrawQuad {
            color: HANDLE_COLOR,
            border_color: Color::TRANSPARENT,
            origin: Point::new(parent_abs.x() + handle_x, parent_abs.y() + handle_y),
            z: 0.01,
            size: Size::new(HANDLE_WIDTH, HANDLE_HEIGHT),
            border_radius: HANDLE_HEIGHT / 2.0,
            border_thickness: 0.0,
        });

        cmds
    }

    fn on_event(&mut self, interactivity: &InteractivityState, tree: &mut WidgetTree) -> bool {
        self.now = monotonic_now();
        let (sw, sh) = self.surface_size(tree);
        let mut changed = self.is_animating();

        let gesture = &interactivity.gesture;
        let dd = gesture.drag_data();
        let cur_state = dd.map(|d| d.state);

        if drag_state_changed(&mut self.prev_drag_state, cur_state) {

            match cur_state {
                Some(DragState::Start) => {
                    let d = dd.unwrap();
                    if !self.drag_active && self.in_swipe_zone(d.start_position.x(), d.start_position.y(), sw, sh) {
                        self.drag_active = true;
                        self.drag_start(d.start_position.x(), d.start_position.y());
                        changed = true;
                    }
                }
                Some(DragState::Move) if self.drag_active => {
                    let d = dd.unwrap();
                    self.drag_move(d.current_position.x(), d.current_position.y(), d.total.y(), sw, sh);
                    changed = true;
                }
                Some(DragState::End) if self.drag_active => {
                    let extrapolated = gesture.swipe_data().is_some();
                    self.finish_drag(extrapolated);
                    changed = true;
                }
                Some(DragState::Cancel) if self.drag_active => {
                    self.cancel_drag();
                    changed = true;
                }
                _ => {}
            }
        } else if cur_state == Some(DragState::Move) && self.drag_active {
            let d = dd.unwrap();
            if d.delta.x().abs() > 0.01 || d.delta.y().abs() > 0.01 {
                self.drag_move(d.current_position.x(), d.current_position.y(), d.total.y(), sw, sh);
                changed = true;
            }
        }

        changed
    }
}
