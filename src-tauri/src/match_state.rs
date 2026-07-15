use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

#[derive(Clone)]
pub struct ActiveMatchesState {
    pub active_matches: Arc<Mutex<Vec<(String, String, String, String, String, String)>>>, // (sport, series_id, match_id, match_title, status, league_name)
    pub selected_match: Arc<Mutex<Option<(String, String, String)>>>,      // (sport, series_id, match_id)
    pub notify: Arc<Notify>,
}

impl ActiveMatchesState {
    pub fn new() -> Self {
        Self {
            active_matches: Arc::new(Mutex::new(Vec::new())),
            selected_match: Arc::new(Mutex::new(None)),
            notify: Arc::new(Notify::new()),
        }
    }
}
