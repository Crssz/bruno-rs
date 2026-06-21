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

/// Set the palette explicitly (used to apply a persisted preference on startup).
pub fn set_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
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
// Extended hue palette (Bruno's per-mode hues) for method / protocol badges.
color!(indigo, 0x79c8f6, 0x404aa6);
color!(teal, 0x4ed9b8, 0x2e8a85);
color!(cyan, 0x7fd6f3, 0x30a0b9);
color!(purple, 0xd28bef, 0x8b41b2);

// Gold tab underline (Bruno's `primary.strong`).
color!(tab_underline, 0xeab455, 0xd58a2a);
// Subtlest border (`border0`): card outlines, indent guides, tab seams.
color!(border0, 0x2a2a2a, 0xefefef);

// Bottom status bar.
color!(statusbar_bg, 0x1e1e1e, 0xf6f6f6);
color!(statusbar_border, 0x323233, 0xe9e9e9);
color!(statusbar_text, 0xa9a9a9, 0x646464);

/// Draft / unsaved indicator (Bruno's `draftColor`, identical in both modes).
pub fn draft_dot() -> Hsla {
    rgb(0xcc7b1b).into()
}

/// HTTP method → badge color, following Bruno's per-mode `request.methods` map.
pub fn method_color(m: &str) -> Hsla {
    match m.to_ascii_uppercase().as_str() {
        "GET" => green(),
        "POST" => {
            if is_dark() {
                indigo()
            } else {
                purple()
            }
        }
        "PUT" => orange(),
        "PATCH" => {
            if is_dark() {
                orange()
            } else {
                purple()
            }
        }
        "DELETE" => red(),
        "OPTIONS" => teal(),
        "HEAD" => cyan(),
        _ => subtext(),
    }
}

#[cfg(test)]
mod cov_tests {
    use super::*;

    // Snapshot/restore the global flag around each test so ordering can't leak.
    struct DarkGuard(bool);
    impl DarkGuard {
        fn capture() -> Self {
            DarkGuard(is_dark())
        }
    }
    impl Drop for DarkGuard {
        fn drop(&mut self) {
            set_dark(self.0);
        }
    }

    #[test]
    fn set_dark_and_is_dark_round_trip() {
        let _g = DarkGuard::capture();
        set_dark(true);
        assert!(is_dark());
        set_dark(false);
        assert!(!is_dark());
        set_dark(true);
        assert!(is_dark());
    }

    #[test]
    fn toggle_flips_the_flag() {
        let _g = DarkGuard::capture();
        set_dark(true);
        toggle();
        assert!(!is_dark());
        toggle();
        assert!(is_dark());
    }

    // Every color! accessor must resolve in dark mode without panicking and yield
    // a fully-opaque color (rgb() -> alpha 1.0).
    #[test]
    fn all_colors_resolve_in_dark_mode() {
        let _g = DarkGuard::capture();
        set_dark(true);
        let all = [
            bg(),
            mantle(),
            surface0(),
            input_bg(),
            border1(),
            border2(),
            text(),
            subtext(),
            muted(),
            accent(),
            green(),
            blue(),
            orange(),
            red(),
            indigo(),
            teal(),
            cyan(),
            purple(),
            tab_underline(),
            border0(),
            statusbar_bg(),
            statusbar_border(),
            statusbar_text(),
            draft_dot(),
        ];
        for c in all {
            assert!((c.a - 1.0).abs() < f32::EPSILON);
        }
    }

    // Same accessors must resolve in light mode too.
    #[test]
    fn all_colors_resolve_in_light_mode() {
        let _g = DarkGuard::capture();
        set_dark(false);
        let all = [
            bg(),
            mantle(),
            surface0(),
            input_bg(),
            border1(),
            border2(),
            text(),
            subtext(),
            muted(),
            accent(),
            green(),
            blue(),
            orange(),
            red(),
            indigo(),
            teal(),
            cyan(),
            purple(),
            tab_underline(),
            border0(),
            statusbar_bg(),
            statusbar_border(),
            statusbar_text(),
        ];
        for c in all {
            assert!((c.a - 1.0).abs() < f32::EPSILON);
        }
    }

