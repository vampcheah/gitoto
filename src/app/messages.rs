use std::time::Instant;

use crate::app::App;

impl App {
    pub(super) fn set_error_message(&mut self, message: impl Into<String>) {
        self.error_message = Some((message.into(), Instant::now()));
    }

    pub(super) fn set_success_message(&mut self, message: impl Into<String>) {
        self.success_message = Some((message.into(), Instant::now()));
    }

    pub(super) fn set_sanitized_error_message(&mut self, message: &str) {
        let clean: String = message
            .chars()
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect();
        let truncated = if clean.len() > 120 {
            format!("{}...", &clean[..117])
        } else {
            clean
        };
        self.set_error_message(truncated);
    }
}
