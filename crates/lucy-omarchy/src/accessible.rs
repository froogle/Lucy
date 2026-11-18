use std::error::Error;
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atspi::events::object::StateChangedEvent;
use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::zbus;
use atspi::{AccessibilityConnection, CoordType, ObjectRefOwned, Role, State};
use futures_lite::StreamExt;
use tokio::task::JoinHandle;

use crate::screen::{Element, FocusedWindow, InWindow, Page, Pid, WindowPoint};

const PATIENCE: Duration = Duration::from_secs(2);

const DEEPEST: usize = 32;

const LONGEST: usize = 300;

const HIT_RETRIES: u32 = 2;

const HIT_WAIT: Duration = Duration::from_millis(40);

const MOST_SEARCHED: usize = 4000;

const POPUPS: [Role; 4] = [Role::Menu, Role::PopupMenu, Role::Window, Role::ToolTip];

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

enum Looked<N> {
    Focused,
    Inside(Vec<N>),
}

struct Top<N> {
    node: N,
    name: String,
    role: Role,
    showing: bool,
}

pub async fn text_at(at: InWindow) -> Result<Option<Element>, String> {
    patiently(read(at)).await
}

pub async fn page(window: InWindow) -> Result<Page, String> {
    patiently(find_page(window)).await
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

async fn find_page(window: InWindow) -> Reading<Page> {
    let a11y = AccessibilityConnection::new().await?;
    let bus = a11y.connection();
    let Some(app) = app(&a11y, window.pid).await? else {
        return Ok(Page::Unknown);
    };
    let tops = tops(bus, &app).await?;
    let Some(frame) = frame(&tops, &window.title) else {
        return Ok(Page::Unknown);
    };
    let mut unsearched = std::collections::VecDeque::from([frame.clone()]);
    for _ in 0..MOST_SEARCHED {
        let Some(node) = unsearched.pop_front() else {
            return Ok(Page::Empty);
        };
        let proxy = node.as_accessible_proxy(bus).await?;
        if proxy.get_role().await? == Role::DocumentWeb {
            return Ok(Page::Filled);
        }
        unsearched.extend(proxy.get_children().await?);
    }
    Ok(Page::Filled)
}

pub async fn focused_role(events: &FocusEvents, window: FocusedWindow) -> Result<String, String> {
    match tokio::time::timeout(PATIENCE, find_focused(events.last(), window)).await {
        Ok(found) => found.map_err(|e| format!("the focus can't be found through AT-SPI: {e}")),
        Err(_) => Err(format!(
            "AT-SPI didn't answer within {} s",
            PATIENCE.as_secs()
        )),
    }
}

async fn find_focused(last: Option<ObjectRefOwned>, window: FocusedWindow) -> Reading<String> {
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
    let found = search(frames, |node| look(bus, node))
        .await?
        .ok_or("nothing in the focused window has the focus")?;
    role(bus, found).await
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

async fn role(bus: &zbus::Connection, node: ObjectRefOwned) -> Reading<String> {
    let role = node.as_accessible_proxy(bus).await?.get_role().await?;
    Ok(role.name().to_owned())
}

async fn look(bus: &zbus::Connection, node: ObjectRefOwned) -> Reading<Looked<ObjectRefOwned>> {
    let proxy = node.as_accessible_proxy(bus).await?;
    let state = proxy.get_state().await?;
    if state.contains(State::Focused) {
        return Ok(Looked::Focused);
    }
    if !state.contains(State::Showing) || state.contains(State::ManagesDescendants) {
        return Ok(Looked::Inside(Vec::new()));
    }
    Ok(Looked::Inside(proxy.get_children().await?))
}

async fn search<N, F, H>(tops: Vec<N>, mut look: F) -> Reading<Option<N>>
where
    N: Clone,
    F: FnMut(N) -> H,
    H: Future<Output = Reading<Looked<N>>>,
{
    let mut left: Vec<N> = tops.into_iter().rev().collect();
    for _ in 0..MOST_SEARCHED {
        let Some(node) = left.pop() else {
            return Ok(None);
        };
        match look(node.clone()).await? {
            Looked::Focused => return Ok(Some(node)),
            Looked::Inside(children) => left.extend(children.into_iter().rev()),
        }
    }
    Err(format!("more than {MOST_SEARCHED} elements to search").into())
}

async fn read(at: InWindow) -> Reading<Option<Element>> {
    let a11y = AccessibilityConnection::new().await?;
    let bus = a11y.connection();
    let Some(app) = app(&a11y, at.pid).await? else {
        return Ok(None);
    };
    let tops = tops(bus, &app).await?;
    let Some(frame) = frame(&tops, &at.title) else {
        return Ok(None);
    };
    let here = descend(frame.clone(), |node| hit(bus, node, at.at)).await?;
    if here == *frame {
        return Ok(None);
    }
    element(&here.as_accessible_proxy(bus).await?).await
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
            role: proxy.get_role().await?,
            showing: proxy.get_state().await?.contains(State::Showing),
            node,
        });
    }
    Ok(tops)
}

