use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchStatus {
    Live,
    Break,
    NoMatch,
    Scheduled,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamScore {
    pub id: String,
    pub name: String,
    pub abbreviation: String,
    pub score: String,
    pub runs: u32,
    pub wickets: u32,
    pub overs: f32,
    pub is_batting: bool,
    pub is_winner: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SportType {
    Cricket,
    Soccer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchScore {
    pub match_id: String,
    pub series_id: String,
    pub match_title: String,
    pub status: MatchStatus,
    pub team1: TeamScore,
    pub team2: TeamScore,
    pub batting_team: u8, // 1 for team1, 2 for team2, 0 if none
    pub crr: f32,
    pub rrr: Option<f32>,
    pub target: Option<u32>,
    pub runs_needed: Option<u32>,
    pub timestamp: u64,
    pub sport: SportType,
    pub soccer_clock: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchEventType {
    Wicket,
    Boundary,
    Win,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchEvent {
    pub event_type: MatchEventType,
    pub title: String,
    pub description: String,
    pub score: String,
}
