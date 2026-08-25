#[derive(Debug, Clone)]
pub enum AppEvent {
    ScoreChanged(Box<crate::engine::models::MatchScore>),
    MatchEvent(Box<crate::engine::models::MatchEvent>),
    MatchesDiscovered(Vec<DiscoveredMatch>),
}

#[derive(Debug, Clone)]
pub struct DiscoveredMatch {
    pub sport: String,
    pub series_id: String,
    pub match_id: String,
    pub title: String,
    pub status: String,
    pub league_name: String,
    pub start_time: String,
}
