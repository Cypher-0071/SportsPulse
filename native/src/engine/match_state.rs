use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct ActiveMatchesState {
    pub active_matches: Arc<Mutex<Vec<(String, String, String, String, String, String, String)>>>, // (sport, series_id, match_id, match_title, status, league_name, start_time)
    pub selected_match: Arc<Mutex<Option<(String, String, String)>>>,      // (sport, series_id, match_id)
    pub notify: Arc<Notify>,
    pub initial_fetch_completed: Arc<AtomicBool>,
}

impl ActiveMatchesState {
    pub fn new() -> Self {
        Self {
            active_matches: Arc::new(Mutex::new(Vec::new())),
            selected_match: Arc::new(Mutex::new(None)),
            notify: Arc::new(Notify::new()),
            initial_fetch_completed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn select_match(&self, sport: String, series_id: String, match_id: String) {
        if let Ok(mut sel) = self.selected_match.lock() {
            *sel = Some((sport, series_id, match_id));
        }
        self.notify.notify_one();
    }

    pub fn untrack_match(&self) {
        if let Ok(mut sel) = self.selected_match.lock() {
            *sel = None;
        }
        self.notify.notify_one();
    }

    pub fn get_selected_match(&self) -> Option<(String, String, String)> {
        self.selected_match.lock().ok().and_then(|s| s.clone())
    }
}
