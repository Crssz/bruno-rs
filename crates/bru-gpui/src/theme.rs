//! gpui port of the iced app's dark palette (Bruno-styled). Colors mirror
//! `bru-app/src/theme.rs` DARK so the gpui shell matches the existing look.

use gpui::{rgb, Hsla};

macro_rules! color {
    ($name:ident, $hex:literal) => {
        pub fn $name() -> Hsla {
            rgb($hex).into()
        }
    };
}

color!(bg, 0x1a1a1a);
color!(mantle, 0x222224);
color!(surface0, 0x26292b);
color!(input_bg, 0x1b1b1b);
color!(border1, 0x333333);
color!(border2, 0x444444);
color!(text, 0xcccccc);
color!(subtext, 0xaaaaaa);
color!(muted, 0x808080);
color!(accent, 0xd9a342); // Bruno gold
color!(green, 0x73e899);
color!(blue, 0x79c8f6);
color!(orange, 0xf6ab79);
color!(red, 0xe06552);

/// HTTP method → badge color, matching the iced sidebar.
pub fn method_color(m: &str) -> Hsla {
    match m.to_ascii_uppercase().as_str() {
        "GET" => green(),
        "POST" => orange(),
        "PUT" | "PATCH" => blue(),
        "DELETE" => red(),
        _ => subtext(),
    }
}
