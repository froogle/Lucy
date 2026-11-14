//! How text Lucy writes for the model begins. None of it is for the user to
//! read: a notification or anything else shown says it plainly instead.

use crate::persona;

/// What Lucy tells the model about an Authoring build.
pub const PLUGIN_BUILD: &str = "[Lucy's Plugin build]";
/// A Turn the user cut into, before or after some of the reply played.
pub const INTERRUPTED: &str = "[You were interrupted";
/// A confirmed call's result that came in after a Barge-in.
pub const CALL_FINISHED: &str = "[A confirmed call finished after the user cut in:";

/// How each piece of text written for the model begins, or the whole of it.
pub const MARKERS: &[&str] = &[
    PLUGIN_BUILD,
    INTERRUPTED,
    CALL_FINISHED,
    persona::INTRODUCTION,
];