    // Dark and light variants differ for the representative tokens that have
    // distinct dark/light literals.
    #[test]
    fn dark_and_light_differ() {
        let _g = DarkGuard::capture();

        set_dark(true);
        let bg_dark = bg();
        let text_dark = text();
        let accent_dark = accent();
        let mantle_dark = mantle();
        let border1_dark = border1();
        let green_dark = green();
        let blue_dark = blue();

        set_dark(false);
        let bg_light = bg();
        let text_light = text();
        let accent_light = accent();
        let mantle_light = mantle();
        let border1_light = border1();
        let green_light = green();
        let blue_light = blue();

        assert!(bg_dark != bg_light);
        assert!(text_dark != text_light);
        assert!(accent_dark != accent_light);
        assert!(mantle_dark != mantle_light);
        assert!(border1_dark != border1_light);
        assert!(green_dark != green_light);
        assert!(blue_dark != blue_light);
    }

    // draft_dot is mode-independent (same literal in both modes).
    #[test]
    fn draft_dot_is_mode_independent() {
        let _g = DarkGuard::capture();
        set_dark(true);
        let d = draft_dot();
        set_dark(false);
        let l = draft_dot();
        assert!(d == l);
    }

    // method_color: every recognized verb maps to its expected accessor in dark
    // mode, and the case-insensitive + fallback paths are exercised.
    #[test]
    fn method_color_dark_mode_branches() {
        let _g = DarkGuard::capture();
        set_dark(true);
        assert!(method_color("GET") == green());
        assert!(method_color("POST") == indigo());
        assert!(method_color("PUT") == orange());
        assert!(method_color("PATCH") == orange());
        assert!(method_color("DELETE") == red());
        assert!(method_color("OPTIONS") == teal());
        assert!(method_color("HEAD") == cyan());
        // Unknown verb -> subtext fallback.
        assert!(method_color("TRACE") == subtext());
        assert!(method_color("") == subtext());
    }

    // method_color: the POST/PATCH branches diverge in light mode (purple).
    #[test]
    fn method_color_light_mode_branches() {
        let _g = DarkGuard::capture();
        set_dark(false);
        assert!(method_color("GET") == green());
        assert!(method_color("POST") == purple());
        assert!(method_color("PUT") == orange());
        assert!(method_color("PATCH") == purple());
        assert!(method_color("DELETE") == red());
        assert!(method_color("OPTIONS") == teal());
        assert!(method_color("HEAD") == cyan());
        assert!(method_color("WHATEVER") == subtext());
    }

    // The .to_ascii_uppercase() normalization path: lowercase / mixed-case verbs
    // resolve identically to their uppercase form.
    #[test]
    fn method_color_is_case_insensitive() {
        let _g = DarkGuard::capture();
        set_dark(true);
        assert!(method_color("get") == method_color("GET"));
        assert!(method_color("Post") == method_color("POST"));
        assert!(method_color("deLeTe") == method_color("DELETE"));
        assert!(method_color("options") == method_color("OPTIONS"));
    }

    // POST diverges between modes (indigo dark vs purple light); confirm the
    // mode actually drives the inner branch.
    #[test]
    fn method_color_post_differs_by_mode() {
        let _g = DarkGuard::capture();
        set_dark(true);
        let post_dark = method_color("POST");
        set_dark(false);
        let post_light = method_color("POST");
        assert!(post_dark != post_light);
    }

    // PATCH likewise diverges (orange dark vs purple light).
    #[test]
    fn method_color_patch_differs_by_mode() {
        let _g = DarkGuard::capture();
        set_dark(true);
        let patch_dark = method_color("PATCH");
        set_dark(false);
        let patch_light = method_color("PATCH");
        assert!(patch_dark != patch_light);
    }
}
