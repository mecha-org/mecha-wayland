use taffy::Style;
use ui::widgets::Text;
use ui::{Point, Render, RenderCommand, WidgetTree};

#[derive(Debug, Clone, Copy)]
pub struct ClockUpdate(pub u32, pub u32); // (hour, minute)
impl app::Event for ClockUpdate {}

#[derive(Debug, Clone, Copy)]
pub struct ClockChanged;
impl app::Event for ClockChanged {}

#[ui::widget]
pub struct ClockText {
    #[widget(child)]
    pub inner: Text,
    pub format_24h: bool,
}

impl ClockText {
    pub fn new(style: Style) -> Self {
        let text_style = Style {
            ..Default::default()
        };
        let mut inner = Text::new(text_style);
        inner.z = 0.7;

        let (h, m, ..) = crate::time::local_time();
        let mut widget = Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style,
            inner,
            format_24h: true,
        };
        widget.inner.text = widget.format_time(h, m);
        widget
    }

    /// Update the displayed time, marking the text node dirty if it changed.
    ///
    /// Returns `true` when the displayed string actually changed.
    pub fn update(&mut self, tree: &mut WidgetTree, h: u32, m: u32) -> bool {
        let new = self.format_time(h, m);
        if self.inner.text == new {
            return false;
        }
        self.inner.set_text(tree, new);
        true
    }

    fn format_time(&self, h: u32, m: u32) -> String {
        if self.format_24h {
            format!("{:02}:{:02}", h, m)
        } else {
            let hour = ((h + 11) % 12) + 1;
            let am_pm = if h < 12 { "AM" } else { "PM" };
            format!("{:02}:{:02} {}", hour, m, am_pm)
        }
    }
}

impl Render for ClockText {
    fn render(&self, _layout: &taffy::Layout, _abs_pos: Point) -> Vec<RenderCommand> {
        vec![]
    }
}
