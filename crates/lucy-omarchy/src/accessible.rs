use std::error::Error;
use std::future::Future;
use std::time::Duration;

use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::zbus;
use atspi::{AccessibilityConnection, CoordType, ObjectRefOwned, Role, State};

use crate::screen::{Element, InWindow, Page, Pid, WindowPoint};

const PATIENCE: Duration = Duration::from_secs(2);

const DEEPEST: usize = 32;

const LONGEST: usize = 300;

const HIT_RETRIES: u32 = 2;

const HIT_WAIT: Duration = Duration::from_millis(40);

const MOST_SEARCHED: usize = 500;

const POPUPS: [Role; 4] = [Role::Menu, Role::PopupMenu, Role::Window, Role::ToolTip];

type Reading<T> = Result<T, Box<dyn Error + Send + Sync>>;

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
    patiently(search(window)).await
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

async fn search(window: InWindow) -> Reading<Page> {
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
}