fn frame<'a, N>(tops: &'a [Top<N>], title: &str) -> Option<&'a N> {
    let mut titled = tops.iter().filter(|top| top.name == title);
    let frame = titled.next()?;
    let popup = tops
        .iter()
        .any(|top| top.showing && POPUPS.contains(&top.role) && !std::ptr::eq(top, frame));
    (titled.next().is_none() && !popup).then_some(&frame.node)
}

async fn hit(
    bus: &zbus::Connection,
    node: ObjectRefOwned,
    at: WindowPoint,
) -> Reading<Option<ObjectRefOwned>> {
    let proxies = node.as_accessible_proxy(bus).await?.proxies().await?;
    let Ok(component) = proxies.component().await else {
        return Ok(None);
    };
    let found = component
        .get_accessible_at_point(at.x, at.y, CoordType::Window)
        .await?;
    Ok((!found.is_null()).then_some(found))
}

async fn descend<N, F, H>(frame: N, mut hit: F) -> Reading<N>
where
    N: Clone + PartialEq,
    F: FnMut(N) -> H,
    H: Future<Output = Reading<Option<N>>>,
{
    let mut here = frame;
    'depth: for _ in 0..DEEPEST {
        for retry in 0..=HIT_RETRIES {
            if retry > 0 {
                tokio::time::sleep(HIT_WAIT).await;
            }
            match hit(here.clone()).await? {
                Some(found) if found != here => {
                    here = found;
                    continue 'depth;
                }
                Some(_) => {}
                None => break 'depth,
            }
        }
        break;
    }
    Ok(here)
}

async fn element(accessible: &AccessibleProxy<'_>) -> Reading<Option<Element>> {
    let name = accessible.name().await?;
    let role = accessible.get_role_name().await?;
    let text = match accessible.proxies().await?.text().await {
        Ok(text) => {
            let count = text.character_count().await?;
            let end = count.min(i32::try_from(LONGEST).expect("LONGEST fits an i32"));
            text.get_text(0, end).await?
        }
        Err(_) => String::new(),
    };
    let (name, text) = (readable(&name), readable(&text));
    if name.is_empty() && text.is_empty() {
        return Ok(None);
    }
    Ok(Some(Element { name, text, role }))
}

