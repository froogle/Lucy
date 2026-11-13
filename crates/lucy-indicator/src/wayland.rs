//! The marker on screen: a wlr-layer-shell surface on the overlay layer,
//! anchored bottom right, that never takes keyboard focus or reserves space.
//! It exists only while there is something to show.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use lucy_ipc::State;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, FrameCallbackData};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Channel};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_output, wl_pointer, wl_seat, wl_shm, wl_surface,
};
use smithay_client_toolkit::reexports::client::{Connection, EventQueue, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};

use crate::look::{Indicator, SIZE};
use crate::theme::Palette;

/// Gap between the marker and the screen's bottom-right corner.
const MARGIN: i32 = 16;
/// How often the theme is re-read, to follow theme changes.
const THEME_POLL: Duration = Duration::from_secs(2);
/// Linux `BTN_LEFT`.
const LEFT_BUTTON: u32 = 0x110;

/// What the lucyd connection reports.
pub enum Update {
    State(State),
    /// Lucy's voice level, as RMS from 0.0 to 1.0.
    Level(f32),
    /// The connection to lucyd ended.
    Lost(String),
}

/// A compositor that can show the marker.
pub struct Display {
    connection: Connection,
    queue: EventQueue<App>,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
}

impl Display {
    /// Connects to the Wayland compositor, or says why the marker can't be
    /// shown there.
    pub fn connect() -> Result<Display, String> {
        let connection = Connection::connect_to_env()
            .map_err(|e| format!("can't connect to a Wayland compositor: {e}"))?;
        let (globals, queue) = registry_queue_init(&connection)
            .map_err(|e| format!("can't list the compositor's globals: {e}"))?;
        let qh = queue.handle();
        let layer_shell = LayerShell::bind(&globals, &qh)
            .map_err(|_| "the compositor has no layer-shell (wlr-layer-shell)".to_owned())?;
        let compositor = CompositorState::bind(&globals, &qh)
            .map_err(|e| format!("the compositor has no wl_compositor: {e}"))?;
        let shm =
            Shm::bind(&globals, &qh).map_err(|e| format!("the compositor has no wl_shm: {e}"))?;
        Ok(Display {
            registry: RegistryState::new(&globals),
            seats: SeatState::new(&globals, &qh),
            outputs: OutputState::new(&globals, &qh),
            connection,
            queue,
            compositor,
            layer_shell,
            shm,
        })
    }

    /// Shows lucyd's state until the connection to lucyd ends, and returns
    /// why it stopped. `end_session` runs when the marker is clicked.
    pub fn run(
        self,
        updates: Channel<Update>,
        colors_path: Option<PathBuf>,
        end_session: fn(),
    ) -> String {
        match self.try_run(updates, colors_path, end_session) {
            Ok(never) => match never {},
            Err(why) => why,
        }
    }

    fn try_run(
        self,
        updates: Channel<Update>,
        colors_path: Option<PathBuf>,
        end_session: fn(),
    ) -> Result<std::convert::Infallible, String> {
        let mut event_loop: EventLoop<App> =
            EventLoop::try_new().map_err(|e| format!("can't start the event loop: {e}"))?;
        let handle = event_loop.handle();
        let palette = colors_path
            .as_deref()
            .map(Palette::load)
            .unwrap_or_default();
        let pool = SlotPool::new((SIZE * SIZE * 4 * 2) as usize, &self.shm)
            .map_err(|e| format!("can't allocate shared memory: {e}"))?;
        let qh = self.queue.handle();
        let mut app = App {
            registry: self.registry,
            seats: self.seats,
            outputs: self.outputs,
            shm: self.shm,
            compositor: self.compositor,
            layer_shell: self.layer_shell,
            pool,
            qh,
            surface: None,
            pointer: None,
            indicator: Indicator::new(palette),
            palette,
            started: Instant::now(),
            end_session,
            lost: None,
        };

        WaylandSource::new(self.connection, self.queue)
            .insert(handle.clone())
            .map_err(|e| format!("can't watch the Wayland connection: {e}"))?;
        handle
            .insert_source(updates, |event, _, app: &mut App| match event {
                channel::Event::Msg(Update::State(state)) => app.on_state(state),
                channel::Event::Msg(Update::Level(rms)) => app.indicator.on_level(rms),
                channel::Event::Msg(Update::Lost(why)) => app.lost = Some(why),
                channel::Event::Closed => {}
            })
            .map_err(|e| format!("can't watch lucyd: {e}"))?;
        if let Some(path) = colors_path {
            handle
                .insert_source(Timer::from_duration(THEME_POLL), move |_, _, app| {
                    app.set_palette(Palette::load(&path));
                    TimeoutAction::ToDuration(THEME_POLL)
                })
                .map_err(|e| format!("can't watch the theme: {e}"))?;
        }

        loop {
            event_loop
                .dispatch(None, &mut app)
                .map_err(|e| format!("Wayland error: {e}"))?;
            if let Some(why) = app.lost.take() {
                return Err(why);
            }
        }
    }
}

