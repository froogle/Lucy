use crate::screen::{Combo, Modifier};

const NAMED: &[(&[&str], &str)] = &[
    (&["enter", "return"], "Return"),
    (&["tab"], "Tab"),
    (&["esc", "escape"], "Escape"),
    (&["space"], "space"),
    (&["backspace"], "BackSpace"),
    (&["delete", "del"], "Delete"),
    (&["insert"], "Insert"),
    (&["home"], "Home"),
    (&["end"], "End"),
    (&["pageup", "page_up", "pgup"], "Page_Up"),
    (&["pagedown", "page_down", "pgdn"], "Page_Down"),
    (&["up"], "Up"),
    (&["down"], "Down"),
    (&["left"], "Left"),
    (&["right"], "Right"),
    (&["menu"], "Menu"),
    (&["f1"], "F1"),
    (&["f2"], "F2"),
    (&["f3"], "F3"),
    (&["f4"], "F4"),
    (&["f5"], "F5"),
    (&["f6"], "F6"),
    (&["f7"], "F7"),
    (&["f8"], "F8"),
    (&["f9"], "F9"),
    (&["f10"], "F10"),
    (&["f11"], "F11"),
    (&["f12"], "F12"),
];

const MODIFIERS: &[(&[&str], Modifier)] = &[
    (&["shift"], Modifier::Shift),
    (&["ctrl", "control"], Modifier::Ctrl),
    (&["alt"], Modifier::Alt),
    (&["super", "meta", "win", "cmd"], Modifier::Super),
];

pub(crate) const COMBO_HELP: &str = "a key, or modifiers (ctrl, shift, alt, super) and a key \
     joined by +: \"Return\", \"ctrl+l\", \"ctrl+shift+t\", \"alt+Left\", \"Page_Down\". Keys \
     are one character, or Enter, Tab, Escape, space, BackSpace, Delete, Insert, Home, End, \
     Page_Up, Page_Down, Up, Down, Left, Right, Menu, F1 to F12.";

pub(crate) fn combo(text: &str) -> Result<Combo, String> {
    let unknown = || format!("{text:?} isn't a key combination; combo is {COMBO_HELP}");
    let (held, key) = match text.strip_suffix("++") {
        Some(held) => (held, "+"),
        None if text == "+" => ("", "+"),
        None => text.rsplit_once('+').unwrap_or(("", text)),
    };
    let mut modifiers = Vec::new();
    for name in held.split('+').filter(|_| !held.is_empty()) {
        let modifier = lookup(MODIFIERS, name.trim()).ok_or_else(unknown)?;
        if !modifiers.contains(&modifier) {
            modifiers.push(modifier);
        }
    }
    let keysym = keysym(key.trim()).ok_or_else(unknown)?;
    Ok(Combo { modifiers, keysym })
}

pub(crate) fn char_keysym(c: char) -> Option<String> {
    match c {
        '\n' => Some("Return".into()),
        '\t' => Some("Tab".into()),
        c if c.is_control() => None,
        c => Some(format!("U{:04X}", u32::from(c))),
    }
}

fn keysym(key: &str) -> Option<String> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_whitespace() => char_keysym(c.to_lowercase().next()?),
        _ => lookup(NAMED, key).map(str::to_owned),
    }
}

fn lookup<T: Copy>(table: &[(&[&str], T)], name: &str) -> Option<T> {
    let name = name.to_lowercase();
    table
        .iter()
        .find(|(names, _)| names.contains(&name.as_str()))
        .map(|(_, found)| *found)
}
