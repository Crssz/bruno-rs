//! Bruno-styled palette with a runtime dark/light switch. Colors mirror the
//! iced app's DARK palette; the LIGHT variant is a contrast-matched inverse.
//! The active mode is a process-global flag (rendering is single-threaded on
//! gpui's foreground), so the free color fns stay call-site-simple.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{rgb, Hsla};

static DARK: AtomicBool = AtomicBool::new(true);

/// Whether the dark palette is active.
pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// Flip between dark and light.
pub fn toggle() {
    DARK.fetch_xor(true, Ordering::Relaxed);
}

macro_rules! color {
    ($name:ident, $dark:literal, $light:literal) => {
        pub fn $name() -> Hsla {
            if is_dark() {
                rgb($dark).into()
            } else {
                rgb($light).into()
            }
        }
    };
}

//        name        dark        light
color!(bg, 0x1a1a1a, 0xf6f6f7);
color!(mantle, 0x222224, 0xececee);
color!(surface0, 0x26292b, 0xe1e3e6);
color!(input_bg, 0x1b1b1b, 0xffffff);
color!(border1, 0x333333, 0xd6d8dc);
color!(border2, 0x444444, 0xc2c5ca);
color!(text, 0xcccccc, 0x1c1e22);
color!(subtext, 0xaaaaaa, 0x4a4d52);
color!(muted, 0x808080, 0x80858c);
color!(accent, 0xd9a342, 0xb07d1e); // Bruno gold (darkened for light bg)
color!(green, 0x73e899, 0x1f9d57);
color!(blue, 0x79c8f6, 0x2277b5);
color!(orange, 0xf6ab79, 0xc06a23);
color!(red, 0xe06552, 0xc0392b);

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