/// The marker's surface while it is shown.
struct Surface {
    layer: LayerSurface,
    /// False until the compositor first configures it.
    configured: bool,
}

struct App {
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    pool: SlotPool,
    qh: QueueHandle<App>,
    surface: Option<Surface>,
    pointer: Option<wl_pointer::WlPointer>,
    indicator: Indicator,
    palette: Palette,
    started: Instant,
    end_session: fn(),
    lost: Option<String>,
}

impl App {
    fn now(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    fn on_state(&mut self, state: State) {
        let now = self.now();
        self.indicator.on_state(state, now);
        if self.surface.is_none() && self.indicator.draw(now).is_some() {
            self.show();
        }
    }

    fn set_palette(&mut self, palette: Palette) {
        if palette != self.palette {
            self.palette = palette;
            self.indicator.set_palette(palette);
        }
    }

    /// Creates the surface; it is drawn once the compositor configures it.
    fn show(&mut self) {
        let wl_surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            wl_surface,
            Layer::Overlay,
            Some("lucy-indicator"),
            None,
        );
        layer.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
        layer.set_margin(0, MARGIN, MARGIN, 0);
        layer.set_size(SIZE, SIZE);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        self.surface = Some(Surface {
            layer,
            configured: false,
        });
    }

    /// Draws the next frame and asks for the one after, or removes the
    /// surface once there is nothing to show.
    fn draw(&mut self) {
        let Some(surface) = &self.surface else {
            return;
        };
        let Some(frame) = self.indicator.draw(self.now()) else {
            self.surface = None;
            return;
        };
        let stride = SIZE as i32 * 4;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(SIZE as i32, SIZE as i32, stride, wl_shm::Format::Argb8888)
        else {
            eprintln!("lucy-indicator: can't allocate a frame buffer");
            return;
        };
        // tiny-skia's premultiplied RGBA to wl_shm's little-endian ARGB.
        for (out, pixel) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(frame.pixels()) {
            *out = [pixel.blue(), pixel.green(), pixel.red(), pixel.alpha()];
        }
        let wl_surface = surface.layer.wl_surface();
        wl_surface.damage_buffer(0, 0, SIZE as i32, SIZE as i32);
        wl_surface.frame(&self.qh, FrameCallbackData(wl_surface.clone()));
        if buffer.attach_to(wl_surface).is_err() {
            eprintln!("lucy-indicator: can't attach the frame buffer");
            return;
        }
        surface.layer.commit();
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.draw();
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for App {
    /// The compositor took the surface away, e.g. its monitor was unplugged.
    /// It comes back on the next state change.
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.surface = None;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(surface) = &mut self.surface {
            if !surface.configured {
                surface.configured = true;
                self.draw();
            }
        }
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let Some(surface) = &self.surface else {
            return;
        };
        let clicked = events.iter().any(|event| {
            &event.surface == surface.layer.wl_surface()
                && matches!(event.kind, PointerEventKind::Press { button, .. } if button == LEFT_BUTTON)
        });
        if clicked {
            let end_session = self.end_session;
            std::thread::spawn(end_session);
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seats.get_pointer(qh, &seat).ok();
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
delegate_dispatch2!(App);
