use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use atspi::Role;

use crate::accessible::FocusEvents;
use crate::capture::{Look, Placement};
use crate::hyprland::{Hyprland, Monitor, Window};

const MODEL_LONG_EDGE: f64 = 1568.0;

const MODEL_PIXELS: f64 = 1_150_000.0;

/// A part of the desktop in Hyprland's layout coordinates (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// An encoded picture of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

/// A point on the desktop in layout coordinates, with the whole desktop's bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    /// The smallest region holding every monitor.
    pub desktop: Region,
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// Which way a scroll turns the wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// A modifier held down through a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Super,
}

/// A key press: modifiers held around one key, named by its XKB keysym.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub modifiers: Vec<Modifier>,
    pub keysym: String,
}

/// A process ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pid(pub u32);

/// The window with the keyboard focus, named by the process that owns it and its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedWindow {
    pub pid: Pid,
    pub title: String,
}

/// What [`Screen::focused_role`] returns: the focused element's role, or why it can't be found.
pub type RoleFound<'a> = Pin<Box<dyn Future<Output = Result<Role, String>> + Send + 'a>>;

/// What a [`Screen`] call returns once it has finished.
pub type Captured<'a> = Pin<Box<dyn Future<Output = Result<Image, String>> + Send + 'a>>;

/// What a [`Screen`] action returns once it is done.
pub type Acted<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

/// The Plugin's way to the screen itself, which tests replace with a fake.
///
/// Each method's error says why it couldn't, ready for the model.
pub trait Screen: Send + Sync {
    /// Captures `region` at `scale` image pixels per logical pixel.
    fn capture(&self, region: Region, scale: f64) -> Captured<'_>;

    /// Moves the pointer to `at` and clicks `button` there.
    fn click(&self, at: Point, button: Button) -> Acted<'_>;

    /// Moves the pointer to `at` and turns the wheel `notches` clicks `direction`.
    fn scroll(&self, at: Point, direction: Direction, notches: u32) -> Acted<'_>;

    /// Presses and releases `combo`.
    fn key(&self, combo: Combo) -> Acted<'_>;

    /// Types `text` as though on a keyboard, a newline as Enter.
    fn type_text(&self, text: String) -> Acted<'_>;

    /// Finds the AT-SPI role of the element with the keyboard focus in `window`.
    fn focused_role(&self, window: FocusedWindow) -> RoleFound<'_>;
}

pub(crate) struct Wayland {
    hyprland: Hyprland,
    focus: FocusEvents,
}

impl Wayland {
    pub(crate) fn new(socket: PathBuf) -> Wayland {
        Wayland {
            hyprland: Hyprland::new(socket),
            focus: FocusEvents::default(),
        }
    }
}

impl Screen for Wayland {
    fn click(&self, at: Point, button: Button) -> Acted<'_> {
        crate::wayland::act(move |input| input.click(at, button))
    }

    fn scroll(&self, at: Point, direction: Direction, notches: u32) -> Acted<'_> {
        crate::wayland::act(move |input| input.scroll(at, direction, notches))
    }

    fn key(&self, combo: Combo) -> Acted<'_> {
        crate::wayland::act(move |input| input.key(&combo))
    }

    fn type_text(&self, text: String) -> Acted<'_> {
        crate::wayland::act(move |input| input.type_text(&text))
    }

    fn focused_role(&self, window: FocusedWindow) -> RoleFound<'_> {
        self.focus.follow();
        Box::pin(crate::accessible::focused_role(&self.focus, window))
    }

    fn capture(&self, region: Region, scale: f64) -> Captured<'_> {
        Box::pin(async move {
            let monitors = self.hyprland.monitors().await?;
            let monitor =
                holding(&monitors, region).ok_or("no monitor shows that part of the screen")?;
            let look = Look {
                monitor: Placement::from(monitor),
                region,
                image_scale: scale,
            };
            crate::screencopy::capture(monitor.name.clone(), look).await
        })
    }
}

