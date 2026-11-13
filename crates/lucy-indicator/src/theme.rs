//! The Indicator's colours, from the Omarchy theme when there is one.

use std::path::{Path, PathBuf};

use tiny_skia::Color;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Listening, Thinking and Speaking.
    pub accent: Color,
    /// Confirming.
    pub warning: Color,
    /// The error flash.
    pub error: Color,
}

impl Palette {
    /// The palette from an Omarchy `colors.toml`. Keys it lacks, or a file
    /// that can't be read, keep the built-in colours.
    pub fn load(path: &Path) -> Palette {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Palette::from_colors_toml(&text)
    }

    fn from_colors_toml(text: &str) -> Palette {
        let table: toml::Table = text.parse().unwrap_or_default();
        let colour = |key: &str, fallback: Color| {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .and_then(parse_hex)
                .unwrap_or(fallback)
        };
        let builtin = Palette::default();
        Palette {
            accent: colour("accent", builtin.accent),
            warning: colour("yellow", builtin.warning),
            error: colour("red", builtin.error),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            accent: Color::from_rgba8(0x7a, 0xa2, 0xf7, 0xff),
            warning: Color::from_rgba8(0xe0, 0xaf, 0x68, 0xff),
            error: Color::from_rgba8(0xf7, 0x76, 0x8e, 0xff),
        }
    }
}

/// The current Omarchy theme's colours:
/// `~/.local/state/omarchy/current/theme/colors.toml`.
pub fn omarchy_colors_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".local/state/omarchy/current/theme/colors.toml"))
}

/// `#rrggbb`.
fn parse_hex(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some(Color::from_rgba8(
        channel(0)?,
        channel(2)?,
        channel(4)?,
        0xff,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_come_from_the_omarchy_theme() {
        let palette = Palette::from_colors_toml(
            r##"
            mode = "dark"
            accent = "#82FB9C"
            red = "#50f872"
            yellow = "#50f7d4"
            "##,
        );

        assert_eq!(
            palette,
            Palette {
                accent: Color::from_rgba8(0x82, 0xfb, 0x9c, 0xff),
                warning: Color::from_rgba8(0x50, 0xf7, 0xd4, 0xff),
                error: Color::from_rgba8(0x50, 0xf8, 0x72, 0xff),
            }
        );
    }

    #[test]
    fn missing_or_bad_colours_keep_the_builtin_palette() {
        let builtin = Palette::default();

        let partial = Palette::from_colors_toml(r##"accent = "#zzzzzz""##);

        assert_eq!(partial, builtin);
        assert_eq!(
            Palette::load(Path::new("/nonexistent/colors.toml")),
            builtin
        );
    }
}
