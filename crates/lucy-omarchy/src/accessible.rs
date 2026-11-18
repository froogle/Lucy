use std::error::Error;
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atspi::events::object::StateChangedEvent;
use atspi::proxy::accessible::ObjectRefExt;
use atspi::zbus;
use atspi::{AccessibilityConnection, ObjectRefOwned, Role, State};
use futures_lite::StreamExt;
use tokio::task::JoinHandle;

use crate::screen::{FocusedWindow, Pid};

const PATIENCE: Duration = Duration::from_secs(2);

const MOST_SEARCHED: usize = 4000;

const FOCUS_POISONED: &str = "no thread panics while holding the focus";

type Reading<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Default)]
pub struct FocusEvents {
    last: Arc<Mutex<Option<ObjectRefOwned>>>,
    following: Mutex<Option<JoinHandle<()>>>,
}

impl FocusEvents {
    pub fn follow(&self) {
        self.keep_following(|last| async move {
            if let Err(e) = follow(&last).await {
                eprintln!("omarchy: AT-SPI's focus events can't be followed: {e}");
            }
        });
    }

    fn keep_following<F, H>(&self, listen: F)
    where
        F: FnOnce(Arc<Mutex<Option<ObjectRefOwned>>>) -> H,
        H: Future<Output = ()> + Send + 'static,
    {
        let mut following = self.following.lock().expect(FOCUS_POISONED);
        if following.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        *following = Some(tokio::spawn(listen(self.last.clone())));
    }

    #[cfg(test)]
    fn stopped(&self) -> bool {
        self.following
            .lock()
            .expect(FOCUS_POISONED)
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
    }

    fn last(&self) -> Option<ObjectRefOwned> {
        self.last.lock().expect(FOCUS_POISONED).clone()
    }
}

async fn follow(last: &Mutex<Option<ObjectRefOwned>>) -> Reading<()> {
    let a11y = AccessibilityConnection::new().await?;
    let mut events = pin!(a11y.event_stream());
    a11y.register_event::<StateChangedEvent>().await?;
    while let Some(event) = events.next().await {
        let Ok(changed) = event.and_then(StateChangedEvent::try_from) else {
            continue;
        };
        if changed.state == State::Focused && changed.enabled {
            *last.lock().expect(FOCUS_POISONED) = Some(changed.item);
        }
    }
    Ok(())
}

enum Seen<N> {
    Sought,
    Inside(Vec<N>),
}

#[derive(Debug, PartialEq, Eq)]
enum Walked<N> {
    Found(N),
    Absent,
    Unfinished,
}

struct Top<N> {
    node: N,
    name: String,
}

async fn patiently<T>(reading: impl Future<Output = Reading<T>>) -> Result<T, String> {
    match tokio::time::timeout(PATIENCE, reading).await {
        Ok(read) => read.map_err(|e| format!("AT-SPI can't be read: {e}")),
        Err(_) => Err(format!(
            "AT-SPI didn't answer within {} s",
            PATIENCE.as_secs()
        )),
    }
}

pub async fn focused_role(events: &FocusEvents, window: FocusedWindow) -> Result<Role, String> {
    patiently(find_focused(events.last(), window)).await
}

async fn find_focused(last: Option<ObjectRefOwned>, window: FocusedWindow) -> Reading<Role> {
    let a11y = AccessibilityConnection::new().await?;
    let bus = a11y.connection();
    let app = app(&a11y, window.pid)
        .await?
        .ok_or("no app on the bus owns the focused window")?;
    let frames: Vec<ObjectRefOwned> = tops(bus, &app)
        .await?
        .into_iter()
        .filter(|top| top.name == window.title)
        .map(|top| top.node)
        .collect();
    if let Some(last) = last.filter(|last| last.name() == app.name()) {
        if focused(bus, &last).await
            && within(last.clone(), &frames, |node| parent(bus, node))
                .await
                .is_ok_and(|inside| inside)
        {
            return role(bus, last).await;
        }
    }
    match walk(frames, |node| focus_in(bus, node)).await? {
        Walked::Found(found) => role(bus, found).await,
        Walked::Absent => Err("nothing in the focused window has the focus".into()),
        Walked::Unfinished => Err(format!("more than {MOST_SEARCHED} elements to search").into()),
    }
}

