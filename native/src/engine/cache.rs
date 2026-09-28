use super::models::{MatchEvent, MatchScore};
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct ScoreCache {
    pub current_score: Arc<RwLock<Option<MatchScore>>>,
    pub latest_event: Arc<RwLock<Option<MatchEvent>>>,
}

impl ScoreCache {
    pub fn new() -> Self {
        Self {
            current_score: Arc::new(RwLock::new(None)),
            latest_event: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set(&self, score: Option<MatchScore>) {
        // Recover from a poisoned lock instead of dropping the write: a panic
        // elsewhere must not freeze the overlay on stale data (`None` here
        // would fake a NoMatch).
        let mut w = self
            .current_score
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *w = score;
    }

    pub fn get(&self) -> Option<MatchScore> {
        self.current_score
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_latest_event(&self, event: Option<MatchEvent>) {
        let mut w = self.latest_event.write().unwrap_or_else(|e| e.into_inner());
        *w = event;
    }

    pub fn get_latest_event(&self) -> Option<MatchEvent> {
        self.latest_event
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn clear(&self) {
        self.set(None);
        self.set_latest_event(None);
    }
}
