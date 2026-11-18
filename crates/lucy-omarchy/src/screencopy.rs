use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec};
use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
use wayland_client::protocol::wl_output::{self, Transform, WlOutput};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::{wl_buffer::WlBuffer, wl_registry, wl_shm_pool::WlShmPool};
use wayland_client::backend::WaylandError;
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, QueueHandle, WEnum};
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
use crate::screen::Captured;

const PATIENCE: Duration = Duration::from_secs(5);

const NAMED_OUTPUT: u32 = 4;

const BYTES_PER_PIXEL: u32 = 4;

const NO_CAPTURE: &str = "this desktop doesn't offer screen capture";

const UNFINISHED: &str = "the screen capture didn't finish";

#[derive(Default)]
struct State {
    names: Vec<Option<String>>,
    size: Option<(u32, u32)>,
    formats: Vec<wl_shm::Format>,
    described: bool,
    stopped: bool,
    transform: Option<Transform>,
    frame: Option<Result<(), String>>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut State,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
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
            Event::Done => state.described = true,
            Event::Stopped => state.stopped = true,
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
            Screencopy::connect()?.take(&output, &look, deadline)
        })
        .await
        .map_err(|e| format!("the capture thread failed: {e}"))?
    })
}

struct Screencopy {
    globals: GlobalList,
    queue: EventQueue<State>,
    state: State,
}

impl Screencopy {
    fn connect() -> Result<Screencopy, String> {
        let connection = Connection::connect_to_env()
            .map_err(|e| format!("can't reach the Wayland display: {e}"))?;
        let (globals, queue) = registry_queue_init::<State>(&connection)
            .map_err(|e| format!("the Wayland display didn't list its globals: {e}"))?;
        Ok(Screencopy {
            globals,
            queue,
            state: State::default(),
        })
    }

    fn take(
        &mut self,
        output: &str,
        look: &Look,
        deadline: Instant,
    ) -> Result<crate::screen::Image, String> {
        let handle = self.queue.handle();
        let sources: ExtOutputImageCaptureSourceManagerV1 = self
            .globals
            .bind(&handle, 1..=1, ())
            .map_err(|_| NO_CAPTURE)?;
        let copier: ExtImageCopyCaptureManagerV1 = self
            .globals
            .bind(&handle, 1..=1, ())
            .map_err(|_| NO_CAPTURE)?;
        let shm: WlShm = self
            .globals
            .bind(&handle, 1..=1, ())
            .map_err(|e| format!("the compositor offers no shared memory: {e}"))?;
        let output = self.output(output)?;

        let source = sources.create_source(&output, &handle, ());
        let session = copier.create_session(&source, Options::empty(), &handle, ());
        self.wait(|state| state.described || state.stopped, deadline)?;
        if self.state.stopped {
            return Err("the screen capture stopped before it began".into());
        }
        let (width, height) = self
            .state
            .size
            .filter(|&(width, height)| width > 0 && height > 0)
            .ok_or("the compositor gave no size for the screen capture")?;
        let (channels, shm_format) = self
            .state
            .formats
            .iter()
            .find_map(|&offered| Some((accepted(offered)?, offered)))
            .ok_or("the compositor offers no 8-bit pixel format for screen capture")?;

        let stride = width * BYTES_PER_PIXEL;
        let length = u64::from(stride) * u64::from(height);
        let memory = memory(length)?;
        let pool_size = i32::try_from(length).map_err(|_| "the screen is too big to capture")?;
        let as_int = |value: u32| i32::try_from(value).expect("a screen's size fits an i32");
        let pool = shm.create_pool(memory.as_fd(), pool_size, &handle, ());
        let buffer = pool.create_buffer(
            0,
            as_int(width),
            as_int(height),
            as_int(stride),
            shm_format,
            &handle,
            (),
        );
        let frame = session.create_frame(&handle, ());
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, as_int(width), as_int(height));
        frame.capture();
        self.wait(|state| state.frame.is_some(), deadline)?;
        self.state
            .frame
            .take()
            .expect("the wait ends once the frame is ready or failed")?;

        let mut bytes = vec![0; usize::try_from(length).expect("the buffer fits in memory")];
        memory
            .read_exact_at(&mut bytes, 0)
            .map_err(|e| format!("can't read the screen capture: {e}"))?;
        frame.destroy();
        session.destroy();
        source.destroy();
        buffer.destroy();
        pool.destroy();
        let transform = self.state.transform.unwrap_or(Transform::Normal);
        let buffer = Buffer {
            bytes: &bytes,
            width,
            height,
            stride,
            channels,
        };
        capture::picture(&buffer, transform, look)
    }

    fn output(&mut self, name: &str) -> Result<WlOutput, String> {
        let handle = self.queue.handle();
        let listed: Vec<(u32, u32)> = self.globals.contents().with_list(|globals| {
            globals
                .iter()
                .filter(|global| global.interface == "wl_output")
                .map(|global| (global.name, global.version))
                .collect()
        });
        let outputs: Vec<WlOutput> = listed
            .into_iter()
            .enumerate()
            .map(|(index, (global, version))| {
                self.globals.registry().bind::<WlOutput, _, _>(
                    global,
                    version.min(NAMED_OUTPUT),
                    &handle,
                    index,
                )
            })
            .collect();
        self.state.names = vec![None; outputs.len()];
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| format!("the Wayland display hung up: {e}"))?;
        let found = self
            .state
            .names
            .iter()
            .position(|named| named.as_deref() == Some(name));
        found
            .map(|index| outputs[index].clone())
            .ok_or_else(|| format!("the Wayland display has no monitor named {name}"))
    }

    fn wait(&mut self, until: impl Fn(&State) -> bool, deadline: Instant) -> Result<(), String> {
        let hung_up = |e: &dyn std::fmt::Display| format!("the Wayland display hung up: {e}");
        loop {
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(|e| hung_up(&e))?;
            if until(&self.state) {
                return Ok(());
            }
            self.queue.flush().map_err(|e| hung_up(&e))?;
            let Some(guard) = self.queue.prepare_read() else {
                continue;
            };
            let left = deadline.saturating_duration_since(Instant::now());
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
                Err(e) => return Err(hung_up(&e)),
            }
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(hung_up(&e)),
            }
        }
    }
}

fn accepted(format: wl_shm::Format) -> Option<Channels> {
    match format {
        wl_shm::Format::Argb8888 => Some(Channels::Argb8888),
        wl_shm::Format::Xrgb8888 => Some(Channels::Xrgb8888),
        wl_shm::Format::Abgr8888 => Some(Channels::Abgr8888),
        wl_shm::Format::Xbgr8888 => Some(Channels::Xbgr8888),
        _ => None,
    }
}

fn memory(length: u64) -> Result<std::fs::File, String> {
    let fd = rustix::fs::memfd_create("lucy-capture", rustix::fs::MemfdFlags::CLOEXEC)
        .map_err(|e| format!("can't hold the screen capture: {e}"))?;
    let file = std::fs::File::from(fd);
    file.set_len(length)
        .map_err(|e| format!("can't hold the screen capture: {e}"))?;
    Ok(file)
}
