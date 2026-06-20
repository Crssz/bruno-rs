//! Embedded monochrome SVG icons plus a tiny gpui `AssetSource`. gpui renders an
//! SVG as an alpha mask tinted by the element's `text_color`, so the icon files
//! are plain single-color shapes and the color is chosen at each call site.

use std::borrow::Cow;

use gpui::prelude::*;
use gpui::{px, svg, AssetSource, SharedString, Svg};

pub struct Assets;

/// Map each logical asset path to its embedded bytes. Adding an icon = drop an
/// SVG in `assets/icons/` and add one line here.
macro_rules! embedded {
    ($($path:literal => $file:literal),* $(,)?) => {
        impl AssetSource for Assets {
            fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
                let bytes: Option<&'static [u8]> = match path {
                    $( $path => Some(include_bytes!($file)), )*
                    _ => None,
                };
                Ok(bytes.map(Cow::Borrowed))
            }
            fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
                Ok(Vec::new())
            }
        }
    };
}

embedded! {
    "icons/search.svg" => "../assets/icons/search.svg",
    "icons/plus.svg" => "../assets/icons/plus.svg",
    "icons/chevron-right.svg" => "../assets/icons/chevron-right.svg",
    "icons/chevron-down.svg" => "../assets/icons/chevron-down.svg",
    "icons/send.svg" => "../assets/icons/send.svg",
    "icons/play.svg" => "../assets/icons/play.svg",
    "icons/home.svg" => "../assets/icons/home.svg",
    "icons/x.svg" => "../assets/icons/x.svg",
    "icons/folder.svg" => "../assets/icons/folder.svg",
    "icons/settings.svg" => "../assets/icons/settings.svg",
}

/// A 16px icon by short name (e.g. `icon("search")`). The caller sets the color
/// via `.text_color(..)` and may override the size with `.size(..)`.
pub fn icon(name: &str) -> Svg {
    svg().size(px(16.)).path(format!("icons/{name}.svg"))
}
