use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

use super::models::SportType;

/// One row of the scoreboard discovery list:
/// (sport, series_id, match_id, match_title, status, league_name, start_time).
/// Named alias so `clippy::type_complexity` stays quiet on the 7-tuple.
pub type ActiveMatchEntry = (SportType, String, String, String, String, String, String);

/// Currently tracked selection: (sport, series_id, match_id).
pub type SelectedMatch = (SportType, String, String);

#[derive(Clone, Default)]
pub struct ActiveMatchesState {
    pub active_matches: Arc<Mutex<Vec<ActiveMatchEntry>>>,
    pub selected_match: Arc<Mutex<Option<SelectedMatch>>>,
    /// Wake-up signal for the fetcher's `select!` sleep. `Notify` coalesces
    /// multiple `notify_one` calls into a single permit, which is safe here:
    /// the fetcher is the only consumer and every wake-up re-reads the full
    /// selection state, so coalesced wakes can never lose a selection.
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

    /// ESPN path segments allow `[A-Za-z0-9._-]{1,64}` — the same shape the
    /// fetcher interpolates into `detail_url`, so anything else is rejected.
    pub fn is_valid_id(s: &str) -> bool {
        !s.is_empty()
            && s.len() <= 64
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    }

    pub fn is_valid_selection(sport: SportType, series_id: &str, match_id: &str) -> bool {
        // `SportType` is always a valid slug by construction; only the ESPN
        // path-segment ids need shape validation here.
        let _ = sport;
        Self::is_valid_id(series_id) && Self::is_valid_id(match_id)
    }

    /// String-slug entry point (dashboard bridge / tests): parses
    /// `"cricket"`/`"soccer"`/`"football"` via `SportType::parse_slug`
    /// before delegating to the typed check above.
    pub fn is_valid_selection_str(sport: &str, series_id: &str, match_id: &str) -> bool {
        SportType::parse_slug(sport)
            .is_some_and(|s| Self::is_valid_selection(s, series_id, match_id))
    }

    pub fn select_match(&self, sport: SportType, series_id: String, match_id: String) {
        if !Self::is_valid_selection(sport, &series_id, &match_id) {
            #[cfg(debug_assertions)]
            eprintln!(
                "[DEBUG] select_match ignored invalid selection: sport={} series={} match={}",
                sport, series_id, match_id
            );
            return;
        }
        // Recover from a poisoned mutex (a panic elsewhere must not wedge
        // selection): `into_inner` keeps the last good value instead of
        // silently dropping the write like `if let Ok(...)` would.
        *self
            .selected_match
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some((sport, series_id, match_id));
        self.notify.notify_one();
    }

    pub fn untrack_match(&self) {
        *self
            .selected_match
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.notify.notify_one();
    }

    pub fn get_selected_match(&self) -> Option<SelectedMatch> {
        self.selected_match
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_id_shapes() {
        assert!(ActiveMatchesState::is_valid_id("14135"));
        assert!(ActiveMatchesState::is_valid_id("eng.1"));
        assert!(ActiveMatchesState::is_valid_id("a-b_c.d1"));
        assert!(!ActiveMatchesState::is_valid_id(""));
        assert!(!ActiveMatchesState::is_valid_id("a/b"));
        assert!(!ActiveMatchesState::is_valid_id("a b"));
        assert!(!ActiveMatchesState::is_valid_id("a?b"));
        assert!(!ActiveMatchesState::is_valid_id(&"x".repeat(65)));
        assert!(ActiveMatchesState::is_valid_id(&"x".repeat(64)));
    }

    #[test]
    fn test_is_valid_selection_typed_and_str() {
        assert!(ActiveMatchesState::is_valid_selection(
            SportType::Cricket,
            "14135",
            "1413511"
        ));
        assert!(ActiveMatchesState::is_valid_selection(
            SportType::Soccer,
            "eng.1",
            "700100"
        ));
        assert!(!ActiveMatchesState::is_valid_selection(
            SportType::Cricket,
            "",
            "1413511"
        ));
        assert!(!ActiveMatchesState::is_valid_selection(
            SportType::Cricket,
            "14135",
            "bad/id"
        ));
        assert!(ActiveMatchesState::is_valid_selection_str(
            "cricket", "14135", "1413511"
        ));
        assert!(ActiveMatchesState::is_valid_selection_str(
            "soccer", "eng.1", "700100"
        ));
        assert!(ActiveMatchesState::is_valid_selection_str(
            "football", "eng.1", "700100"
        ));
        assert!(!ActiveMatchesState::is_valid_selection_str(
            "hockey", "14135", "1413511"
        ));
        assert!(!ActiveMatchesState::is_valid_selection_str(
            "cricket", "", "1413511"
        ));
    }

    #[test]
    fn test_poison_recovery_keeps_selection() {
        let state = ActiveMatchesState::new();
        state.select_match(
            SportType::Cricket,
            "14135".to_string(),
            "1413511".to_string(),
        );
        // Poison the mutex: a panicking holder must not wedge later locks.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.selected_match.lock().unwrap();
            panic!("intentional poison");
        }));
        assert!(state.selected_match.is_poisoned());
        // Our `unwrap_or_else(into_inner)` path recovers instead of dropping.
        state.select_match(SportType::Soccer, "eng.1".to_string(), "700100".to_string());
        let sel = state
            .get_selected_match()
            .expect("selection survives poison");
        assert_eq!(sel.0, SportType::Soccer);
        assert_eq!(sel.1, "eng.1");
        assert_eq!(sel.2, "700100");
        // Untrack also recovers through a poisoned lock.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.selected_match.lock().unwrap();
            panic!("poison again");
        }));
        state.untrack_match();
        assert!(state.get_selected_match().is_none());
    }
}
