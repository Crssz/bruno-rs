//! Bruno's dark-theme palette (`themes/dark/dark.js`) and the widget styles that
//! reproduce its chrome: panels, buttons, tabs, inputs, dropdowns, checkboxes.

use iced::widget::{button, checkbox, container, pick_list, text_input};
use iced::{color, Background, Border, Color, Font};

// ── palette ─────────────────────────────────────────────────────────────────
pub const BG: Color = color!(0x1a1a1a); // background.BASE  hsl(0,0%,10%)
pub const MANTLE: Color = color!(0x222224);
pub const SURFACE0: Color = color!(0x26292b);
pub const SURFACE1: Color = color!(0x2f3133);
pub const INPUT_BG: Color = color!(0x1b1b1b);
pub const BORDER1: Color = color!(0x333333);
pub const BORDER2: Color = color!(0x444444);
pub const TEXT: Color = color!(0xcccccc); // text.BASE  hsl(0,0%,80%)
pub const SUBTEXT: Color = color!(0xaaaaaa);
pub const MUTED: Color = color!(0x808080);
pub const ACCENT: Color = color!(0xd9a342); // Bruno gold
pub const GREEN: Color = color!(0x73e899);
pub const BLUE: Color = color!(0x79c8f6);
pub const ORANGE: Color = color!(0xf6ab79);
pub const RED: Color = color!(0xe06552);
pub const TEAL: Color = color!(0x57d6bf);
pub const CYAN: Color = color!(0x7cdcf0);
pub const PURPLE: Color = color!(0xb185db);
pub const BLACK: Color = color!(0x000000);
pub const WHITE: Color = color!(0xffffff);

pub const MONO: Font = Font::MONOSPACE;
pub const BOLD: Font = Font {
    weight: iced::font::Weight::Bold,
    ..Font::DEFAULT
};

// ── containers ────────────────────────────────────────────────────────────────
pub fn panel(bg: Color, border: Option<Color>) -> container::Style {
    container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(TEXT),
        border: Border {
            color: border.unwrap_or(Color::TRANSPARENT),
            width: if border.is_some() { 1.0 } else { 0.0 },
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn rounded_panel(bg: Color, border: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(TEXT),
        border: Border {
            color: border,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

// ── buttons ───────────────────────────────────────────────────────────────────
pub fn solid_button(bg: Color, fg: Color) -> button::Style {
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: fg,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn ghost_button(status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => SURFACE1,
        _ => SURFACE0,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: TEXT,
        border: Border {
            color: BORDER2,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

/// A borderless, transparent button that only tints on hover — used for icon
/// affordances (close ×, add +, delete, chevrons).
pub fn icon_button(status: button::Status, fg: Color) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Some(Background::Color(SURFACE1)),
        _ => None,
    };
    button::Style {
        background: bg,
        text_color: fg,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A request sub-tab / response sub-tab button. Active tabs get the gold
/// underline Bruno uses (approximated with a bottom border).
pub fn tab_button(active: bool) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: if active { TEXT } else { MUTED },
        border: Border {
            color: if active { ACCENT } else { Color::TRANSPARENT },
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

/// An open-request tab in the top tab strip.
pub fn request_tab(active: bool, status: button::Status) -> button::Style {
    let bg = if active {
        BG
    } else if matches!(status, button::Status::Hovered) {
        SURFACE0
    } else {
        MANTLE
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: if active { TEXT } else { SUBTEXT },
        border: Border {
            color: BORDER1,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

/// Destructive action button (Delete, Don't Save) — solid red.
pub fn danger_button(status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => color!(0xc9543f),
        _ => RED,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: color!(0xffffff),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A floating context-menu panel (rounded, raised with a shadow).
pub fn menu_panel() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE0)),
        text_color: Some(TEXT),
        border: Border {
            color: BORDER2,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: iced::Vector::new(0.0, 3.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    }
}

/// One row inside a context menu.
pub fn menu_item(status: button::Status, danger: bool) -> button::Style {
    let fg = if danger { RED } else { TEXT };
    let bg = match status {
        button::Status::Hovered => Some(Background::Color(SURFACE1)),
        _ => Some(Background::Color(Color::TRANSPARENT)),
    };
    button::Style {
        background: bg,
        text_color: fg,
        border: Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The dimming backdrop behind a modal.
pub fn scrim() -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.55))),
        ..Default::default()
    }
}

/// A centered modal card.
pub fn modal_card() -> container::Style {
    container::Style {
        background: Some(Background::Color(MANTLE)),
        text_color: Some(TEXT),
        border: Border {
            color: BORDER2,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 24.0,
        },
        ..Default::default()
    }
}

/// A 1px horizontal separator line.
pub fn separator() -> container::Style {
    container::Style {
        background: Some(Background::Color(BORDER1)),
        ..Default::default()
    }
}

pub fn sidebar_item(selected: bool, status: button::Status) -> button::Style {
    let bg = if selected {
        SURFACE0
    } else if matches!(status, button::Status::Hovered) {
        MANTLE
    } else {
        Color::TRANSPARENT
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: TEXT,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

// ── inputs ────────────────────────────────────────────────────────────────────
pub fn input_style(_t: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => ACCENT,
        text_input::Status::Hovered => BORDER2,
        _ => BORDER1,
    };
    text_input::Style {
        background: Background::Color(INPUT_BG),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 4.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: TEXT,
        selection: ACCENT,
    }
}

/// A borderless input that blends into a table cell.
pub fn cell_input(_t: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let bg = match status {
        text_input::Status::Focused { .. } => INPUT_BG,
        _ => Color::TRANSPARENT,
    };
    let border_color = match status {
        text_input::Status::Focused { .. } => ACCENT,
        _ => Color::TRANSPARENT,
    };
    text_input::Style {
        background: Background::Color(bg),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: TEXT,
        selection: ACCENT,
    }
}

pub fn picklist_style(_t: &iced::Theme, status: pick_list::Status) -> pick_list::Style {
    let border_color = match status {
        pick_list::Status::Opened { .. } => ACCENT,
        pick_list::Status::Hovered => BORDER2,
        _ => BORDER1,
    };
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: MUTED,
        handle_color: SUBTEXT,
        background: Background::Color(INPUT_BG),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 4.0.into(),
        },
    }
}

pub fn checkbox_style(_t: &iced::Theme, status: checkbox::Status) -> checkbox::Style {
    let checked = matches!(
        status,
        checkbox::Status::Active { is_checked: true }
            | checkbox::Status::Hovered { is_checked: true }
            | checkbox::Status::Disabled { is_checked: true }
    );
    checkbox::Style {
        background: Background::Color(if checked { ACCENT } else { INPUT_BG }),
        icon_color: BLACK,
        border: Border {
            color: if checked { ACCENT } else { BORDER2 },
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: Some(TEXT),
    }
}

// ── method / status colours ────────────────────────────────────────────────────
pub fn method_color(m: &str) -> Color {
    match m.to_uppercase().as_str() {
        "GET" => GREEN,
        "POST" => BLUE,
        "PUT" | "PATCH" => ORANGE,
        "DELETE" => RED,
        "OPTIONS" => TEAL,
        "HEAD" => CYAN,
        _ => PURPLE,
    }
}

pub fn status_color(status: u16) -> Color {
    match status {
        200..=299 => GREEN,
        300..=399 => ACCENT,
        400..=599 => RED,
        _ => TEXT,
    }
}