async fn focused(bus: &zbus::Connection, node: &ObjectRefOwned) -> bool {
    let Ok(proxy) = node.as_accessible_proxy(bus).await else {
        return false;
    };
    proxy
        .get_state()
        .await
        .is_ok_and(|state| state.contains(State::Focused))
}

async fn parent(bus: &zbus::Connection, node: ObjectRefOwned) -> Reading<Option<ObjectRefOwned>> {
    let up = node.as_accessible_proxy(bus).await?.parent().await?;
    Ok((!up.is_null()).then_some(up))
}

async fn within<N, F, H>(node: N, frames: &[N], mut parent: F) -> Reading<bool>
where
    N: PartialEq,
    F: FnMut(N) -> H,
    H: Future<Output = Reading<Option<N>>>,
{
    let mut here = node;
    for _ in 0..MOST_SEARCHED {
        if frames.contains(&here) {
            return Ok(true);
        }
        match parent(here).await? {
            Some(up) => here = up,
            None => return Ok(false),
        }
    }
    Ok(false)
}

async fn role(bus: &zbus::Connection, node: ObjectRefOwned) -> Reading<Role> {
    Ok(node.as_accessible_proxy(bus).await?.get_role().await?)
}

async fn focus_in(bus: &zbus::Connection, node: ObjectRefOwned) -> Reading<Seen<ObjectRefOwned>> {
    let proxy = node.as_accessible_proxy(bus).await?;
    let state = proxy.get_state().await?;
    if state.contains(State::Focused) {
        return Ok(Seen::Sought);
    }
    if !state.contains(State::Showing) || state.contains(State::ManagesDescendants) {
        return Ok(Seen::Inside(Vec::new()));
    }
    Ok(Seen::Inside(proxy.get_children().await?))
}

async fn walk<N, F, H>(tops: Vec<N>, mut see: F) -> Reading<Walked<N>>
where
    N: Clone,
    F: FnMut(N) -> H,
    H: Future<Output = Reading<Seen<N>>>,
{
    let mut left: Vec<N> = tops.into_iter().rev().collect();
    for _ in 0..MOST_SEARCHED {
        let Some(node) = left.pop() else {
            return Ok(Walked::Absent);
        };
        match see(node.clone()).await? {
            Seen::Sought => return Ok(Walked::Found(node)),
            Seen::Inside(children) => left.extend(children.into_iter().rev()),
        }
    }
    Ok(Walked::Unfinished)
}

async fn app(a11y: &AccessibilityConnection, pid: Pid) -> Reading<Option<ObjectRefOwned>> {
    let dbus = zbus::fdo::DBusProxy::new(a11y.connection()).await?;
    for app in a11y
        .root_accessible_on_registry()
        .await?
        .get_children()
        .await?
    {
        let Some(name) = app.name() else { continue };
        let owner = dbus
            .get_connection_unix_process_id(name.clone().into())
            .await;
        if owner.ok() == Some(pid.0) {
            return Ok(Some(app));
        }
    }
    Ok(None)
}

