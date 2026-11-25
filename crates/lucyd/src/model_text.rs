//! How text Lucy writes for the model begins. None of it is for the user to
//! read: a notification or anything else shown says it plainly instead.

use crate::persona;

pub const PLUGIN_BUILD: &str = "[Lucy's Plugin build]";
pub const HAND_OFF: &str = "[Lucy's Hand-off]";
pub const INTERRUPTED: &str = "[You were interrupted";
pub const CALL_FINISHED: &str = "[A confirmed call finished after the user cut in:";
pub const NOT_APPROVABLE: &str = "[That Plugin can't be approved:";
pub const MEMORY: &str = "[Lucy's Memory]";

/// How each piece of text written for the model begins, or the whole of it.
pub const MARKERS: &[&str] = &[
    PLUGIN_BUILD,
    HAND_OFF,
    INTERRUPTED,
    CALL_FINISHED,
    NOT_APPROVABLE,
    MEMORY,
    persona::INTRODUCTION,
];
