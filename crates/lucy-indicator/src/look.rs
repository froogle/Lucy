//! What the Indicator looks like in each state, drawn with tiny-skia. Pure:
//! the caller passes the time, so it is testable without a compositor.

use lucy_ipc::State;
use tiny_skia::{Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::theme::Palette;

/// The marker is a square of this many pixels at scale 1.
pub const SIZE: u32 = 48;

pub struct Indicator {
    palette: Palette,
    state: State,
    /// Lucy's voice level, as `level` RMS.
    level: f32,
    /// When the red flash of an error ends.
    flash_until: Option<f32>,
    /// An Authoring build is running: a dot in the corner.
    building: bool,
}

impl Indicator {
    pub fn new(palette: Palette) -> Self {
        Indicator {
            palette,
            state: State::Idle,
            level: 0.0,
            flash_until: None,
            building: false,
        }
    }

    /// The theme changed.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// lucyd published the voice level `rms`.
    pub fn on_level(&mut self, rms: f32) {
        self.level = rms;
    }

    /// lucyd published whether an Authoring build is running.
    pub fn on_building(&mut self, building: bool) {
        self.building = building;
    }

    /// lucyd published `state` at time `t`, in seconds.
    /// An error flashes over whatever state follows it.
    pub fn on_state(&mut self, state: State, t: f32) {
        match state {
            State::Error => self.flash_until = Some(t + FLASH),
            _ => self.state = state,
        }
    }

    /// The marker at time `t`, or `None` while it is hidden.
    pub fn draw(&self, t: f32) -> Option<Pixmap> {
        let mut canvas = Pixmap::new(SIZE, SIZE).expect("non-zero size");
        if let Some(until) = self.flash_until.filter(|&until| t < until) {
            flash(&mut canvas, self.palette.error, (until - t) / FLASH);
            return Some(canvas);
        }
        match self.state {
            // An error is only ever drawn as the flash.
            State::Idle | State::Error => return None,
            State::Listening => listening(&mut canvas, self.palette.accent, t),
            State::Thinking => thinking(&mut canvas, self.palette.accent, t),
            State::Speaking => speaking(&mut canvas, self.palette.accent, self.level),
            State::Confirming => confirming(&mut canvas, self.palette.warning, t),
        }
        if self.building {
            building(&mut canvas, self.palette.accent);
        }
        Some(canvas)
    }
}

const CENTRE: f32 = SIZE as f32 / 2.0;

/// Seconds an error flashes for.
const FLASH: f32 = 0.6;

/// A red disc that fades as `left` runs from 1 to 0.
fn flash(canvas: &mut Pixmap, colour: Color, left: f32) {
    disc(canvas, 16.0, with_alpha(colour, 0.4 + 0.6 * left));
}

/// The "building" mark: a dot in the top-right corner.
fn building(canvas: &mut Pixmap, colour: Color) {
    let dot = PathBuilder::from_circle(SIZE as f32 - 5.0, 5.0, 4.0).expect("positive radius");
    canvas.fill_path(
        &dot,
        &paint(colour),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// A disc and ring that pulse once a second, quicker than Listening.
fn confirming(canvas: &mut Pixmap, colour: Color, t: f32) {
    let pulse = wave(t, 1.0);
    disc(canvas, 8.0 + 4.0 * pulse, colour);
    ring(canvas, 18.0, 3.0, with_alpha(colour, 0.5 + 0.5 * pulse));
}

/// Five bars that rise with Lucy's voice level.
fn speaking(canvas: &mut Pixmap, colour: Color, rms: f32) {
    const SHAPE: [f32; 5] = [0.5, 0.8, 1.0, 0.8, 0.5];
    const WIDTH: f32 = 4.0;
    const GAP: f32 = 3.0;
    // Speech RMS rarely passes 0.3; that fills the meter.
    let loudness = (rms / 0.3).clamp(0.0, 1.0);
    let left = CENTRE - (SHAPE.len() as f32 * (WIDTH + GAP) - GAP) / 2.0;
    for (i, shape) in SHAPE.iter().enumerate() {
        let height = 4.0 + 28.0 * loudness * shape;
        let x = left + i as f32 * (WIDTH + GAP);
        let bar = Rect::from_xywh(x, CENTRE - height / 2.0, WIDTH, height).expect("positive size");
        canvas.fill_rect(bar, &paint(colour), Transform::identity(), None);
    }
}

/// An arc that orbits a faint track, one turn a second.
fn thinking(canvas: &mut Pixmap, colour: Color, t: f32) {
    ring(canvas, 15.0, 4.0, with_alpha(colour, 0.2));
    let start = t.fract() * std::f32::consts::TAU;
    arc(canvas, 15.0, start, start + 1.8, 4.0, colour);
}

/// A ring that breathes slowly around a steady dot.
fn listening(canvas: &mut Pixmap, colour: Color, t: f32) {
    let breath = wave(t, LISTENING_PERIOD);
    disc(canvas, 6.0, colour);
    ring(
        canvas,
        13.0 + 4.0 * breath,
        3.0,
        with_alpha(colour, 0.5 + 0.5 * breath),
    );
}

/// Seconds per breath while Listening.
const LISTENING_PERIOD: f32 = 2.4;

/// 0 → 1 → 0 once every `period` seconds.
fn wave(t: f32, period: f32) -> f32 {
    0.5 - 0.5 * (t / period * std::f32::consts::TAU).cos()
}

fn with_alpha(mut colour: Color, alpha: f32) -> Color {
    colour.set_alpha(colour.alpha() * alpha.clamp(0.0, 1.0));
    colour
}

fn paint(colour: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(colour);
    paint.anti_alias = true;
    paint
}

fn disc(canvas: &mut Pixmap, radius: f32, colour: Color) {
    let circle = PathBuilder::from_circle(CENTRE, CENTRE, radius).expect("positive radius");
    canvas.fill_path(
        &circle,
        &paint(colour),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn ring(canvas: &mut Pixmap, radius: f32, width: f32, colour: Color) {
    let circle = PathBuilder::from_circle(CENTRE, CENTRE, radius).expect("positive radius");
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    canvas.stroke_path(
        &circle,
        &paint(colour),
        &stroke,
        Transform::identity(),
        None,
    );
}

/// An arc from angle `from` to `to`, in radians clockwise from the right.
fn arc(canvas: &mut Pixmap, radius: f32, from: f32, to: f32, width: f32, colour: Color) {
    const STEPS: u32 = 24;
    let point = |angle: f32| (CENTRE + radius * angle.cos(), CENTRE + radius * angle.sin());
    let mut path = PathBuilder::new();
    let (x, y) = point(from);
    path.move_to(x, y);
    for step in 1..=STEPS {
        let (x, y) = point(from + (to - from) * step as f32 / STEPS as f32);
        path.line_to(x, y);
    }
    let path = path.finish().expect("an arc has points");
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };
    canvas.stroke_path(&path, &paint(colour), &stroke, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette {
            accent: Color::from_rgba8(0, 255, 0, 255),
            warning: Color::from_rgba8(255, 255, 0, 255),
            error: Color::from_rgba8(255, 0, 0, 255),
        }
    }

    /// The marker's visible pixels: how many, and the palette colour most of
    /// them are closest to.
    struct Ink {
        pixels: usize,
        colour: &'static str,
    }

    fn ink(pixmap: &Pixmap) -> Ink {
        let palette = palette();
        let named = [
            ("accent", palette.accent),
            ("warning", palette.warning),
            ("error", palette.error),
        ];
        let mut votes = [0usize; 3];
        let mut pixels = 0;
        for pixel in pixmap.pixels() {
            if pixel.alpha() < 64 {
                continue;
            }
            pixels += 1;
            let c = pixel.demultiply();
            let distance = |colour: &Color| {
                let d = |a: u8, b: f32| (a as f32 / 255.0 - b).powi(2);
                d(c.red(), colour.red()) + d(c.green(), colour.green()) + d(c.blue(), colour.blue())
            };
            let closest = (0..named.len())
                .min_by(|&a, &b| distance(&named[a].1).total_cmp(&distance(&named[b].1)))
                .unwrap();
            votes[closest] += 1;
        }
        let winner = (0..named.len()).max_by_key(|&i| votes[i]).unwrap();
        Ink {
            pixels,
            colour: named[winner].0,
        }
    }

    fn at(state: State, t: f32) -> Option<Pixmap> {
        let mut indicator = Indicator::new(palette());
        indicator.on_state(state, 0.0);
        indicator.draw(t)
    }

    #[test]
    fn a_build_adds_a_mark_in_the_corner_but_idle_stays_hidden() {
        let corner = |frame: &Pixmap| {
            (SIZE - 8..SIZE)
                .flat_map(|x| (0..8).map(move |y| (x, y)))
                .filter(|&(x, y)| frame.pixel(x, y).is_some_and(|p| p.alpha() >= 64))
                .count()
        };
        let mut indicator = Indicator::new(palette());
        indicator.on_state(State::Listening, 0.0);
        assert_eq!(corner(&indicator.draw(0.0).expect("shown")), 0);

        indicator.on_building(true);
        assert!(corner(&indicator.draw(0.0).expect("shown")) > 10);

        indicator.on_state(State::Idle, 0.0);
        assert!(indicator.draw(0.0).is_none());
    }

    #[test]
    fn idle_is_hidden() {
        assert!(at(State::Idle, 0.0).is_none());
    }

    #[test]
    fn listening_is_an_accent_mark() {
        let frame = at(State::Listening, 0.0).expect("shown");

        assert_eq!(ink(&frame).colour, "accent");
        assert!(ink(&frame).pixels > 0);
    }

    fn pixels(state: State, t: f32) -> usize {
        ink(&at(state, t).expect("shown")).pixels
    }

    #[test]
    fn listening_pulses_slowly() {
        // One breath takes 2.4 s.
        assert!(pixels(State::Listening, 1.2) > pixels(State::Listening, 0.0) + 50);
        assert_eq!(pixels(State::Listening, 2.4), pixels(State::Listening, 0.0));
    }

    /// Which way the ink leans from the centre: the mean (x, y) offset.
    fn lean(pixmap: &Pixmap) -> (f32, f32) {
        let (mut x, mut y, mut n) = (0.0, 0.0, 0.0);
        for (i, pixel) in pixmap.pixels().iter().enumerate() {
            if pixel.alpha() >= 64 {
                x += (i as u32 % SIZE) as f32 - CENTRE;
                y += (i as u32 / SIZE) as f32 - CENTRE;
                n += 1.0;
            }
        }
        (x / n, y / n)
    }

    #[test]
    fn thinking_is_an_accent_spinner() {
        let start = at(State::Thinking, 0.0).expect("shown");
        let half_turn = at(State::Thinking, 0.5).expect("shown");

        assert_eq!(ink(&start).colour, "accent");
        let (a, b) = (lean(&start), lean(&half_turn));
        assert!(a.0 * b.0 + a.1 * b.1 < 0.0, "{a:?} then {b:?}");
    }

    fn speaking_at(level: f32) -> Pixmap {
        let mut indicator = Indicator::new(palette());
        indicator.on_state(State::Speaking, 0.0);
        indicator.on_level(level);
        indicator.draw(0.0).expect("shown")
    }

    #[test]
    fn speaking_is_a_meter_that_follows_the_level() {
        let quiet = speaking_at(0.02);
        let loud = speaking_at(0.3);

        assert_eq!(ink(&loud).colour, "accent");
        assert!(ink(&loud).pixels > ink(&quiet).pixels * 2);
        assert!(ink(&quiet).pixels > 0, "a silent meter still shows");
    }

    #[test]
    fn confirming_is_a_warning_pulse() {
        let frame = at(State::Confirming, 0.0).expect("shown");

        assert_eq!(ink(&frame).colour, "warning");
        // A pulse a second: quicker than Listening's breath.
        assert!(pixels(State::Confirming, 0.5) > pixels(State::Confirming, 0.0) + 50);
        assert_eq!(
            pixels(State::Confirming, 1.0),
            pixels(State::Confirming, 0.0)
        );
    }

    #[test]
    fn an_error_flashes_red_then_shows_the_next_state() {
        let mut indicator = Indicator::new(palette());
        indicator.on_state(State::Thinking, 0.0);
        indicator.on_state(State::Error, 1.0);
        indicator.on_state(State::Listening, 1.01);

        assert_eq!(ink(&indicator.draw(1.1).expect("shown")).colour, "error");
        assert_eq!(ink(&indicator.draw(2.0).expect("shown")).colour, "accent");
    }

    #[test]
    fn an_error_that_ends_the_session_flashes_then_hides() {
        let mut indicator = Indicator::new(palette());
        indicator.on_state(State::Listening, 0.0);
        indicator.on_state(State::Error, 1.0);
        indicator.on_state(State::Idle, 1.01);

        assert_eq!(ink(&indicator.draw(1.1).expect("shown")).colour, "error");
        assert!(indicator.draw(2.0).is_none());
    }
}
