pub use utils::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum ActiveTheme {
    Dark = 0,
    Light = 1,
    Nord = 2,
    Dracula = 3,
}

impl ActiveTheme {
    pub const ALL: [Self; 4] = [Self::Dark, Self::Light, Self::Nord, Self::Dracula];

    pub fn next(self) -> Self {
        Self::ALL[(self as usize + 1) % Self::ALL.len()]
    }

    pub fn colors(self) -> &'static ThemeColors {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
            Self::Nord => &NORD,
            Self::Dracula => &DRACULA,
        }
    }
}

impl std::fmt::Display for ActiveTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    pub app_bg: Color,
    pub app_border: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub tab_bg_active: Color,
    pub tab_border_active: Color,
    pub tab_text_active: Color,
    pub tab_text_inactive: Color,
    pub btn_bg: Color,
    pub btn_border: Color,
    pub btn_text: Color,
    pub modal_backdrop: Color,
    pub modal_bg: Color,
    pub modal_border: Color,
    pub done_bg: Color,
    pub done_border: Color,
    pub stopwatch_success_bg: Color,
    pub stopwatch_success_border: Color,
    pub stopwatch_danger_bg: Color,
    pub stopwatch_danger_border: Color,
}

pub const DARK: ThemeColors = ThemeColors {
    app_bg: Color::from_rgb8(34, 34, 34),
    app_border: Color::from_rgb8(46, 46, 46),
    text_primary: Color::from_rgb8(255, 255, 255),
    text_secondary: Color::from_rgb8(179, 179, 179),
    text_muted: Color::from_rgb8(128, 128, 128),
    tab_bg_active: Color::from_rgb8(51, 51, 51),
    tab_border_active: Color::from_rgb8(77, 77, 77),
    tab_text_active: Color::from_rgb8(255, 255, 255),
    tab_text_inactive: Color::from_rgb8(128, 128, 128),
    btn_bg: Color::from_rgb8(51, 51, 56),
    btn_border: Color::from_rgb8(77, 77, 89),
    btn_text: Color::from_rgb8(230, 230, 230),
    modal_backdrop: Color::from_rgba8(0, 0, 0, 0.6),
    modal_bg: Color::from_rgb8(41, 41, 46),
    modal_border: Color::from_rgb8(71, 71, 82),
    done_bg: Color::from_rgb8(31, 107, 51),
    done_border: Color::from_rgb8(56, 158, 77),
    stopwatch_success_bg: Color::from_rgb8(31, 128, 51),
    stopwatch_success_border: Color::from_rgb8(56, 179, 77),
    stopwatch_danger_bg: Color::from_rgb8(153, 31, 41),
    stopwatch_danger_border: Color::from_rgb8(204, 56, 66),
};

pub const LIGHT: ThemeColors = ThemeColors {
    app_bg: Color::from_rgb8(245, 245, 245),
    app_border: Color::from_rgb8(217, 217, 217),
    text_primary: Color::from_rgb8(20, 20, 26),
    text_secondary: Color::from_rgb8(89, 89, 102),
    text_muted: Color::from_rgb8(140, 140, 153),
    tab_bg_active: Color::from_rgb8(224, 224, 230),
    tab_border_active: Color::from_rgb8(204, 204, 209),
    tab_text_active: Color::from_rgb8(20, 20, 26),
    tab_text_inactive: Color::from_rgb8(140, 140, 153),
    btn_bg: Color::from_rgb8(224, 224, 230),
    btn_border: Color::from_rgb8(204, 204, 209),
    btn_text: Color::from_rgb8(26, 26, 31),
    modal_backdrop: Color::from_rgba8(0, 0, 0, 0.35),
    modal_bg: Color::from_rgb8(255, 255, 255),
    modal_border: Color::from_rgb8(217, 217, 224),
    done_bg: Color::from_rgb8(38, 140, 64),
    done_border: Color::from_rgb8(64, 179, 89),
    stopwatch_success_bg: Color::from_rgb8(38, 140, 64),
    stopwatch_success_border: Color::from_rgb8(64, 179, 89),
    stopwatch_danger_bg: Color::from_rgb8(191, 38, 51),
    stopwatch_danger_border: Color::from_rgb8(230, 64, 77),
};

pub const NORD: ThemeColors = ThemeColors {
    app_bg: Color::from_rgb8(46, 56, 69),
    app_border: Color::from_rgb8(59, 69, 84),
    text_primary: Color::from_rgb8(237, 242, 245),
    text_secondary: Color::from_rgb8(217, 222, 232),
    text_muted: Color::from_rgb8(140, 161, 186),
    tab_bg_active: Color::from_rgb8(66, 82, 105),
    tab_border_active: Color::from_rgb8(89, 110, 138),
    tab_text_active: Color::from_rgb8(135, 191, 209),
    tab_text_inactive: Color::from_rgb8(140, 161, 186),
    btn_bg: Color::from_rgb8(66, 82, 105),
    btn_border: Color::from_rgb8(89, 110, 138),
    btn_text: Color::from_rgb8(237, 242, 245),
    modal_backdrop: Color::from_rgba8(20, 26, 33, 0.6),
    modal_bg: Color::from_rgb8(46, 56, 69),
    modal_border: Color::from_rgb8(89, 110, 138),
    done_bg: Color::from_rgb8(143, 184, 148),
    done_border: Color::from_rgb8(168, 209, 173),
    stopwatch_success_bg: Color::from_rgb8(143, 184, 148),
    stopwatch_success_border: Color::from_rgb8(168, 209, 173),
    stopwatch_danger_bg: Color::from_rgb8(191, 97, 107),
    stopwatch_danger_border: Color::from_rgb8(217, 122, 133),
};

pub const DRACULA: ThemeColors = ThemeColors {
    app_bg: Color::from_rgb8(38, 36, 51),
    app_border: Color::from_rgb8(69, 71, 89),
    text_primary: Color::from_rgb8(242, 242, 245),
    text_secondary: Color::from_rgb8(140, 230, 255),
    text_muted: Color::from_rgb8(102, 97, 140),
    tab_bg_active: Color::from_rgb8(69, 71, 89),
    tab_border_active: Color::from_rgb8(189, 120, 242),
    tab_text_active: Color::from_rgb8(250, 122, 176),
    tab_text_inactive: Color::from_rgb8(102, 97, 140),
    btn_bg: Color::from_rgb8(69, 71, 89),
    btn_border: Color::from_rgb8(102, 97, 140),
    btn_text: Color::from_rgb8(242, 242, 245),
    modal_backdrop: Color::from_rgba8(15, 13, 20, 0.65),
    modal_bg: Color::from_rgb8(38, 36, 51),
    modal_border: Color::from_rgb8(189, 120, 242),
    done_bg: Color::from_rgb8(79, 199, 120),
    done_border: Color::from_rgb8(105, 224, 145),
    stopwatch_success_bg: Color::from_rgb8(79, 199, 120),
    stopwatch_success_border: Color::from_rgb8(105, 224, 145),
    stopwatch_danger_bg: Color::from_rgb8(255, 84, 84),
    stopwatch_danger_border: Color::from_rgb8(255, 110, 110),
};