async fn tops(bus: &zbus::Connection, app: &ObjectRefOwned) -> Reading<Vec<Top<ObjectRefOwned>>> {
    let mut tops = Vec::new();
    for node in app.as_accessible_proxy(bus).await?.get_children().await? {
        let proxy = node.as_accessible_proxy(bus).await?;
        tops.push(Top {
            name: proxy.name().await?,
            node,
        });
    }
    Ok(tops)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn reading<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a runtime builds")
            .block_on(future)
    }

    fn started(events: &FocusEvents, starts: &Arc<Mutex<u32>>, ends: bool) {
        let starts = starts.clone();
        events.keep_following(move |_| async move {
            *starts.lock().expect("the count lock holds") += 1;
            if !ends {
                std::future::pending::<()>().await;
            }
        });
    }

    async fn ended(events: &FocusEvents) {
        while !events.stopped() {
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn following_starts_once_while_it_runs() {
        let starts = Arc::new(Mutex::new(0));
        reading(async {
            let events = FocusEvents::default();
            started(&events, &starts, false);
            tokio::task::yield_now().await;
            started(&events, &starts, false);
            tokio::task::yield_now().await;
        });

        assert_eq!(*starts.lock().expect("the count lock holds"), 1);
    }

    #[test]
    fn following_starts_again_after_it_stops() {
        let starts = Arc::new(Mutex::new(0));
        reading(async {
            let events = FocusEvents::default();
            started(&events, &starts, true);
            ended(&events).await;
            started(&events, &starts, true);
            ended(&events).await;
        });

        assert_eq!(*starts.lock().expect("the count lock holds"), 2);
    }

    fn walked(tree: &[(&str, &[&str])], sought: &str) -> (Walked<String>, Vec<String>) {
        let seen = RefCell::new(Vec::new());
        let found = reading(walk(vec!["frame".to_owned()], |node: String| {
            seen.borrow_mut().push(node.clone());
            let answer = if node == sought {
                Seen::Sought
            } else {
                let children = tree.iter().find(|(at, _)| *at == node);
                let children = children.map_or(&[][..], |(_, children)| children);
                Seen::Inside(children.iter().map(|child| (*child).to_owned()).collect())
            };
            async move { Ok(answer) }
        }))
        .expect("the fake tree reads");
        (found, seen.into_inner())
    }

    const FORM: [(&str, &[&str]); 3] = [
        ("frame", &["toolbar", "document"]),
        ("toolbar", &["address"]),
        ("document", &["user", "password"]),
    ];

    #[test]
    fn the_walk_finds_the_sought_element_looking_in_tree_order() {
        let (found, seen) = walked(&FORM, "password");

        assert_eq!(found, Walked::Found("password".to_owned()));
        assert_eq!(
            seen,
            ["frame", "toolbar", "address", "document", "user", "password"]
        );
    }

    #[test]
    fn a_tree_without_the_sought_element_is_walked_whole() {
        let (found, seen) = walked(&FORM, "elsewhere");

        assert_eq!(found, Walked::Absent);
        assert_eq!(seen.len(), 6);
    }

    fn inside(parents: &[(&str, &str)], node: &str, frames: &[&str]) -> bool {
        let frames: Vec<String> = frames.iter().map(|frame| (*frame).to_owned()).collect();
        reading(within(node.to_owned(), &frames, |node: String| {
            let up = parents.iter().find(|(child, _)| *child == node);
            let up = up.map(|(_, parent)| (*parent).to_owned());
            async move { Ok(up) }
        }))
        .expect("the fake tree reads")
    }

    const TWO_WINDOWS: [(&str, &str); 6] = [
        ("password", "document"),
        ("document", "frame"),
        ("frame", "app"),
        ("search", "other document"),
        ("other document", "other frame"),
        ("other frame", "app"),
    ];

    #[test]
    fn an_element_under_the_frame_is_within_it() {
        assert!(inside(&TWO_WINDOWS, "password", &["frame"]));
    }

    #[test]
    fn an_element_in_another_window_of_the_app_isnt_within_the_frame() {
        assert!(!inside(&TWO_WINDOWS, "search", &["frame"]));
    }

    #[test]
    fn a_tree_too_big_to_walk_is_unfinished() {
        let many: Vec<String> = (0..MOST_SEARCHED).map(|n| n.to_string()).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();

        let (found, seen) = walked(&[("frame", &many)], "elsewhere");

        assert_eq!(found, Walked::Unfinished);
        assert_eq!(seen.len(), MOST_SEARCHED);
    }
}
