use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;

use tokio::process::Command;

use crate::hyprland::{Monitor, Window};

const GRIM: &str = "grim";

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

/// What a [`Screen`] call returns once it has finished.
pub type Captured<'a> = Pin<Box<dyn Future<Output = Result<Image, String>> + Send + 'a>>;

/// The Plugin's way to the screen itself, which tests replace with a fake.
pub trait Screen: Send + Sync {
    /// Captures `region` at `scale` image pixels per logical pixel.
    ///
    /// # Errors
    ///
    /// Why the screen couldn't be captured, ready for the model.
    fn capture(&self, region: Region, scale: f64) -> Captured<'_>;
}

/// The real [`Screen`]: captures through `grim`, found on `path`.
pub struct Grim {
    path: OsString,
}

impl Grim {
    pub fn new(path: OsString) -> Grim {
        Grim { path }
    }
}

impl Screen for Grim {
    fn capture(&self, region: Region, scale: f64) -> Captured<'_> {
        Box::pin(async move {
            let geometry = format!(
                "{},{} {}x{}",
                region.x, region.y, region.width, region.height
            );
            let output = Command::new(GRIM)
                .args([
                    "-g",
                    &geometry,
                    "-s",
                    &format!("{scale:.4}"),
                    "-t",
                    "png",
                    "-",
                ])
                .env("PATH", &self.path)
                .stdin(Stdio::null())
                .output()
                .await
                .map_err(|e| format!("{GRIM} can't run: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "{GRIM} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            Ok(Image {
                bytes: output.stdout,
                mime_type: "image/png".into(),
            })
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
