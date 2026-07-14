use std::sync::{Arc, RwLock};
use crate::models::{MatchScore, MatchEvent};

#[derive(Clone)]
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
        if let Ok(mut w) = self.current_score.write() {
            *w = score;
        }
    }

    pub fn get(&self) -> Option<MatchScore> {
        if let Ok(r) = self.current_score.read() {
            r.clone()
        } else {
            None
        }
    }

    pub fn set_latest_event(&self, event: Option<MatchEvent>) {
        if let Ok(mut w) = self.latest_event.write() {
            *w = event;
        }
    }

    pub fn get_latest_event(&self) -> Option<MatchEvent> {
        if let Ok(r) = self.latest_event.read() {
            r.clone()
        } else {
            None
        }
    }
}
