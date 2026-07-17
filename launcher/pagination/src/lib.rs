#![recursion_limit = "4096"]

mod pagination;

use assets::BakedFont;
use ui::EventCtx;
use interactivity::hit::{HitArea, HitAreaRegistry};
use launcher_counter::CounterUi;
use launcher_volume::VolumeUi;
use pagination::{PagerState, Pages};
use taffy::Style;
use taffy::prelude::*;
use ui::widgets::{Div, Text};
use ui::{Point, RenderCommand, Widget, WidgetList, WidgetTree};
use utils::Color;

type Page1Div = Div<VolumeUi>;
type Page2Div = Div<CounterUi>;
type Page3Div = Div<CounterUi>;
type PagerType = Pages<(Page1Div, Page2Div, Page3Div)>;
type RootDiv = Div<(PagerType,)>;

pub struct PaginationUi {
    root: RootDiv,
    hit_areas: HitAreaRegistry,
    pager_width: f32,
}

impl PaginationUi {
    pub fn new(font_24: &'static BakedFont, font_100: &'static BakedFont) -> Self {
        Self {
            root: build_root(font_24, font_100),
            hit_areas: HitAreaRegistry::new(),
            pager_width: 540.0,
        }
    }
}

impl WidgetList for PaginationUi {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<taffy::NodeId> {
        vec![self.root.build_tree(tree)]
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        let pager_node = self.root.children.0.node_id();
        if let Ok(layout) = tree.layout(pager_node) {
            if layout.size.width > 0.0 {
                self.pager_width = layout.size.width;
            }
        }

        let commands = self.root.render_children(tree, parent_abs);

        self.hit_areas.clear();
        for cmd in &commands {
            if let RenderCommand::RegisterHitArea { id, rect } = cmd {
                self.hit_areas.push(HitArea {
                    id: *id,
                    rect: *rect,
                });
            }
        }

        commands
    }

    fn on_event(&mut self, ctx: &mut EventCtx) {
        let pager = &mut self.root.children.0;
        let pager_hit_id: u64 = pager.node_id().into();
        let state = &mut pager.state;

        pager.children.on_event(ctx);

        let interactivity = ctx.interactivity();
        if let Some(drag) = interactivity.gesture.drag_data() {
            match drag.state {
                interactivity::gesture::DragState::Start => {
                    let mut over_pager = false;
                    let mut over_child = false;
                    for id in self.hit_areas.hit_test_all(drag.start_position) {
                        if id == pager_hit_id {
                            over_pager = true;
                        } else {
                            over_child = true;
                        }
                    }
                    if over_pager && !over_child {
                        state.handle_drag_start(drag.start_position.x() as f64);
                    }
                }
                interactivity::gesture::DragState::Move => {
                    if state.is_dragging {
                        state.handle_drag_move(drag.current_position.x() as f64);
                    }
                }
                interactivity::gesture::DragState::End
                | interactivity::gesture::DragState::Cancel => {
                    if state.is_dragging {
                        state.handle_drag_end(self.pager_width);
                    }
                }
            }
        }
    }
}

fn build_root(font_24: &'static BakedFont, font_100: &'static BakedFont) -> RootDiv {
    let volume = VolumeUi::new(font_24, font_100);

    let counter1 = CounterUi::new(font_24, font_100);
    let counter2 = CounterUi::new(font_24, font_100);

    let page_style = Style {
        display: Display::Flex,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };

    let page1 = Div::new(page_style.clone(), volume);
    let page2 = Div::new(page_style.clone(), counter1);
    let page3 = Div::new(page_style.clone(), counter2);

    let pager_style = Style {
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        ..Default::default()
    };

    let pager_state = PagerState::new(3);
    let pager = Pages::new(pager_style, pager_state, (page1, page2, page3));

    let root_style = Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        ..Default::default()
    };

    let mut root = Div::new(root_style, (pager,));
    root.color = Color::rgb(0.08, 0.08, 0.10);

    root
}