fn readable(text: &str) -> String {
    let text: String = text
        .chars()
        .filter(|c| *c != '\u{fffc}')
        .take(LONGEST)
        .collect();
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn top(name: &str, role: Role, showing: bool) -> Top<String> {
        Top {
            node: format!("{name} ({role:?})"),
            name: name.to_owned(),
            role,
            showing,
        }
    }

    fn reading<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a runtime builds")
            .block_on(future)
    }

    #[test]
    fn the_frame_is_the_one_titled_like_the_window() {
        let tops = [
            top("Drafts", Role::Frame, true),
            top("Inbox", Role::Frame, false),
        ];

        assert_eq!(frame(&tops, "Inbox"), Some(&tops[1].node));
    }

    #[test]
    fn no_frame_titled_like_the_window_is_no_frame_even_with_one_active() {
        let tops = [
            top("Drafts", Role::Frame, true),
            top("Settings", Role::Frame, true),
        ];

        assert_eq!(frame(&tops, "Inbox"), None);
    }

    #[test]
    fn two_frames_titled_like_the_window_are_no_frame() {
        let tops = [
            top("Inbox", Role::Frame, true),
            top("Inbox", Role::Frame, true),
        ];

        assert_eq!(frame(&tops, "Inbox"), None);
    }

    #[test]
    fn a_showing_popup_or_menu_of_the_app_is_no_frame() {
        for role in [Role::Menu, Role::PopupMenu, Role::Window, Role::ToolTip] {
            let tops = [top("Inbox", Role::Frame, true), top("", role, true)];

            assert_eq!(frame(&tops, "Inbox"), None, "{role:?}");
        }
    }

    #[test]
    fn a_hidden_popup_or_another_frame_leaves_the_frame() {
        let tops = [
            top("Inbox", Role::Frame, true),
            top("", Role::Menu, false),
            top("Drafts", Role::Frame, true),
        ];

        assert_eq!(frame(&tops, "Inbox"), Some(&tops[0].node));
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

    fn descended(answers: &[(&str, Option<&str>)]) -> (String, Vec<String>) {
        let asked = RefCell::new(Vec::new());
        let answers = RefCell::new(answers.to_vec());
        let here = reading(descend("frame".to_owned(), |node: String| {
            asked.borrow_mut().push(node.clone());
            let answers = &mut *answers.borrow_mut();
            let index = answers.iter().position(|(at, _)| *at == node);
            let answer = index.and_then(|i| answers.remove(i).1.map(str::to_owned));
            async move { Ok(answer) }
        }))
        .expect("the fake tree reads");
        (here, asked.into_inner())
    }

    #[test]
    fn each_depth_asks_again_when_answered_with_itself() {
        let (here, asked) = descended(&[
            ("frame", Some("frame")),
            ("frame", Some("document")),
            ("document", Some("document")),
            ("document", Some("document")),
            ("document", Some("button")),
            ("button", None),
        ]);

        assert_eq!(here, "button");
        assert_eq!(
            asked,
            ["frame", "frame", "document", "document", "document", "button"]
        );
    }

    #[test]
    fn retries_dont_use_up_the_depths() {
        let levels: Vec<String> = (0..DEEPEST).map(|level| level.to_string()).collect();
        let mut answers = Vec::new();
        let mut here = "frame";
        for level in &levels {
            answers.push((here, Some(here)));
            answers.push((here, Some(level.as_str())));
            here = level;
        }

        let (deepest, _) = descended(&answers);

        assert_eq!(deepest, levels[DEEPEST - 1]);
    }

    #[test]
    fn an_object_still_answering_with_itself_after_the_retries_is_where_it_stops() {
        let (here, asked) = descended(&[
            ("frame", Some("button")),
            ("button", Some("button")),
            ("button", Some("button")),
            ("button", Some("button")),
            ("button", Some("label")),
        ]);

        assert_eq!(here, "button");
        assert_eq!(asked.len(), 4);
    }

    fn searched(tree: &[(&str, &[&str])], focused: &str) -> (Reading<Option<String>>, Vec<String>) {
        let looked = RefCell::new(Vec::new());
        let found = reading(search(vec!["frame".to_owned()], |node: String| {
            looked.borrow_mut().push(node.clone());
            let answer = if node == focused {
                Looked::Focused
            } else {
                let children = tree.iter().find(|(at, _)| *at == node);
                let children = children.map_or(&[][..], |(_, children)| children);
                Looked::Inside(children.iter().map(|child| (*child).to_owned()).collect())
            };
            async move { Ok(answer) }
        }));
        (found, looked.into_inner())
    }

    const FORM: [(&str, &[&str]); 3] = [
        ("frame", &["toolbar", "document"]),
        ("toolbar", &["address"]),
        ("document", &["user", "password"]),
    ];

    #[test]
    fn the_search_finds_the_focused_element_looking_in_tree_order() {
        let (found, looked) = searched(&FORM, "password");

        assert_eq!(
            found.expect("the fake tree reads"),
            Some("password".to_owned())
        );
        assert_eq!(
            looked,
            ["frame", "toolbar", "address", "document", "user", "password"]
        );
    }

    #[test]
    fn a_window_with_nothing_focused_has_no_focused_element() {
        let (found, looked) = searched(&FORM, "elsewhere");

        assert_eq!(found.expect("the fake tree reads"), None);
        assert_eq!(looked.len(), 6);
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
    fn a_tree_too_big_to_search_is_an_error() {
        let many: Vec<String> = (0..MOST_SEARCHED).map(|n| n.to_string()).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();

        let (found, looked) = searched(&[("frame", &many)], "elsewhere");

        assert!(found.is_err());
        assert_eq!(looked.len(), MOST_SEARCHED);
    }
}
