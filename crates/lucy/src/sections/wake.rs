use ratatui::backend::Backend;

use super::{Context, Page};
use crate::ui::{Stop, Ui};

const TEST_SECS: f64 = 10.0;

pub(super) fn page() -> Page {
    Page::new(vec![format!(
        "Test the Wake word: say it within {TEST_SECS} seconds"
    )])
    .needing_lucyd()
}

pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    let Some(mut client) = context.lucyd(ui, "and the test needs it")? else {
        return Ok(());
    };
    ui.say(format!(
        "Say the Wake word in the next {TEST_SECS} seconds..."
    ))?;
    match client.test_wake_word(TEST_SECS) {
        Ok(result) if result.heard => {
            ui.say(format!("Heard it, with a score of {:.2}.", result.score))
        }
        Ok(result) => ui.say(format!(
            "Lucy didn't hear it. Its best score was {:.2}; [wake_word] threshold \
             in config.toml sets the score it needs.",
            result.score
        )),
        Err(e) => ui.say(format!("Can't test it: {e}")),
    }
}
