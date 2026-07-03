mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
pub mod time;
pub mod widgets;

pub use atlas::UI as ATLAS;
pub use atlas::UI_FONT_INTER_6;
pub use atlas::UI_FONT_INTER_14;
pub use atlas::UI_FONT_INTER_16;
pub use atlas::UI_FONT_INTER_24;
pub use atlas::UI_FONT_INTER_100;
pub use atlas::UI_NAV_GRADIENT;
pub use atlas::UI_NAV_HEART;
pub use atlas::UI_NAV_NOTIFICATIONS;
pub use atlas::UI_NAV_RECENT;
pub use atlas::UI_NAV_SEARCH;
pub use atlas::UI_NAV_SETTINGS;

use widgets::{
    battery::BatteryWidget,
    bluetooth::{BluetoothState, BluetoothWidget},
    clock::ClockWidget,
    wifi::WifiWidget,
};

use ui::{Point, RenderCommand, WidgetList, WidgetTree};
use utils::{Color, Size};

const BAR_HEIGHT: f32 = 36.0;
const ICON_SIZE: f32 = 24.0;
const GAP: f32 = 12.0;
const PADDING: f32 = 12.0;

pub struct StatusBarUi {
    container: Option<taffy::NodeId>,
    clock: ClockWidget,
    battery: BatteryWidget,
    bluetooth: BluetoothWidget,
    wifi: WifiWidget,
}

impl StatusBarUi {
    pub fn new() -> Self {
        let mut bluetooth = BluetoothWidget::new();
        bluetooth.state = BluetoothState::Connected;

        Self {
            container: None,
            clock: ClockWidget::new(),
            battery: BatteryWidget::new(),
            bluetooth,
            wifi: WifiWidget::new(),
        }
    }
}

fn format_clock(h: u32, m: u32, day: u32, mon: u32) -> String {
    const MONTHS: &[&str] = &[
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let hour = ((h + 11) % 12) + 1;
    let am_pm = if h < 12 { "AM" } else { "PM" };
    format!("{} {}  {:02}:{:02} {}", day, MONTHS[mon as usize], hour, m, am_pm)
}

impl WidgetList for StatusBarUi {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<taffy::NodeId> {
        let node = tree.new_leaf(taffy::Style {
            size: taffy::Size {
                width: taffy::Dimension::percent(1.0),
                height: taffy::Dimension::percent(1.0),
            },
            ..taffy::Style::default()
        }).unwrap();
        self.container = Some(node);
        vec![node]
    }

    fn render_children(&mut self, tree: &WidgetTree, _parent_abs: Point) -> Vec<RenderCommand> {
        let node = match self.container {
            Some(n) => n,
            None => return vec![],
        };
        let win_w = tree.layout(node).unwrap().size.width;

        let font = &atlas::UI_FONT_INTER_16;
        let atlas_id = atlas::UI.id;
        let mut cmds = Vec::new();

        // Clock — left-aligned
        let (h, m, _, day, mon) = time::local_time();
        cmds.push(RenderCommand::DrawText {
            font,
            text: format_clock(h, m, day, mon),
            origin: Point::new(PADDING, font.get_baseline_offset(BAR_HEIGHT)),
            z: 0.5,
            color: Color::WHITE,
        });

        // Icons — right-aligned
        let battery_w = self.battery.slot_width();
        let bluetooth_w = self.bluetooth.slot_width();
        let wifi_w = self.wifi.slot_width();

        let visible = [wifi_w, bluetooth_w, battery_w]
            .iter()
            .filter(|&&w| w > 0.0)
            .count() as f32;
        let right_w = wifi_w + bluetooth_w + battery_w + GAP * (visible - 1.0).max(0.0);
        let icon_y = (BAR_HEIGHT - ICON_SIZE) * 0.5;
        let mut cursor = win_w - PADDING - right_w;

        if bluetooth_w > 0.0 {
            let region = *self.bluetooth.sprite_region();
            cmds.push(RenderCommand::DrawMonochromeSprite {
                atlas_id,
                region,
                origin: Point::new(cursor, icon_y),
                z: 0.1,
                size: Size::new(ICON_SIZE, ICON_SIZE),
                color: Color::WHITE,
            });
            cursor += bluetooth_w + GAP;
        }

        {
            let region = *self.wifi.sprite_region();
            cmds.push(RenderCommand::DrawMonochromeSprite {
                atlas_id,
                region,
                origin: Point::new(cursor, icon_y),
                z: 0.1,
                size: Size::new(ICON_SIZE, ICON_SIZE),
                color: Color::WHITE,
            });
            cursor += wifi_w + GAP;
        }

        {
            let region = *self.battery.sprite_region();
            cmds.push(RenderCommand::DrawMonochromeSprite {
                atlas_id,
                region,
                origin: Point::new(cursor, icon_y),
                z: 0.1,
                size: Size::new(ICON_SIZE, ICON_SIZE),
                color: Color::WHITE,
            });
        }

        cmds
    }
}
