use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ActiveMatchesState {
    pub active_matches: Arc<Mutex<Vec<(String, String, String, String)>>>, // (sport, series_id, match_id, match_title)
    pub selected_match: Arc<Mutex<Option<(String, String, String)>>>,      // (sport, series_id, match_id)
}

impl ActiveMatchesState {
    pub fn new() -> Self {
        Self {
            active_matches: Arc::new(Mutex::new(Vec::new())),
            selected_match: Arc::new(Mutex::new(None)),
        }
    }
}
