use rmcp::model::CallToolResult;
use serde_json::Value;

use crate::mcp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Brave,
    Chrome,
    Chromium,
}

impl Browser {
    pub fn off(result: &CallToolResult) -> Option<Browser> {
        if result.is_error == Some(true) {
            return None;
        }
        let answer: Value = serde_json::from_str(&mcp::result_text(result)).ok()?;
        match answer["accessibility_off"].as_str()? {
            "brave" => Some(Browser::Brave),
            "chrome" => Some(Browser::Chrome),
            "chromium" => Some(Browser::Chromium),
            _ => None,
        }
    }

    pub fn program(self) -> &'static str {
        match self {
            Browser::Brave => "brave",
            Browser::Chrome => "google-chrome-stable",
            Browser::Chromium => "chromium",
        }
    }

    pub fn settings(self) -> &'static str {
        match self {
            Browser::Brave => "brave://accessibility",
            Browser::Chrome | Browser::Chromium => "chrome://accessibility",
        }
    }

    pub fn offer(self) -> String {
        let (name, flags) = match self {
            Browser::Brave => ("Brave", "brave-flags.conf"),
            Browser::Chrome => ("Chrome", "chrome-flags.conf"),
            Browser::Chromium => ("Chromium", "chromium-flags.conf"),
        };
        format!(
            "I can't read the page in {name}, so I'll ask before each click. Its accessibility \
             switch turns off when {name} restarts; the force-renderer-accessibility flag in \
             {flags} keeps it on. Want me to open {name}'s accessibility settings?"
        )
    }
}
