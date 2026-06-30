#![recursion_limit = "4096"]

mod button;
mod slider;

use assets::{AtlasId, BakedFont};
use button::Button;
use interactivity::InteractivityState;
use slider::Slider;
use taffy::Style;
use taffy::prelude::*;
use ui::widgets::{Div, Text};
use ui::{Point, RenderCommand, Widget, WidgetList, WidgetTree};
use utils::Color;

const MIN_VOLUME: i32 = 0;
const MAX_VOLUME: i32 = 100;
const STEP_SIZE: i32 = 10;

type RowDiv = Div<(Button, Text, Button)>;
type RootDiv = Div<(Text, Slider, RowDiv)>;

pub struct VolumeUi {
    root: RootDiv,
    count: i32,
    minus_rect: utils::Rect,
    plus_rect: utils::Rect,
    slider_rect: utils::Rect,
}

impl VolumeUi {
    pub fn new(
        atlas_id: AtlasId,
        font_24: &'static BakedFont,
        font_100: &'static BakedFont,
    ) -> Self {
        Self {
            root: make_root(atlas_id, font_24, font_100),
            count: 0,
            minus_rect: utils::Rect::ZERO,
            plus_rect: utils::Rect::ZERO,
            slider_rect: utils::Rect::ZERO,
        }
    }

    fn update_value(&mut self, mut new_value: i32) {
        new_value = new_value.clamp(MIN_VOLUME, MAX_VOLUME);
        self.count = new_value;
        let slider = &mut self.root.children.1;
        slider.set_value(self.count as f32);
    }

    fn update_ui(&mut self, tree: &mut WidgetTree) {
        let text_widget = &mut self.root.children.2.children.1;
        let slider = &mut self.root.children.1;
        text_widget.set_text(tree, self.count.to_string());
        slider.update_ui(tree);
    }
}

impl WidgetList for VolumeUi {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<taffy::NodeId> {
        vec![self.root.build_tree(tree)]
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        let commands = self.root.render_children(tree, parent_abs);
        let minus_id: u64 = self.root.children.2.children.0.node_id().into();
        let plus_id: u64 = self.root.children.2.children.2.node_id().into();
        let slider_id: u64 = self.root.children.1.node_id().into();
        // TODO Does this setup happen every render? Optimize?
        for cmd in &commands {
            if let RenderCommand::RegisterHitArea { id, rect } = cmd {
                if *id == minus_id {
                    self.minus_rect = *rect;
                } else if *id == plus_id {
                    self.plus_rect = *rect;
                } else if *id == slider_id {
                    self.slider_rect = *rect;
                }
            }
        }
        commands
    }

    fn on_event(&mut self, interactivity: &InteractivityState, tree: &mut WidgetTree) -> bool {
        if interactivity.is_clicked(self.minus_rect) {
            self.update_value(self.count - STEP_SIZE);
            self.update_ui(tree);
            return true;
        }
        if interactivity.is_clicked(self.plus_rect) {
            self.update_value(self.count + STEP_SIZE);
            self.update_ui(tree);
            return true;
        }
        if interactivity.is_clicked(self.slider_rect) {
            let slider = &mut self.root.children.1;
            let y = interactivity.pointer.y;
            let new_value = slider.calculate_new_value(y, self.slider_rect);
            self.update_value(new_value as i32);
            self.update_ui(tree);
            return true;
        }
        false
    }
}

fn make_root(
    atlas_id: AtlasId,
    font_24: &'static BakedFont,
    font_100: &'static BakedFont,
) -> RootDiv {
    let mut title = Text::new(Style::default());
    title.font = Some(font_24);
    title.text = "Volume".to_string();
    title.color = Color::WHITE;
    title.z = 0.95;
    title.atlas_id = Some(atlas_id);

    let mut slider = Slider::new(MIN_VOLUME as f32, MIN_VOLUME as f32, MAX_VOLUME as f32);
    let background_color = Color::rgb(0.0, 0.47, 0.71);
    let foreground_color = Color::rgb(0.37, 0.8, 0.95);
    slider.div.color = background_color;
    slider.div.z = 0.95;
    slider.div.border_radius = 8.0;
    slider.div.border_thickness = 2.0;
    slider.div.border_color = foreground_color;
    slider.div.children.color = foreground_color;
    slider.div.children.z = 1.0;
    slider.div.children.border_radius = 8.0;
    slider.div.children.border_thickness = 2.0;
    slider.div.children.border_color = foreground_color;

    let mut minus = Button::new("-");
    minus.div.color = Color::rgb(0.2, 0.4, 0.9);
    minus.div.border_color = Color::rgb(0.4, 0.6, 1.0);
    minus.div.border_radius = 12.0;
    minus.div.border_thickness = 2.0;
    minus.div.z = 1.0;
    minus.div.children.font = Some(font_24);
    minus.div.children.color = Color::WHITE;
    minus.div.children.z = 0.4;
    minus.div.children.atlas_id = Some(atlas_id);

    let mut count_text = Text::new(Style::default());
    count_text.font = Some(font_24);
    count_text.text = "0".to_string();
    count_text.color = Color::WHITE;
    count_text.z = 0.95;
    count_text.atlas_id = Some(atlas_id);

    let mut plus = Button::new("+");
    plus.div.color = Color::rgb(0.2, 0.7, 0.3);
    plus.div.border_color = Color::rgb(0.4, 0.9, 0.5);
    plus.div.border_radius = 12.0;
    plus.div.border_thickness = 2.0;
    plus.div.z = 1.0;
    plus.div.children.font = Some(font_24);
    plus.div.children.color = Color::WHITE;
    plus.div.children.z = 0.5;
    plus.div.children.atlas_id = Some(atlas_id);

    let row_style = Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        size: Size {
            width: percent(1.0_f32),
            height: length(52.0_f32),
        },
        padding: Rect {
            left: length(60.0_f32),
            right: length(60.0_f32),
            top: zero(),
            bottom: zero(),
        },
        justify_content: Some(JustifyContent::SpaceBetween),
        align_items: Some(AlignItems::Center),
        ..Default::default()
    };
    let row = Div::new(row_style, (minus, count_text, plus));

    let root_style = Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        gap: Size {
            width: zero(),
            height: length(40.0_f32),
        },
        ..Default::default()
    };
    let mut root = Div::new(root_style, (title, slider, row));
    root.color = Color::rgb(0.16, 0.16, 0.18);
    root.border_color = Color::rgb(0.30, 0.30, 0.35);
    root.border_radius = 20.0;
    root.border_thickness = 2.0;

    root
}
