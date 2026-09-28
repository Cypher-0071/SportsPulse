use super::models::{MatchEvent, MatchScore, SportType};

#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    ScoreChanged(Box<MatchScore>),
    MatchEvent(Box<MatchEvent>),
    MatchesDiscovered(Vec<DiscoveredMatch>),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredMatch {
    /// Typed sport (`Display` → `"cricket"`/`"soccer"` for ESPN URLs).
    /// Replaces the former `String` so dashboard filtering and fetcher
    /// routing compare enums instead of string literals.
    pub sport: SportType,
    pub series_id: String,
    pub match_id: String,
    pub title: String,
    pub status: String,
    pub league_name: String,
    pub start_time: String,
}
