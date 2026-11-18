use std::collections::BTreeSet;
use std::io::Write;
use std::os::fd::AsFd;
use std::time::Duration;

use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
use wayland_client::protocol::wl_pointer::{Axis, AxisSource, ButtonState};
use wayland_client::protocol::{wl_registry, wl_seat::WlSeat};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

use crate::keys;
use crate::screen::{Acted, Button, Combo, Direction, Modifier, Point};

const PAUSE: Duration = Duration::from_millis(8);

const WHEEL_NOTCH: f64 = 15.0;

const XKB_V1: u32 = 1;

const PRESSED: u32 = 1;

const RELEASED: u32 = 0;

const MOST_KEYS: usize = 200;

const MODIFIER_KEYS: [(Modifier, &str, &str, u32); 4] = [
    (Modifier::Shift, "Shift_L", "Shift", 1),
    (Modifier::Ctrl, "Control_L", "Control", 1 << 2),
    (Modifier::Alt, "Alt_L", "Mod1", 1 << 3),
    (Modifier::Super, "Super_L", "Mod4", 1 << 6),
];

struct Events;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Events {
    fn event(
        _: &mut Events,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Events>,
    ) {
    }
}

delegate_noop!(Events: ignore WlSeat);
delegate_noop!(Events: ZwlrVirtualPointerManagerV1);
delegate_noop!(Events: ZwlrVirtualPointerV1);
delegate_noop!(Events: ZwpVirtualKeyboardManagerV1);
delegate_noop!(Events: ZwpVirtualKeyboardV1);

pub(crate) fn act<F>(action: F) -> Acted<'static>
where
    F: FnOnce(&mut Input) -> Result<(), String> + Send + 'static,
{
    Box::pin(async move {
        tokio::task::spawn_blocking(move || {
            let mut input = Input::connect()?;
            action(&mut input)?;
            input.sync()
        })
        .await
        .map_err(|e| format!("the input thread failed: {e}"))?
    })
}

pub(crate) struct Input {
    globals: GlobalList,
    queue: EventQueue<Events>,
}

impl Input {
    fn connect() -> Result<Input, String> {
        let connection = Connection::connect_to_env()
            .map_err(|e| format!("can't reach the Wayland display: {e}"))?;
        let (globals, queue) = registry_queue_init::<Events>(&connection)
            .map_err(|e| format!("the Wayland display didn't list its globals: {e}"))?;
        Ok(Input { globals, queue })
    }

    pub(crate) fn click(&mut self, at: Point, button: Button) -> Result<(), String> {
        let pointer = self.pointer()?;
        self.move_to(&pointer, at)?;
        let code = match button {
            Button::Left => 0x110,
            Button::Right => 0x111,
            Button::Middle => 0x112,
        };
        for state in [ButtonState::Pressed, ButtonState::Released] {
            pointer.button(now(), code, state);
            pointer.frame();
            self.pause()?;
        }
        pointer.destroy();
        Ok(())
    }

    pub(crate) fn scroll(
        &mut self,
        at: Point,
        direction: Direction,
        notches: u32,
    ) -> Result<(), String> {
        let pointer = self.pointer()?;
        self.move_to(&pointer, at)?;
        let (axis, sign) = match direction {
            Direction::Up => (Axis::VerticalScroll, -1),
            Direction::Down => (Axis::VerticalScroll, 1),
            Direction::Left => (Axis::HorizontalScroll, -1),
            Direction::Right => (Axis::HorizontalScroll, 1),
        };
        for _ in 0..notches {
            pointer.axis_source(AxisSource::Wheel);
            pointer.axis_discrete(now(), axis, WHEEL_NOTCH * f64::from(sign), sign);
            pointer.frame();
            self.pause()?;
        }
        pointer.destroy();
        Ok(())
    }

    pub(crate) fn key(&mut self, combo: &Combo) -> Result<(), String> {
        let keyboard = self.keyboard(std::slice::from_ref(&combo.keysym))?;
        let mut held = 0;
        for modifier in &combo.modifiers {
            let (code, mask) = modifier_key(*modifier);
            held |= mask;
            keyboard.keyboard.key(now(), code, PRESSED);
            keyboard.keyboard.modifiers(held, 0, 0, 0);
            self.pause()?;
        }
        self.press(&keyboard.keyboard, keyboard.code(&combo.keysym))?;
        for modifier in combo.modifiers.iter().rev() {
            let (code, mask) = modifier_key(*modifier);
            held &= !mask;
            keyboard.keyboard.key(now(), code, RELEASED);
            keyboard.keyboard.modifiers(held, 0, 0, 0);
            self.pause()?;
        }
        keyboard.keyboard.destroy();
        Ok(())
    }

    pub(crate) fn type_text(&mut self, text: &str) -> Result<(), String> {
        let keysyms: Vec<String> = text
            .chars()
            .map(|c| keys::char_keysym(c).ok_or_else(|| format!("{c:?} can't be typed")))
            .collect::<Result<_, _>>()?;
        for chunk in chunks(&keysyms) {
            let keyboard = self.keyboard(chunk)?;
            for keysym in chunk {
                self.press(&keyboard.keyboard, keyboard.code(keysym))?;
            }
            keyboard.keyboard.destroy();
        }
        Ok(())
    }

