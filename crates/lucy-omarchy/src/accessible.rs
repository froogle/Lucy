use std::error::Error;
use std::time::Duration;

use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::zbus;
use atspi::{AccessibilityConnection, CoordType, ObjectRefOwned, State};

use crate::screen::{Element, InWindow};

const PATIENCE: Duration = Duration::from_secs(2);

const DEEPEST: usize = 32;

const LONGEST: usize = 300;

const HIT_RETRIES: u32 = 2;

const HIT_WAIT: Duration = Duration::from_millis(40);

type Read<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub async fn text_at(at: InWindow) -> Result<Option<Element>, String> {
    match tokio::time::timeout(PATIENCE, read(at)).await {
        Ok(read) => read.map_err(|e| format!("AT-SPI can't be read: {e}")),
        Err(_) => Err(format!(
            "AT-SPI didn't answer within {} s",
            PATIENCE.as_secs()
        )),
    }
}

async fn read(at: InWindow) -> Read<Option<Element>> {
    let a11y = AccessibilityConnection::new().await?;
    let bus = a11y.connection();
    let Some(frame) = frame(&a11y, &at).await? else {
        return Ok(None);
    };
    let mut here = frame.clone();
    let mut retries = 0;
    for _ in 0..DEEPEST {
        let proxies = here.as_accessible_proxy(bus).await?.proxies().await?;
        let Ok(component) = proxies.component().await else {
            break;
        };
        let found = component
            .get_accessible_at_point(at.x, at.y, CoordType::Window)
            .await?;
        if found == here && retries < HIT_RETRIES {
            retries += 1;
            tokio::time::sleep(HIT_WAIT).await;
            continue;
        }
        if found.is_null() || found == here {
            break;
        }
        here = found;
    }
    if here == frame {
        return Ok(None);
    }
    element(&here.as_accessible_proxy(bus).await?).await
}

async fn frame(a11y: &AccessibilityConnection, at: &InWindow) -> Read<Option<ObjectRefOwned>> {
    let bus = a11y.connection();
    let dbus = zbus::fdo::DBusProxy::new(bus).await?;
    for app in a11y
        .root_accessible_on_registry()
        .await?
        .get_children()
        .await?
    {
        let Some(name) = app.name() else { continue };
        let pid = dbus
            .get_connection_unix_process_id(name.clone().into())
            .await;
        if pid.ok() != Some(at.pid) {
            continue;
        }
        let frames = app.as_accessible_proxy(bus).await?.get_children().await?;
        return pick(bus, frames, &at.title).await;
    }
    Ok(None)
}

async fn pick(
    bus: &zbus::Connection,
    frames: Vec<ObjectRefOwned>,
    title: &str,
) -> Read<Option<ObjectRefOwned>> {
    let mut active = None;
    for frame in &frames {
        let proxy = frame.as_accessible_proxy(bus).await?;
        if proxy.name().await? == title {
            return Ok(Some(frame.clone()));
        }
        if active.is_none() && proxy.get_state().await?.contains(State::Active) {
            active = Some(frame.clone());
        }
    }
    Ok(active.or_else(|| frames.into_iter().next()))
}

async fn element(accessible: &AccessibleProxy<'_>) -> Read<Option<Element>> {
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
