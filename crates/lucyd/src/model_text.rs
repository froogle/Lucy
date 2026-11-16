//! How text Lucy writes for the model begins. None of it is for the user to
//! read: a notification or anything else shown says it plainly instead.

use crate::persona;

pub const PLUGIN_BUILD: &str = "[Lucy's Plugin build]";
pub const INTERRUPTED: &str = "[You were interrupted";
pub const CALL_FINISHED: &str = "[A confirmed call finished after the user cut in:";

/// How each piece of text written for the model begins, or the whole of it.
pub const MARKERS: &[&str] = &[
    PLUGIN_BUILD,
    INTERRUPTED,
    CALL_FINISHED,
    persona::INTRODUCTION,
];
