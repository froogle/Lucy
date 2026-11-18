use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec};
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_display::WlDisplay;
use wayland_client::protocol::wl_output::{self, Transform, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::{wl_buffer::WlBuffer, wl_shm_pool::WlShmPool};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::ExtImageCaptureSourceV1;
use wayland_protocols::ext::image_capture_source::v1::client::ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1;
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_frame_v1::{
    self, ExtImageCopyCaptureFrameV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_manager_v1::{
    ExtImageCopyCaptureManagerV1, Options,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_session_v1::{
    self, ExtImageCopyCaptureSessionV1,
};

use crate::capture::{self, Buffer, Channels, Look};
use crate::screen::{Captured, Image};

const PATIENCE: Duration = Duration::from_secs(5);

const OUTPUT_VERSION_WITH_NAME: u32 = 4;

const BYTES_PER_PIXEL: u32 = 4;

const NO_CAPTURE: &str = "this desktop doesn't offer screen capture";

const UNFINISHED: &str = "the screen capture didn't finish";

struct Global {
    name: u32,
    interface: String,
    version: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum Session {
    #[default]
    Starting,
    Described,
    Stopped,
}

#[derive(Default)]
struct State {
    globals: Vec<Global>,
    syncs: u32,
    names: Vec<Option<String>>,
    size: Option<(u32, u32)>,
    formats: Vec<wl_shm::Format>,
    session: Session,
    transform: Option<Transform>,
    frame: Option<Result<(), String>>,
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut State,
        _: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => state.globals.push(Global {
                name,
                interface,
                version,
            }),
            wl_registry::Event::GlobalRemove { name } => {
                state.globals.retain(|global| global.name != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, ()> for State {
    fn event(
        state: &mut State,
        _: &WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.syncs += 1;
        }
    }
}

impl Dispatch<WlOutput, usize> for State {
    fn event(
        state: &mut State,
        _: &WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let wl_output::Event::Name { name } = event {
            state.names[*index] = Some(name);
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for State {
    fn event(
        state: &mut State,
        _: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        match event {
            Event::BufferSize { width, height } => state.size = Some((width, height)),
            Event::ShmFormat {
                format: WEnum::Value(format),
            } => state.formats.push(format),
            Event::Done => state.session = Session::Described,
            Event::Stopped => state.session = Session::Stopped,
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, ()> for State {
    fn event(
        state: &mut State,
        _: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        match event {
            Event::Transform {
                transform: WEnum::Value(transform),
            } => state.transform = Some(transform),
            Event::Ready => state.frame = Some(Ok(())),
            Event::Failed { reason } => {
                state.frame = Some(Err(format!("the screen capture failed: {reason:?}")));
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ignore WlShm);
delegate_noop!(State: WlShmPool);
delegate_noop!(State: ignore WlBuffer);
delegate_noop!(State: ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(State: ExtImageCaptureSourceV1);
delegate_noop!(State: ExtImageCopyCaptureManagerV1);

pub(crate) fn capture(output: String, look: Look) -> Captured<'static> {
    Box::pin(async move {
        tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + PATIENCE;
            Compositor::connect(deadline)?.take(&output, &look)
        })
        .await
        .map_err(|e| format!("the capture thread failed: {e}"))?
    })
}

struct Offer {
    session: ExtImageCopyCaptureSessionV1,
    source: ExtImageCaptureSourceV1,
    width: u32,
    height: u32,
    channels: Channels,
    format: wl_shm::Format,
}

impl Offer {
    fn stride(&self) -> u32 {
        self.width * BYTES_PER_PIXEL
    }

    fn length(&self) -> u64 {
        u64::from(self.stride()) * u64::from(self.height)
    }
}

struct Compositor {
    display: WlDisplay,
    registry: WlRegistry,
    queue: EventQueue<State>,
    state: State,
    deadline: Instant,
}

impl Compositor {
    fn connect(deadline: Instant) -> Result<Compositor, String> {
        let connection = Connection::connect_to_env()
            .map_err(|e| format!("can't reach the Wayland display: {e}"))?;
        let queue = connection.new_event_queue();
        let display = connection.display();
        let registry = display.get_registry(&queue.handle(), ());
        let mut compositor = Compositor {
            display,
            registry,
            queue,
            state: State::default(),
            deadline,
        };
        compositor.sync()?;
        Ok(compositor)
    }

    fn take(&mut self, output: &str, look: &Look) -> Result<Image, String> {
        let output = self.output(output)?;
        let offer = self.negotiate(&output)?;
        let bytes = self.copy(&offer)?;
        let transform = self.state.transform.unwrap_or(Transform::Normal);
        let buffer = Buffer {
            bytes: &bytes,
            width: offer.width,
            height: offer.height,
            stride: offer.stride(),
            channels: offer.channels,
        };
        capture::picture(&buffer, transform, look)
    }

    fn negotiate(&mut self, output: &WlOutput) -> Result<Offer, String> {
        let sources: ExtOutputImageCaptureSourceManagerV1 = self.bind().ok_or(NO_CAPTURE)?;
        let copier: ExtImageCopyCaptureManagerV1 = self.bind().ok_or(NO_CAPTURE)?;
        let handle = self.queue.handle();
        let source = sources.create_source(output, &handle, ());
        let session = copier.create_session(&source, Options::empty(), &handle, ());
        self.wait(|state| state.session != Session::Starting)?;
        if self.state.session == Session::Stopped {
            return Err("the screen capture stopped before it began".into());
        }
        let (width, height) = self
            .state
            .size
            .filter(|&(width, height)| width > 0 && height > 0)
            .ok_or("the compositor gave no size for the screen capture")?;
        let (channels, format) = self
            .state
            .formats
            .iter()
            .find_map(|&offered| Some((channels(offered)?, offered)))
            .ok_or("the compositor offers no 8-bit pixel format for screen capture")?;
        Ok(Offer {
            session,
            source,
            width,
            height,
            channels,
            format,
        })
    }

    fn copy(&mut self, offer: &Offer) -> Result<Vec<u8>, String> {
        let shm: WlShm = self
            .bind()
            .ok_or("the compositor offers no shared memory")?;
        let length = offer.length();
        let memory = memfd(length)?;
        let pool_size =
            i32::try_from(length).map_err(|e| format!("the screen is too big to capture: {e}"))?;
        let to_i32 = |value: u32| i32::try_from(value).expect("a screen's size fits an i32");
        let handle = self.queue.handle();
        let pool = shm.create_pool(memory.as_fd(), pool_size, &handle, ());
        let buffer = pool.create_buffer(
            0,
            to_i32(offer.width),
            to_i32(offer.height),
            to_i32(offer.stride()),
            offer.format,
            &handle,
            (),
        );
        let frame = offer.session.create_frame(&handle, ());
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, to_i32(offer.width), to_i32(offer.height));
        frame.capture();
        self.wait(|state| state.frame.is_some())?;
        self.state
            .frame
            .take()
            .expect("the wait ends once the frame is ready or failed")?;

        let mut bytes = vec![0; usize::try_from(length).expect("the buffer fits in memory")];
        memory
            .read_exact_at(&mut bytes, 0)
            .map_err(|e| format!("can't read the screen capture: {e}"))?;
        frame.destroy();
        offer.session.destroy();
        offer.source.destroy();
        buffer.destroy();
        pool.destroy();
        Ok(bytes)
    }

    fn bind<I>(&self) -> Option<I>
    where
        I: Proxy + 'static,
        State: Dispatch<I, ()>,
    {
        let interface = I::interface().name;
        let global = self
            .state
            .globals
            .iter()
            .find(|global| global.interface == interface)?;
        Some(self.registry.bind(global.name, 1, &self.queue.handle(), ()))
    }

    fn output(&mut self, name: &str) -> Result<WlOutput, String> {
        let handle = self.queue.handle();
        let interface = WlOutput::interface().name;
        let outputs: Vec<WlOutput> = self
            .state
            .globals
            .iter()
            .filter(|global| global.interface == interface)
            .enumerate()
            .map(|(index, global)| {
                self.registry.bind::<WlOutput, _, _>(
                    global.name,
                    global.version.min(OUTPUT_VERSION_WITH_NAME),
                    &handle,
                    index,
                )
            })
            .collect();
        self.state.names = vec![None; outputs.len()];
        self.sync()?;
        let found = self
            .state
            .names
            .iter()
            .position(|named| named.as_deref() == Some(name));
        found
            .map(|index| outputs[index].clone())
            .ok_or_else(|| format!("the Wayland display has no monitor named {name}"))
    }

    fn sync(&mut self) -> Result<(), String> {
        let done = self.state.syncs + 1;
        self.display.sync(&self.queue.handle(), ());
        self.wait(|state| state.syncs >= done)
    }

    fn wait(&mut self, until: impl Fn(&State) -> bool) -> Result<(), String> {
        loop {
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(hung_up)?;
            if until(&self.state) {
                return Ok(());
            }
            self.queue.flush().map_err(hung_up)?;
            let Some(guard) = self.queue.prepare_read() else {
                continue;
            };
            let left = self.deadline.saturating_duration_since(Instant::now());
            let timeout = Timespec::try_from(left).expect("five seconds fits a timespec");
            let polled = {
                let fd = guard.connection_fd();
                let mut polled = [PollFd::new(&fd, PollFlags::IN)];
                rustix::event::poll(&mut polled, Some(&timeout))
            };
            match polled {
                Ok(0) => return Err(UNFINISHED.into()),
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => return Err(hung_up(e)),
            }
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(hung_up(e)),
            }
        }
    }
}

fn hung_up(e: impl std::fmt::Display) -> String {
    format!("the Wayland display hung up: {e}")
}

fn unheld(e: impl std::fmt::Display) -> String {
    format!("can't hold the screen capture: {e}")
}

fn channels(format: wl_shm::Format) -> Option<Channels> {
    match format {
        wl_shm::Format::Argb8888 => Some(Channels::Argb8888),
        wl_shm::Format::Xrgb8888 => Some(Channels::Xrgb8888),
        wl_shm::Format::Abgr8888 => Some(Channels::Abgr8888),
        wl_shm::Format::Xbgr8888 => Some(Channels::Xbgr8888),
        _ => None,
    }
}

fn memfd(length: u64) -> Result<std::fs::File, String> {
    let fd = rustix::fs::memfd_create("lucy-capture", rustix::fs::MemfdFlags::CLOEXEC)
        .map_err(unheld)?;
    let file = std::fs::File::from(fd);
    file.set_len(length).map_err(unheld)?;
    Ok(file)
}
