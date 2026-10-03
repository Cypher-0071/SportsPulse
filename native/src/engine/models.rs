use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchStatus {
    Live,
    Break,
    NoMatch,
    Scheduled,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

impl SportType {
    /// URL/API slug (`"cricket"` / `"soccer"`). Single source of truth for
    /// ESPN `detail_url` interpolation and scoreboard query params.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cricket => "cricket",
            Self::Soccer => "soccer",
        }
    }

    /// Parse a slug back into `SportType`. Accepts `"cricket"` and
    /// `"soccer"`/`"football"` (dashboard labels football, ESPN uses soccer).
    /// Returns `None` for anything else so callers reject invalid selections
    /// instead of building a bad URL.
    pub fn parse_slug(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("cricket") {
            Some(Self::Cricket)
        } else if s.eq_ignore_ascii_case("soccer") || s.eq_ignore_ascii_case("football") {
            Some(Self::Soccer)
        } else {
            None
        }
    }
}

impl std::fmt::Display for SportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for SportType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_slug(s).ok_or_else(|| format!("unknown sport: {s}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchScore {
    pub match_id: String,
    pub series_id: String,
    pub match_title: String,
    pub status: MatchStatus,
    pub team1: TeamScore,
    pub team2: TeamScore,
    /// Which side is currently batting: 0 = none/unknown, 1 = team1, 2 = team2.
    /// Kept as `u8` (not an enum) to avoid churn in `render.rs` comparisons.
    pub batting_team: u8,
    pub crr: f32,
    pub rrr: Option<f32>,
    pub target: Option<u32>,
    pub runs_needed: Option<u32>,
    pub timestamp: u64, // unix seconds; `render.rs` stale check uses `> 15s`
    pub sport: SportType,
    pub soccer_clock: Option<String>,
    /// True for multi-day Tests (no RRR). Carried from the parser so the
    /// fetcher doesn't re-derive it with a fragile `title.contains("test")`.
    #[serde(default)]
    pub is_test: bool,
}

/// Named values for `MatchScore::batting_team` (kept `u8` for render compat).
pub const BATTING_NONE: u8 = 0;
pub const BATTING_TEAM1: u8 = 1;
pub const BATTING_TEAM2: u8 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchEventType {
    Wicket,
    Boundary,
    Win,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchEvent {
    pub event_type: MatchEventType,
    pub title: String,
    pub description: String,
    pub score: String,
    /// Typed sport so popup/flash routing never string-compares.
    /// Serializes as `"Cricket"`/`"Soccer"` (serde default); use
    /// `sport.as_str()` / `Display` for ESPN URL slugs.
    pub sport: SportType,
}
