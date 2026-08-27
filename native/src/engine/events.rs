use super::models::{MatchEvent, MatchScore};

#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    ScoreChanged(Box<MatchScore>),
    MatchEvent(Box<MatchEvent>),
    MatchesDiscovered(Vec<DiscoveredMatch>),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredMatch {
    pub sport: String,
    pub series_id: String,
    pub match_id: String,
    pub title: String,
    pub status: String,
    pub league_name: String,
    pub start_time: String,
}
