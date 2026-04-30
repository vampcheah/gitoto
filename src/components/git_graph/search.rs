use crate::git::graph::GraphRow;

pub(super) struct SearchState {
    pub(super) visible: bool,
    pub(super) input: String,
    pub(super) matches: Vec<usize>,
    pub(super) current_match: Option<usize>,
}

impl SearchState {
    pub(super) fn new() -> Self {
        Self {
            visible: false,
            input: String::new(),
            matches: Vec::new(),
            current_match: None,
        }
    }

    pub(super) fn clear(&mut self) {
        self.visible = false;
        self.input.clear();
        self.matches.clear();
        self.current_match = None;
    }

    pub(super) fn open(&mut self) {
        self.visible = true;
        self.input.clear();
        self.matches.clear();
        self.current_match = None;
    }

    pub(super) fn set_matches(&mut self, matches: Vec<usize>) {
        self.current_match = (!matches.is_empty()).then_some(0);
        self.matches = matches;
    }

    pub(super) fn next_match(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let next = match self.current_match {
            Some(i) => (i + 1) % self.matches.len(),
            None => 0,
        };
        self.current_match = Some(next);
        Some(self.matches[next])
    }

    pub(super) fn prev_match(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let prev = match self.current_match {
            Some(0) | None => self.matches.len() - 1,
            Some(i) => i - 1,
        };
        self.current_match = Some(prev);
        Some(self.matches[prev])
    }
}

pub(super) fn matching_rows(input: &str, rows: &[GraphRow]) -> Vec<usize> {
    if input.is_empty() {
        return Vec::new();
    }

    let query = input.to_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(_, row)| {
            row.message.to_lowercase().contains(&query)
                || row.author.to_lowercase().contains(&query)
                || row.short_id.to_lowercase().contains(&query)
        })
        .map(|(i, _)| i)
        .collect()
}