    fn pointer(&mut self) -> Result<ZwlrVirtualPointerV1, String> {
        let handle = self.queue.handle();
        let manager: ZwlrVirtualPointerManagerV1 = self
            .globals
            .bind(&handle, 1..=1, ())
            .map_err(|e| format!("the compositor offers no virtual pointer: {e}"))?;
        Ok(manager.create_virtual_pointer(None, &handle, ()))
    }

    fn move_to(&mut self, pointer: &ZwlrVirtualPointerV1, at: Point) -> Result<(), String> {
        let desktop = at.desktop;
        let within = |position: f64, origin: i32, extent: u32| {
            (position - f64::from(origin))
                .round()
                .clamp(0.0, f64::from(extent.saturating_sub(1))) as u32
        };
        pointer.motion_absolute(
            now(),
            within(at.x, desktop.x, desktop.width),
            within(at.y, desktop.y, desktop.height),
            desktop.width,
            desktop.height,
        );
        pointer.frame();
        self.pause()
    }

    fn keyboard(&mut self, keysyms: &[String]) -> Result<Keyboard, String> {
        let handle = self.queue.handle();
        let seat: WlSeat = self
            .globals
            .bind(&handle, 1..=1, ())
            .map_err(|e| format!("the compositor offers no seat: {e}"))?;
        let manager: ZwpVirtualKeyboardManagerV1 = self
            .globals
            .bind(&handle, 1..=1, ())
            .map_err(|e| format!("the compositor offers no virtual keyboard: {e}"))?;
        let keyboard = manager.create_virtual_keyboard(&seat, &handle, ());
        let keysyms: Vec<String> = keysyms
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let keymap = keymap(&keysyms);
        let fd = rustix::fs::memfd_create("lucy-keymap", rustix::fs::MemfdFlags::CLOEXEC)
            .map_err(|e| format!("can't hold the keymap: {e}"))?;
        let mut file = std::fs::File::from(fd);
        file.write_all(keymap.as_bytes())
            .and_then(|()| file.write_all(b"\0"))
            .map_err(|e| format!("can't hold the keymap: {e}"))?;
        let size = u32::try_from(keymap.len() + 1).expect("a keymap of at most 200 keys is small");
        keyboard.keymap(XKB_V1, file.as_fd(), size);
        self.sync()?;
        Ok(Keyboard { keyboard, keysyms })
    }

    fn press(&mut self, keyboard: &ZwpVirtualKeyboardV1, code: u32) -> Result<(), String> {
        for state in [PRESSED, RELEASED] {
            keyboard.key(now(), code, state);
            self.pause()?;
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), String> {
        self.queue
            .flush()
            .map_err(|e| format!("the Wayland display hung up: {e}"))?;
        std::thread::sleep(PAUSE);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), String> {
        self.queue
            .roundtrip(&mut Events)
            .map(|_| ())
            .map_err(|e| format!("the Wayland display hung up: {e}"))
    }
}

struct Keyboard {
    keyboard: ZwpVirtualKeyboardV1,
    keysyms: Vec<String>,
}

impl Keyboard {
    fn code(&self, keysym: &str) -> u32 {
        let index = self
            .keysyms
            .iter()
            .position(|k| k == keysym)
            .expect("the keymap holds every keysym it was made for");
        evdev(MODIFIER_KEYS.len() + index)
    }
}

fn modifier_key(modifier: Modifier) -> (u32, u32) {
    let index = MODIFIER_KEYS
        .iter()
        .position(|(m, ..)| *m == modifier)
        .expect("every modifier has a key");
    (evdev(index), MODIFIER_KEYS[index].3)
}

fn evdev(index: usize) -> u32 {
    u32::try_from(index + 1).expect("a keymap holds at most a few hundred keys")
}

fn chunks(keysyms: &[String]) -> Vec<&[String]> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut distinct = BTreeSet::new();
    for (i, keysym) in keysyms.iter().enumerate() {
        if !distinct.contains(keysym) && distinct.len() == MOST_KEYS {
            chunks.push(&keysyms[start..i]);
            start = i;
            distinct.clear();
        }
        distinct.insert(keysym);
    }
    chunks.push(&keysyms[start..]);
    chunks
}

pub(crate) fn keymap(keysyms: &[String]) -> String {
    let names: Vec<&str> = MODIFIER_KEYS
        .iter()
        .map(|(_, name, ..)| *name)
        .chain(keysyms.iter().map(String::as_str))
        .collect();
    let mut codes = String::new();
    let mut symbols = String::new();
    for (index, name) in names.iter().enumerate() {
        codes.push_str(&format!("    <K{index}> = {};\n", evdev(index) + 8));
        symbols.push_str(&format!("    key <K{index}> {{ [ {name} ] }};\n"));
    }
    for (index, (_, _, modifier, _)) in MODIFIER_KEYS.iter().enumerate() {
        symbols.push_str(&format!("    modifier_map {modifier} {{ <K{index}> }};\n"));
    }
    format!(
        "xkb_keymap {{\n\
         xkb_keycodes \"lucy\" {{\n    minimum = 8;\n    maximum = {};\n{codes}}};\n\
         xkb_types \"lucy\" {{ include \"complete\" }};\n\
         xkb_compatibility \"lucy\" {{ include \"complete\" }};\n\
         xkb_symbols \"lucy\" {{\n{symbols}}};\n\
         }};\n",
        evdev(names.len()) + 8
    )
}

fn now() -> u32 {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let millis = time.tv_sec * 1000 + time.tv_nsec / 1_000_000;
    millis as u32
}
