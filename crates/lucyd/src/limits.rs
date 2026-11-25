use crate::adapters::RateLimit;

pub fn spoken(limit: &RateLimit, provider: &str) -> String {
    let now = now();
    let reset = limit
        .resets_at
        .and_then(|at| Some((local(at)?, local(now)?)));
    phrase(limit.rate_limit_type.as_deref(), provider, reset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Local {
    years_since_1900: i32,
    day_of_year: i32,
    days_since_sunday: i32,
    hour: i32,
    minute: i32,
}

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
        let day =
            if (at.years_since_1900, at.day_of_year) == (now.years_since_1900, now.day_of_year) {
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
                format!("on {} ", DAYS[at.days_since_sunday.rem_euclid(7) as usize])
            };
        said.push_str(&format!(
            " It resets {day}at {hour}:{:02} {half}.",
            at.minute
        ));
    }
    said
}

pub fn today() -> Option<String> {
    let tm = local_time(now())?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    ))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |now| now.as_secs())
}

fn local(secs: u64) -> Option<Local> {
    let tm = local_time(secs)?;
    Some(Local {
        years_since_1900: tm.tm_year,
        day_of_year: tm.tm_yday,
        days_since_sunday: tm.tm_wday,
        hour: tm.tm_hour,
        minute: tm.tm_min,
    })
}

fn local_time(secs: u64) -> Option<libc::tm> {
    let time = libc::time_t::try_from(secs).ok()?;
    // SAFETY: `tm` is plain data that localtime_r fills in; both pointers are
    // valid for the call.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&time, &mut tm).is_null() {
            return None;
        }
        Some(tm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day_of_year: i32, days_since_sunday: i32, hour: i32, minute: i32) -> Local {
        Local {
            years_since_1900: 126,
            day_of_year,
            days_since_sunday,
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
