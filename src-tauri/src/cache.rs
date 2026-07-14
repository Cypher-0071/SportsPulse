use std::sync::{Arc, RwLock};
use crate::models::MatchScore;

#[derive(Clone)]
pub struct ScoreCache {
    pub current_score: Arc<RwLock<Option<MatchScore>>>,
}

impl ScoreCache {
    pub fn new() -> Self {
        Self {
            current_score: Arc::new(RwLock::new(None)),
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
}
