//! What Lucy says when the subscription has hit a plan or usage limit.

use crate::adapters::RateLimit;

/// "You've reached your Claude five-hour usage limit. It resets at 3:05 PM.",
/// in local time, with the day if it isn't today.
pub fn spoken(limit: &RateLimit, provider: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |now| now.as_secs());
    let reset = limit
        .resets_at
        .and_then(|at| Some((local(at)?, local(now)?)));
    phrase(limit.rate_limit_type.as_deref(), provider, reset)
}

/// A moment on the local clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Local {
    /// Years since 1900, and the day of that year, so moments compare by day.
    year: i32,
    day_of_year: i32,
    /// 0 is Sunday.
    weekday: i32,
    hour: i32,
    minute: i32,
}

/// `reset` is the reset time and now.
fn phrase(window: Option<&str>, provider: &str, reset: Option<(Local, Local)>) -> String {
    let window = match window {
        Some("five_hour") => "five-hour ",
        Some("seven_day" | "seven_day_opus" | "seven_day_sonnet") => "weekly ",
        _ => "",
    };
    let mut said = format!("You've reached your {provider} {window}usage limit.");
    if let Some((at, now)) = reset {
        let (hour, half) = match at.hour {
            0 => (12, "AM"),
            h @ 1..=11 => (h, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        let day = if (at.year, at.day_of_year) == (now.year, now.day_of_year) {
            String::new()
        } else {
            const DAYS: [&str; 7] = [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ];
            format!("on {} ", DAYS[at.weekday.rem_euclid(7) as usize])
        };
        said.push_str(&format!(
            " It resets {day}at {hour}:{:02} {half}.",
            at.minute
        ));
    }
    said
}

/// `secs` since the Unix epoch on the local clock, as `TZ` says.
fn local(secs: u64) -> Option<Local> {
    let time = libc::time_t::try_from(secs).ok()?;
    // SAFETY: `tm` is plain data that localtime_r fills in; both pointers are
    // valid for the call.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&time, &mut tm).is_null() {
            return None;
        }
        tm
    };
    Some(Local {
        year: tm.tm_year,
        day_of_year: tm.tm_yday,
        weekday: tm.tm_wday,
        hour: tm.tm_hour,
        minute: tm.tm_min,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day_of_year: i32, weekday: i32, hour: i32, minute: i32) -> Local {
        Local {
            year: 126,
            day_of_year,
            weekday,
            hour,
            minute,
        }
    }

    #[test]
    fn a_reset_later_today_is_said_as_a_clock_time() {
        let reset = (at(316, 5, 15, 5), at(316, 5, 10, 0));
        assert_eq!(
            phrase(Some("five_hour"), "Claude", Some(reset)),
            "You've reached your Claude five-hour usage limit. It resets at 3:05 PM."
        );
    }

    #[test]
    fn a_reset_on_another_day_names_the_day() {
        let reset = (at(319, 1, 0, 30), at(316, 5, 22, 0));
        assert_eq!(
            phrase(Some("seven_day"), "Claude", Some(reset)),
            "You've reached your Claude weekly usage limit. It resets on Monday at 12:30 AM."
        );
    }

    #[test]
    fn noon_is_pm() {
        let reset = (at(316, 5, 12, 0), at(316, 5, 9, 0));
        assert!(phrase(None, "Claude", Some(reset)).ends_with("at 12:00 PM."));
    }
}