pub(crate) fn monitor(monitor: &Monitor) -> Region {
    let logical = |pixels: u32| (f64::from(pixels) / monitor.scale).round() as u32;
    let (width, height) = (logical(monitor.width), logical(monitor.height));
    let rotated = monitor.transform % 2 == 1;
    let (width, height) = if rotated {
        (height, width)
    } else {
        (width, height)
    };
    Region {
        x: monitor.x,
        y: monitor.y,
        width,
        height,
    }
}

pub(crate) fn holding(monitors: &[Monitor], region: Region) -> Option<&Monitor> {
    let middle_x = f64::from(region.x) + f64::from(region.width) / 2.0;
    let middle_y = f64::from(region.y) + f64::from(region.height) / 2.0;
    monitors.iter().find(|candidate| {
        let shown = monitor(candidate);
        let within = |middle: f64, start: i32, extent: u32| {
            (f64::from(start)..f64::from(start) + f64::from(extent)).contains(&middle)
        };
        within(middle_x, shown.x, shown.width) && within(middle_y, shown.y, shown.height)
    })
}

pub(crate) fn desktop(monitors: &[Monitor]) -> Option<Region> {
    let regions: Vec<Region> = monitors.iter().map(monitor).collect();
    let left = regions.iter().map(|r| r.x).min()?;
    let top = regions.iter().map(|r| r.y).min()?;
    let right = regions.iter().map(|r| r.x + r.width as i32).max()?;
    let bottom = regions.iter().map(|r| r.y + r.height as i32).max()?;
    Some(Region {
        x: left,
        y: top,
        width: (right - left).unsigned_abs(),
        height: (bottom - top).unsigned_abs(),
    })
}

pub(crate) fn window(window: &Window) -> Region {
    Region {
        x: window.at[0],
        y: window.at[1],
        width: window.size[0].max(1).unsigned_abs(),
        height: window.size[1].max(1).unsigned_abs(),
    }
}

pub(crate) fn fit(region: Region, native: f64) -> f64 {
    let (width, height) = (f64::from(region.width), f64::from(region.height));
    let by_edge = MODEL_LONG_EDGE / width.max(height);
    let by_pixels = (MODEL_PIXELS / (width * height)).sqrt();
    let scale = native.min(by_edge).min(by_pixels);
    let (scaled_width, scaled_height) = scaled(region, scale);
    let rounded_over = f64::from(scaled_width).max(f64::from(scaled_height)) > MODEL_LONG_EDGE
        || f64::from(scaled_width) * f64::from(scaled_height) > MODEL_PIXELS;
    if rounded_over {
        scale * 0.999
    } else {
        scale
    }
}

pub(crate) fn scaled(region: Region, scale: f64) -> (u32, u32) {
    let pixels = |logical: u32| (f64::from(logical) * scale).round() as u32;
    (pixels(region.width), pixels(region.height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, x: i32, width: u32, scale: f64) -> Monitor {
        Monitor {
            id: 0,
            name: name.to_owned(),
            x,
            y: 0,
            width,
            height: 1440,
            scale,
            transform: 0,
            focused: false,
        }
    }

    #[test]
    fn a_region_belongs_to_the_monitor_holding_its_middle() {
        let monitors = [
            monitor("DP-1", 0, 2560, 1.0),
            monitor("eDP-1", 2560, 2880, 2.0),
        ];
        let mostly_on_the_laptop = Region {
            x: 2400,
            y: 100,
            width: 800,
            height: 600,
        };

        let held = holding(&monitors, mostly_on_the_laptop).map(|m| m.name.as_str());

        assert_eq!(held, Some("eDP-1"));
    }

    #[test]
    fn a_region_whose_middle_is_off_every_monitor_belongs_to_none() {
        let monitors = [monitor("DP-1", 0, 2560, 1.0)];
        let beside = Region {
            x: 2500,
            y: 0,
            width: 400,
            height: 400,
        };

        assert!(holding(&monitors, beside).is_none());
    }
}
