use super::models::{
    MatchEvent, MatchEventType, MatchScore, MatchStatus, SportType, TeamScore, BATTING_NONE,
    BATTING_TEAM1, BATTING_TEAM2,
};

/// ESPN summary payloads carry a stub commentary entry under this key with no
/// ball data. It must be skipped when scanning for the latest ball, otherwise
/// the wicket/boundary detector fires on an empty stub.
const PLACEHOLDER_COMMENTARY_KEY: &str = "999999999999999";

/// ESPN numeric team id for India (cricket). Used alongside the
/// `displayName` contains-check so renamed/aliased India sides still match.
const ESPN_INDIA_TEAM_ID: &str = "6";

/// Word-boundary `\btest\b` check (case-insensitive) without a regex crate.
///
/// Returns true only when `test` appears as a standalone word, so
/// `Latest`, `Contest`, `Greatest`, `Testament` do not match.
fn title_has_test_word(title: &str) -> bool {
    let lower = title.to_lowercase();
    let bytes = lower.as_bytes();
    let needle = b"test";
    if bytes.len() < 4 {
        return false;
    }
    for i in 0..=bytes.len() - needle.len() {
        if &bytes[i..i + 4] != needle {
            continue;
        }
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let after_ok = i + 4 >= bytes.len() || !bytes[i + 4].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Robust Test-match detection.
///
/// Order: `generalClassCard == "Test"` first (authoritative), then
/// `limitedOvers` typed handling (`bool` / `u64` / `f64`), with the
/// `\btest\b` title word-match last as a fallback.
pub fn is_test_match(comp: &serde_json::Value, title: &str) -> bool {
    if comp
        .get("class")
        .and_then(|c| c.get("generalClassCard"))
        .and_then(|v| v.as_str())
        == Some("Test")
    {
        return true;
    }
    if let Some(lo) = comp.get("limitedOvers") {
        if let Some(b) = lo.as_bool() {
            // ESPN quirk: `limitedOvers: false` (bool) means unlimited = Test.
            return !b;
        }
        if let Some(n) = lo.as_u64() {
            if n > 0 {
                return false; // explicit overs cap => limited-overs, not a Test
            }
            // 0 falls through to the title check (treated like missing)
        } else if let Some(f) = lo.as_f64() {
            if f > 0.0 {
                return false;
            }
        }
    }
    title_has_test_word(title)
}

/// Total-overs cap for RRR maths. Handles ESPN's `f64` (`20.0`),
/// integer (`20`) and legacy `bool`/missing shapes (default 50.0).
fn limited_overs_value(comp: &serde_json::Value) -> f32 {
    if let Some(lo) = comp.get("limitedOvers") {
        if let Some(n) = lo.as_u64() {
            if n > 0 {
                return n as f32;
            }
        } else if let Some(f) = lo.as_f64() {
            if f > 0.0 {
                return f as f32;
            }
        }
    }
    50.0
}

/// Convert cricket `overs` (`14.2` = 14 overs + 2 balls) to a ball count.
///
/// The `+ 0.1` fudge is deliberate: ESPN overs arrive as `f32`, so `14.2`
/// decodes as `14.199999...`; without the fudge the fractional ball count
/// truncates one ball short. Verified by `test_calculate_crr_and_rrr`.
fn overs_to_balls(overs: f32) -> u32 {
    let completed = overs.floor() as u32;
    let extra = ((overs - completed as f32) * 10.0 + 0.1).floor() as u32;
    completed * 6 + extra
}

pub fn parse_all_live_indian_matches(
    value: &serde_json::Value,
) -> Vec<(String, String, String, String, String, String)> {
    let mut matches = Vec::new();
    if let Some(sports) = value.get("sports").and_then(|v| v.as_array()) {
        for sport in sports {
            if sport.get("slug").and_then(|v| v.as_str()) == Some("cricket") {
                if let Some(leagues) = sport.get("leagues").and_then(|v| v.as_array()) {
                    for league in leagues {
                        let series_id = league.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let league_name = league
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Cricket")
                            .to_string();
                        if let Some(events) = league.get("events").and_then(|v| v.as_array()) {
                            for event in events {
                                let match_id =
                                    event.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                let status =
                                    event.get("status").and_then(|v| v.as_str()).unwrap_or("");
                                let name = event
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Cricket Match");

                                if status == "in" || status == "pre" {
                                    if let Some(competitors) =
                                        event.get("competitors").and_then(|v| v.as_array())
                                    {
                                        let mut is_india_match = false;
                                        for comp in competitors {
                                            let id = comp
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let display_name = comp
                                                .get("displayName")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let lower_name = display_name.to_lowercase();
                                            if id == ESPN_INDIA_TEAM_ID
                                                || lower_name.contains("india")
                                                || lower_name == "ind"
                                            {
                                                is_india_match = true;
                                                break;
                                            }
                                        }
                                        if is_india_match {
                                            let start_time = event
                                                .get("date")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            matches.push((
                                                series_id.to_string(),
                                                match_id.to_string(),
                                                name.to_string(),
                                                status.to_string(),
                                                league_name.clone(),
                                                start_time,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    matches
}

pub fn parse_match_detail(
    value: &serde_json::Value,
    series_id: &str,
    match_id: &str,
) -> Option<MatchScore> {
    let header = value.get("header")?;
    let match_title_base = header.get("name")?.as_str()?;
    let match_desc = header.get("description")?.as_str()?;
    let match_title = format!("{} • {}", match_title_base, match_desc);

    let competitions = header.get("competitions")?.as_array()?;
    let comp = competitions.first()?;

    let status = comp.get("status")?;
    let state = status.get("type")?.get("state")?.as_str()?;
    let detail = status
        .get("type")
        .and_then(|t| t.get("detail"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();

    let mut status_enum = match state {
        "in" => MatchStatus::Live,
        "pre" => MatchStatus::Scheduled,
        "post" => MatchStatus::Completed,
        _ => MatchStatus::NoMatch,
    };

    if status_enum == MatchStatus::Live
        && !detail.is_empty()
        && (detail.contains("delay")
            || detail.contains("lunch")
            || detail.contains("tea")
            || detail.contains("stumps")
            || detail.contains("rain")
            || detail.contains("break"))
    {
        status_enum = MatchStatus::Break;
    }

    let competitors_arr = comp.get("competitors")?.as_array()?;
    if competitors_arr.len() < 2 {
        return None;
    }

    let team1 = parse_competitor(&competitors_arr[0]);
    let team2 = parse_competitor(&competitors_arr[1]);

    let batting_team = if team1.is_batting {
        BATTING_TEAM1
    } else if team2.is_batting {
        BATTING_TEAM2
    } else {
        BATTING_NONE
    };

    // CRR from the current innings only. During an innings break nobody is
    // batting: report 0.0 here and let the fetcher preserve the last good
    // value so the overlay never flashes `CRR: 0.00` mid-game.
    let crr = if batting_team == BATTING_TEAM1 {
        calculate_crr(team1.runs, team1.overs)
    } else if batting_team == BATTING_TEAM2 {
        calculate_crr(team2.runs, team2.overs)
    } else {
        0.0
    };

    // Find if there is a target
    let mut target = None;
    let mut runs_needed = None;
    let mut rrr = None;

    let is_test = is_test_match(comp, &match_title);

    let limited_overs = limited_overs_value(comp);

    // Check if team 1 is chasing
    if batting_team == BATTING_TEAM1 {
        if let Some(t) = get_target(&competitors_arr[0]) {
            target = Some(t);
            if team1.runs < t {
                let runs = t - team1.runs;
                runs_needed = Some(runs);
                if !is_test {
                    rrr = Some(calculate_rrr(runs, limited_overs, team1.overs));
                }
            } else {
                // Target reached or passed: nothing needed, no required rate.
                runs_needed = None;
                rrr = None;
            }
        }
    } else if batting_team == BATTING_TEAM2 {
        if let Some(t) = get_target(&competitors_arr[1]) {
            target = Some(t);
            if team2.runs < t {
                let runs = t - team2.runs;
                runs_needed = Some(runs);
                if !is_test {
                    rrr = Some(calculate_rrr(runs, limited_overs, team2.overs));
                }
            } else {
                runs_needed = None;
                rrr = None;
            }
        }
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Some(MatchScore {
        match_id: match_id.to_string(),
        series_id: series_id.to_string(),
        match_title,
        status: status_enum,
        team1,
        team2,
        batting_team,
        crr,
        rrr,
        target,
        runs_needed,
        timestamp,
        sport: SportType::Cricket,
        soccer_clock: None,
        is_test,
    })
}

fn parse_competitor(comp: &serde_json::Value) -> TeamScore {
    let team = comp.get("team").filter(|v| !v.is_null()).unwrap_or(comp);
    if team.is_null() {
        return TeamScore {
            id: String::new(),
            name: String::new(),
            abbreviation: String::new(),
            score: "Yet to bat".to_string(),
            runs: 0,
            wickets: 0,
            overs: 0.0,
            is_batting: false,
            is_winner: false,
        };
    }
    let id = team
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = team
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let abbreviation = team
        .get("abbreviation")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut score_str = comp
        .get("score")
        .and_then(|v| v.as_str())
        .unwrap_or("Yet to bat")
        .to_string();
    let mut runs = 0;
    let mut wickets = 0;
    let mut overs = 0.0;
    let mut is_batting = false;

    if let Some(linescores) = comp.get("linescores").and_then(|v| v.as_array()) {
        let active_linescore = linescores
            .iter()
            .find(|l| {
                l.get("isCurrent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                    || l.get("isCurrent")
                        .and_then(|v| v.as_u64())
                        .map(|n| n == 1)
                        .unwrap_or(false)
            })
            .or_else(|| linescores.last());

        if let Some(linescore) = active_linescore {
            runs = linescore.get("runs").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            wickets = linescore
                .get("wickets")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            overs = linescore
                .get("overs")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            is_batting = linescore
                .get("isBatting")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            // Multi-innings display (Tests): join every innings' score line
            // (`"462 & 193"`) while keeping runs/wickets/overs from the
            // current innings above. Single-innings feeds are unaffected.
            let joined: Vec<&str> = linescores
                .iter()
                .filter_map(|l| l.get("score").and_then(|v| v.as_str()))
                .filter(|s| !s.is_empty())
                .collect();
            if !joined.is_empty() {
                score_str = joined.join(" & ");
            } else if let Some(s) = linescore.get("score").and_then(|v| v.as_str()) {
                score_str = s.to_string();
            }
        }
    }

    let is_winner = comp
        .get("winner")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    TeamScore {
        id,
        name,
        abbreviation,
        score: score_str,
        runs,
        wickets,
        overs,
        is_batting,
        is_winner,
    }
}

fn get_target(comp: &serde_json::Value) -> Option<u32> {
    let linescores = comp.get("linescores")?.as_array()?;
    let active_linescore = linescores
        .iter()
        .find(|l| {
            l.get("isCurrent")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || l.get("isCurrent")
                    .and_then(|v| v.as_u64())
                    .map(|n| n == 1)
                    .unwrap_or(false)
        })
        .or_else(|| linescores.last())?;

    let target = active_linescore
        .get("target")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if target > 0 {
        Some(target)
    } else {
        None
    }
}

fn calculate_crr(runs: u32, overs_float: f32) -> f32 {
    let total_balls = overs_to_balls(overs_float);
    if total_balls == 0 {
        0.0
    } else {
        (runs as f32 / total_balls as f32) * 6.0
    }
}

fn calculate_rrr(runs_needed: u32, total_overs: f32, current_overs: f32) -> f32 {
    let current_balls = overs_to_balls(current_overs);
    let total_balls = overs_to_balls(total_overs);
    if total_balls <= current_balls {
        0.0
    } else {
        let balls_remaining = total_balls - current_balls;
        (runs_needed as f32 / balls_remaining as f32) * 6.0
    }
}

fn parse_batsman_from_text(text: &str) -> Option<String> {
    let clean = text.trim();
    if clean.is_empty() {
        return None;
    }

    if let Some(to_idx) = clean.find(" to ") {
        let after_to = &clean[to_idx + 4..];
        if let Some(out_idx) = after_to.find(", OUT").or_else(|| after_to.find(" OUT")) {
            let name = after_to[..out_idx].trim();
            if !name.is_empty() && name.len() < 40 {
                return Some(name.to_string());
            }
        }
    }

    let work_text =
        if clean.starts_with("OUT!") || clean.starts_with("OUT,") || clean.starts_with("OUT ") {
            clean
                .trim_start_matches("OUT!")
                .trim_start_matches("OUT,")
                .trim_start_matches("OUT")
                .trim()
        } else {
            clean
        };

    let keywords = [
        " c ",
        " lbw",
        " b ",
        " run out",
        " st ",
        " hit wicket",
        " retired",
    ];
    let mut earliest_idx = None;

    for kw in &keywords {
        if let Some(idx) = work_text.find(kw) {
            if earliest_idx.map_or(true, |prev| idx < prev) {
                earliest_idx = Some(idx);
            }
        }
    }

    if let Some(idx) = earliest_idx {
        let candidate = work_text[..idx].trim();
        if !candidate.is_empty() && candidate.len() < 40 {
            return Some(candidate.to_string());
        }
    }

    None
}

fn extract_batsman_name(
    ball_data: &serde_json::Value,
    dismissal: &Option<&serde_json::Value>,
) -> Option<String> {
    if let Some(d) = dismissal {
        if let Some(b) = d.get("batsman") {
            if let Some(a) = b.get("athlete") {
                if let Some(name) = a
                    .get("displayName")
                    .or_else(|| a.get("name"))
                    .or_else(|| a.get("shortName"))
                    .and_then(|v| v.as_str())
                {
                    if !name.is_empty() && name != "Batsman" {
                        return Some(name.to_string());
                    }
                }
            }
            if let Some(name) = b
                .get("displayName")
                .or_else(|| b.get("name"))
                .or_else(|| b.get("shortName"))
                .and_then(|v| v.as_str())
            {
                if !name.is_empty() && name != "Batsman" {
                    return Some(name.to_string());
                }
            }
        }

        if let Some(a) = d.get("athlete") {
            if let Some(name) = a
                .get("displayName")
                .or_else(|| a.get("name"))
                .or_else(|| a.get("shortName"))
                .and_then(|v| v.as_str())
            {
                if !name.is_empty() && name != "Batsman" {
                    return Some(name.to_string());
                }
            }
        }
    }

    if let Some(b) = ball_data.get("batsman") {
        if let Some(a) = b.get("athlete") {
            if let Some(name) = a
                .get("displayName")
                .or_else(|| a.get("name"))
                .and_then(|v| v.as_str())
            {
                if !name.is_empty() && name != "Batsman" {
                    return Some(name.to_string());
                }
            }
        }
        if let Some(name) = b
            .get("displayName")
            .or_else(|| b.get("name"))
            .and_then(|v| v.as_str())
        {
            if !name.is_empty() && name != "Batsman" {
                return Some(name.to_string());
            }
        }
    }

    if let Some(batsmen) = ball_data.get("batsmen").and_then(|v| v.as_array()) {
        for b in batsmen {
            let name = b
                .get("athlete")
                .and_then(|a| a.get("displayName").or_else(|| a.get("name")))
                .or_else(|| b.get("displayName"))
                .or_else(|| b.get("name"))
                .and_then(|v| v.as_str());
            if let Some(n) = name {
                if !n.is_empty() && n != "Batsman" {
                    return Some(n.to_string());
                }
            }
        }
    }

    let text_sources = [
        dismissal.and_then(|d| d.get("text").and_then(|v| v.as_str())),
        dismissal.and_then(|d| d.get("shortText").and_then(|v| v.as_str())),
        ball_data.get("shortText").and_then(|v| v.as_str()),
        ball_data.get("text").and_then(|v| v.as_str()),
    ];

    for src in text_sources.into_iter().flatten() {
        if let Some(name) = parse_batsman_from_text(src) {
            if !name.is_empty() && name != "Batsman" {
                return Some(name);
            }
        }
    }

    // No sentinel: callers fall back to `shortText` / empty text.
    None
}

fn extract_score_str(ball_data: &serde_json::Value) -> String {
    let team_abbr = ball_data
        .get("team")
        .and_then(|t| t.get("abbreviation").or_else(|| t.get("displayName")))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let home_score = ball_data
        .get("homeScore")
        .or_else(|| ball_data.get("currentScore"))
        .or_else(|| ball_data.get("score"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let over_num = ball_data
        .get("over")
        .and_then(|o| {
            o.get("overs")
                .or_else(|| o.get("displayValue"))
                .and_then(|v| {
                    v.as_f64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
        })
        .or_else(|| ball_data.get("overs").and_then(|v| v.as_f64()))
        .unwrap_or(0.0);

    match (team_abbr.is_empty(), home_score.is_empty()) {
        (false, false) => format!("{} {} ({} ov)", team_abbr, home_score, over_num),
        (true, false) => format!("{} ({} ov)", home_score, over_num),
        (false, true) => format!("{} ({} ov)", team_abbr, over_num),
        (true, true) => {
            if over_num > 0.0 {
                format!("{} ov", over_num)
            } else {
                String::new()
            }
        }
    }
}

/// True when the delivery was a wide — boundary runs off a wide must not
/// notify as a batter boundary.
///
/// ESPN exposes wides inconsistently: some payloads carry a bool-ish
/// `wide`/`isWide` field, others only mention it in text. Check the bool
/// fields first, then fall back to a case-insensitive `wide` word scan of
/// `shortText`/`text`. No-ball boundaries *do* count to the batter, so
/// `noBall` shapes are deliberately not excluded here.
fn is_wide_delivery(ball_data: &serde_json::Value) -> bool {
    for key in ["wide", "isWide", "wideBall", "isWideBall"] {
        if ball_data.get(key).and_then(|v| v.as_bool()) == Some(true) {
            return true;
        }
    }
    for key in ["shortText", "text"] {
        if let Some(s) = ball_data.get(key).and_then(|v| v.as_str()) {
            if s.split(|c: char| !c.is_ascii_alphabetic())
                .any(|w| w.eq_ignore_ascii_case("wide") || w.eq_ignore_ascii_case("wides"))
            {
                return true;
            }
        }
    }
    false
}

pub fn parse_latest_event(
    value: &serde_json::Value,
    last_ball_id: &mut Option<String>,
) -> Option<MatchEvent> {
    let header = value.get("header")?;
    let competitions = header.get("competitions")?.as_array()?;
    let comp = competitions.first()?;
    let commentaries = comp.get("commentaries")?.as_object()?;

    let mut latest_key: Option<u64> = None;
    for key_str in commentaries.keys() {
        if key_str == PLACEHOLDER_COMMENTARY_KEY {
            continue;
        }
        if let Ok(key_num) = key_str.parse::<u64>() {
            if latest_key.is_none() || Some(key_num) > latest_key {
                latest_key = Some(key_num);
            }
        }
    }

    let latest_key_str = latest_key?.to_string();
    let ball_data = commentaries.get(&latest_key_str)?;

    let is_new = match last_ball_id {
        Some(prev) => prev != &latest_key_str,
        None => {
            *last_ball_id = Some(latest_key_str.clone());
            false
        }
    };

    if !is_new {
        return None;
    }

    *last_ball_id = Some(latest_key_str);

    let score_str = extract_score_str(ball_data);

    let dismissal = ball_data.get("dismissal");
    let is_dismissal = dismissal
        .and_then(|d| d.get("dismissal").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if is_dismissal {
        // Structured name first, `shortText` fallback, sentinel last resort
        // so the popup never shows an empty title line.
        let batsman_name = extract_batsman_name(ball_data, &dismissal)
            .or_else(|| {
                ball_data
                    .get("shortText")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "Batsman".to_string());
        let dismissal_text = dismissal
            .and_then(|d| d.get("text").and_then(|v| v.as_str()))
            .unwrap_or("");
        let short_desc = ball_data
            .get("shortText")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let desc = if !dismissal_text.is_empty() {
            format!("{}: {} ({})", batsman_name, dismissal_text, short_desc)
        } else if !short_desc.is_empty() {
            format!("{}: {}", batsman_name, short_desc)
        } else {
            batsman_name.clone()
        };

        return Some(MatchEvent {
            event_type: MatchEventType::Wicket,
            title: "Wicket!".to_string(),
            description: desc,
            score: score_str,
            sport: SportType::Cricket,
        });
    }

    let is_boundary = ball_data
        .get("boundary")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let score_value = ball_data
        .get("scoreValue")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // A bare `scoreValue: 4/6` also fires on wides (extra runs), so a
    // wide never notifies even when flagged `boundary: true`.
    let is_wide = is_wide_delivery(ball_data);
    if (is_boundary || score_value == 4 || score_value == 6) && !is_wide {
        let batsman_name = extract_batsman_name(ball_data, &None);
        let short_desc = ball_data
            .get("shortText")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let desc = match batsman_name {
            Some(name) => format!("{}: {}", name, short_desc),
            None => short_desc.to_string(),
        };

        return Some(MatchEvent {
            event_type: MatchEventType::Boundary,
            title: if score_value == 6 { "SIX!" } else { "FOUR!" }.to_string(),
            description: desc,
            score: score_str,
            sport: SportType::Cricket,
        });
    }

    None
}

pub fn parse_soccer_matches(
    value: &serde_json::Value,
) -> Vec<(String, String, String, String, String, String)> {
    let mut matches = Vec::new();

    let parse_event = |event: &serde_json::Value,
                       series_id: &str,
                       league_name: &str|
     -> Option<(String, String, String, String, String, String)> {
        let match_id = event.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if match_id.is_empty() {
            return None;
        }

        // Support string status ("in", "pre"), status object ({ "type": { "state": "in" } }),
        // and fullStatus object ({ "type": { "state": "in" } }).
        let status_raw = if let Some(s) = event.get("status").and_then(|v| v.as_str()) {
            s.to_string()
        } else if let Some(s) = event
            .get("status")
            .and_then(|st| st.get("type"))
            .and_then(|t| t.get("state"))
            .and_then(|v| v.as_str())
        {
            s.to_string()
        } else if let Some(s) = event
            .get("fullStatus")
            .and_then(|st| st.get("type"))
            .and_then(|t| t.get("state"))
            .and_then(|v| v.as_str())
        {
            s.to_string()
        } else {
            String::new()
        };

        let status_lower = status_raw.trim().to_ascii_lowercase();
        let is_live = status_lower == "in"
            || status_lower.contains("live")
            || status_lower.contains("progress");
        let is_pre = status_lower == "pre" || status_lower.contains("sched");
        if !is_live && !is_pre {
            return None;
        }
        let canonical_status = if is_live { "in" } else { "pre" };

        let name = event
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Football Match");
        let mut match_name = name.to_string();

        let competitors = event
            .get("competitors")
            .and_then(|v| v.as_array())
            .or_else(|| {
                event
                    .get("competitions")
                    .and_then(|c| c.as_array())
                    .and_then(|a| a.first())
                    .and_then(|c| c.get("competitors"))
                    .and_then(|v| v.as_array())
            });

        if let Some(comps) = competitors {
            if comps.len() >= 2 {
                fn extract_team_name(c: &serde_json::Value) -> &str {
                    c.get("displayName")
                        .or_else(|| c.get("name"))
                        .or_else(|| c.get("team").and_then(|t| t.get("displayName")))
                        .or_else(|| c.get("team").and_then(|t| t.get("name")))
                        .or_else(|| c.get("shortDisplayName"))
                        .or_else(|| c.get("abbreviation"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                }
                let team1 = extract_team_name(&comps[0]);
                let team2 = extract_team_name(&comps[1]);
                if !team1.is_empty() && !team2.is_empty() {
                    match_name = format!("{} vs {}", team1, team2);
                }
            }
        }

        let start_time = event
            .get("date")
            .or_else(|| {
                event
                    .get("competitions")
                    .and_then(|c| c.as_array())
                    .and_then(|a| a.first())
                    .and_then(|c| c.get("date"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Some((
            series_id.to_string(),
            match_id.to_string(),
            match_name,
            canonical_status.to_string(),
            league_name.to_string(),
            start_time,
        ))
    };

    if let Some(sports) = value.get("sports").and_then(|v| v.as_array()) {
        for sport in sports {
            let slug = sport.get("slug").and_then(|v| v.as_str()).unwrap_or("");
            if slug.eq_ignore_ascii_case("soccer") || slug.eq_ignore_ascii_case("football") {
                if let Some(leagues) = sport.get("leagues").and_then(|v| v.as_array()) {
                    for league in leagues {
                        let series_slug = league.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                        let series_id = if series_slug.is_empty() {
                            league.get("id").and_then(|v| v.as_str()).unwrap_or("")
                        } else {
                            series_slug
                        };
                        let league_name = league
                            .get("name")
                            .or_else(|| league.get("shortName"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Football");

                        if let Some(events) = league.get("events").and_then(|v| v.as_array()) {
                            for event in events {
                                if let Some(m) = parse_event(event, series_id, league_name) {
                                    matches.push(m);
                                }
                            }
                        }
                    }
                }
            }
        }
    } else if let Some(events) = value.get("events").and_then(|v| v.as_array()) {
        let series_id = value
            .get("leagues")
            .and_then(|l| l.as_array())
            .and_then(|a| a.first())
            .and_then(|l| l.get("slug").or_else(|| l.get("id")))
            .and_then(|v| v.as_str())
            .unwrap_or("soccer");
        let league_name = value
            .get("leagues")
            .and_then(|l| l.as_array())
            .and_then(|a| a.first())
            .and_then(|l| l.get("name").or_else(|| l.get("shortName")))
            .and_then(|v| v.as_str())
            .unwrap_or("Football");

        for event in events {
            if let Some(m) = parse_event(event, series_id, league_name) {
                matches.push(m);
            }
        }
    }

    matches
}

pub fn parse_soccer_match_detail(
    value: &serde_json::Value,
    series_id: &str,
    match_id: &str,
) -> Option<MatchScore> {
    let header = value.get("header")?;
    let match_title = header
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Soccer Match")
        .to_string();

    let competitions = header.get("competitions")?.as_array()?;
    let comp = competitions.first()?;

    let status = comp.get("status")?;
    let state = status
        .get("type")
        .and_then(|t| t.get("state"))
        .and_then(|s| s.as_str())
        .unwrap_or("pre");

    // "detail" is optional – absent before kickoff
    let detail = status
        .get("type")
        .and_then(|t| t.get("detail"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let status_enum = match state {
        "in" => MatchStatus::Live,
        "pre" => MatchStatus::Scheduled,
        "post" => MatchStatus::Completed,
        _ => MatchStatus::NoMatch,
    };

    let competitors_arr = comp
        .get("competitors")
        .and_then(|v| v.as_array())
        .filter(|a| a.len() >= 2)?;

    let team1 = parse_soccer_competitor(&competitors_arr[0]);
    let team2 = parse_soccer_competitor(&competitors_arr[1]);

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let clock = if detail.is_empty() {
        None
    } else {
        Some(detail)
    };

    Some(MatchScore {
        match_id: match_id.to_string(),
        series_id: series_id.to_string(),
        match_title,
        status: status_enum,
        team1,
        team2,
        batting_team: BATTING_NONE,
        crr: 0.0,
        rrr: None,
        target: None,
        runs_needed: None,
        timestamp,
        sport: SportType::Soccer,
        soccer_clock: clock,
        is_test: false,
    })
}

fn parse_soccer_competitor(comp: &serde_json::Value) -> TeamScore {
    let team = comp.get("team").unwrap_or(comp);
    let id = team
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = team
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let abbreviation = team
        .get("abbreviation")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let score_val = comp.get("score");
    let score_str = if let Some(s) = score_val.and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(n) = score_val.and_then(|v| v.as_u64()) {
        n.to_string()
    } else {
        "0".to_string()
    };

    let runs = score_str.parse::<u32>().unwrap_or(0);
    let is_winner = comp
        .get("winner")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    TeamScore {
        id,
        name,
        abbreviation,
        score: score_str,
        runs,
        wickets: 0,
        overs: 0.0,
        is_batting: false,
        is_winner,
    }
}

/// `"{home}-{away}"` scoreline for soccer popups, read off the detail
/// payload's competitor scores. Falls back to `""` when absent.
fn soccer_score_line(value: &serde_json::Value) -> String {
    let get = |i: usize| {
        value
            .get("header")?
            .get("competitions")?
            .as_array()?
            .first()?
            .get("competitors")?
            .as_array()?
            .get(i)?
            .get("score")
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            })
    };
    match (get(0), get(1)) {
        (Some(h), Some(a)) => format!("{}-{}", h, a),
        (Some(h), None) => h,
        (None, Some(a)) => a,
        (None, None) => String::new(),
    }
}

pub fn parse_soccer_latest_event(
    value: &serde_json::Value,
    last_event_id: &mut Option<String>,
) -> Option<MatchEvent> {
    let key_events = value.get("keyEvents")?.as_array()?;
    let latest_event = key_events.last()?;

    let event_id = latest_event.get("id")?.as_str()?;

    // First poll seeds dedup state and suppresses the stale event, mirroring
    // the cricket path — otherwise tracking a match mid-game replays the
    // last GOAL/RED as if it just happened.
    let is_new = match last_event_id {
        Some(prev) => prev != event_id,
        None => {
            *last_event_id = Some(event_id.to_string());
            false
        }
    };

    if !is_new {
        return None;
    }

    *last_event_id = Some(event_id.to_string());

    let type_obj = latest_event.get("type")?;
    let event_type_slug = type_obj.get("type")?.as_str()?.to_lowercase();

    let short_text = latest_event
        .get("shortText")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let clock_val = latest_event
        .get("clock")
        .and_then(|c| c.get("displayValue").and_then(|v| v.as_str()))
        .unwrap_or("");

    let mut event_type = None;
    let mut title = String::new();

    if event_type_slug.contains("goal")
        || latest_event
            .get("scoringPlay")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    {
        event_type = Some(MatchEventType::Boundary);
        title = if event_type_slug.contains("own") {
            "OWN GOAL!".to_string()
        } else if event_type_slug.contains("penalty") {
            "PENALTY GOAL!".to_string()
        } else {
            "GOAL!".to_string()
        };
    } else if event_type_slug.contains("red") {
        event_type = Some(MatchEventType::Wicket);
        title = "RED CARD!".to_string();
    }

    event_type.map(|et| MatchEvent {
        event_type: et,
        title,
        description: format!("{} ({})", short_text, clock_val),
        score: soccer_score_line(value),
        sport: SportType::Soccer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_crr_and_rrr() {
        // 145 runs in 14.2 overs = 86 balls -> CRR = (145/86)*6 = 10.116279
        let crr = calculate_crr(145, 14.2);
        assert!((crr - 10.116).abs() < 0.01);

        // 0 balls -> 0.0
        assert_eq!(calculate_crr(0, 0.0), 0.0);

        // 6 runs in 1.0 over = 6 balls -> CRR = 6.0
        assert_eq!(calculate_crr(6, 1.0), 6.0);

        // Chasing: 36 runs needed in 3.0 overs (18 balls) out of 20 overs total, currently at 17.0 overs
        // RRR = (36 / 18) * 6 = 12.0
        let rrr = calculate_rrr(36, 20.0, 17.0);
        assert!((rrr - 12.0).abs() < 0.01);

        // 10 runs needed in 1.4 overs (10 balls)
        let rrr_balls = calculate_rrr(10, 20.0, 18.2);
        assert!((rrr_balls - 6.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_batsman_from_text() {
        assert_eq!(
            parse_batsman_from_text("V Kohli c Smith b Starc"),
            Some("V Kohli".to_string())
        );
        assert_eq!(
            parse_batsman_from_text("OUT! R Sharma lbw b Cummins"),
            Some("R Sharma".to_string())
        );
        assert_eq!(
            parse_batsman_from_text("Starc to S Gill, OUT, caught by Smith"),
            Some("S Gill".to_string())
        );
        assert_eq!(
            parse_batsman_from_text("KL Rahul b Bumrah"),
            Some("KL Rahul".to_string())
        );
        assert_eq!(
            parse_batsman_from_text("R Pant run out (Jadeja)"),
            Some("R Pant".to_string())
        );
    }

    #[test]
    fn test_extract_score_str() {
        let json = serde_json::json!({
            "team": { "abbreviation": "IND" },
            "homeScore": "145/3",
            "over": { "overs": 14.2 }
        });
        assert_eq!(extract_score_str(&json), "IND 145/3 (14.2 ov)");
    }

    #[test]
    fn test_parse_match_detail_test_match() {
        let json = serde_json::json!({
            "header": {
                "name": "Sri Lanka v India",
                "description": "1st Test, India tour of Sri Lanka at Galle, Aug 15-19 2026",
                "competitions": [{
                    "status": {
                        "type": {
                            "state": "in",
                            "detail": "Live"
                        }
                    },
                    "competitors": [
                        {
                            "team": { "id": "8", "displayName": "Sri Lanka", "abbreviation": "SL" },
                            "score": "84/4",
                            "linescores": [{
                                "isCurrent": true,
                                "runs": 84,
                                "wickets": 4,
                                "overs": 34.0,
                                "isBatting": true,
                                "target": 372
                            }]
                        },
                        {
                            "team": { "id": "6", "displayName": "India", "abbreviation": "IND" },
                            "score": "462 & 193",
                            "linescores": [{
                                "isCurrent": false,
                                "runs": 193,
                                "wickets": 10,
                                "overs": 48.5,
                                "isBatting": false
                            }]
                        }
                    ]
                }]
            }
        });

        let score = parse_match_detail(&json, "24567", "1544001").expect("should parse");
        assert_eq!(score.match_id, "1544001");
        assert_eq!(score.series_id, "24567");
        assert_eq!(score.team1.abbreviation, "SL");
        assert_eq!(score.team1.runs, 84);
        assert_eq!(score.team1.wickets, 4);
        assert_eq!(score.batting_team, BATTING_TEAM1);
        assert_eq!(score.target, Some(372));
        assert_eq!(score.runs_needed, Some(288));
        assert_eq!(score.rrr, None); // Test match should not have RRR
        assert_eq!(score.sport, SportType::Cricket);
        assert!(score.is_test);
        // Single-linescore fallback keeps the feed's own aggregate string.
        assert_eq!(score.team2.score, "462 & 193");
        assert_eq!(score.team2.runs, 193);
    }

    #[test]
    fn test_parse_match_detail_test_second_innings_aggregate() {
        // Two linescores with per-innings `score` fields join with " & ",
        // while runs/overs stay on the current innings.
        let json = serde_json::json!({
            "header": {
                "name": "India v Australia",
                "description": "4th Test at Ahmedabad",
                "competitions": [{
                    "class": { "generalClassCard": "Test" },
                    "status": {
                        "type": {
                            "state": "in",
                            "detail": "Live"
                        }
                    },
                    "competitors": [
                        {
                            "team": { "id": "6", "displayName": "India", "abbreviation": "IND" },
                            "score": "320 & 45/1",
                            "linescores": [
                                {
                                    "score": "320",
                                    "isCurrent": false,
                                    "runs": 320,
                                    "wickets": 10,
                                    "overs": 95.2,
                                    "isBatting": false
                                },
                                {
                                    "score": "45/1",
                                    "isCurrent": true,
                                    "runs": 45,
                                    "wickets": 1,
                                    "overs": 12.0,
                                    "isBatting": true
                                }
                            ]
                        },
                        {
                            "team": { "id": "2", "displayName": "Australia", "abbreviation": "AUS" },
                            "score": "410",
                            "linescores": [{
                                "score": "410",
                                "isCurrent": false,
                                "runs": 410,
                                "wickets": 10,
                                "overs": 120.0,
                                "isBatting": false
                            }]
                        }
                    ]
                }]
            }
        });

        let score = parse_match_detail(&json, "24567", "1544002").expect("should parse");
        assert!(score.is_test);
        assert_eq!(score.team1.score, "320 & 45/1");
        assert_eq!(score.team1.runs, 45);
        assert_eq!(score.team1.wickets, 1);
        assert_eq!(score.batting_team, BATTING_TEAM1);
        assert_eq!(score.rrr, None);
    }

    #[test]
    fn test_is_test_match_classification() {
        let class_test = serde_json::json!({ "class": { "generalClassCard": "Test" } });
        assert!(is_test_match(&class_test, "India v Australia"));

        // `limitedOvers: false` (bool) means unlimited => Test.
        let lo_false = serde_json::json!({ "limitedOvers": false });
        assert!(is_test_match(&lo_false, "India v Australia"));
        // `limitedOvers: true` (bool) means limited => not a Test.
        let lo_true = serde_json::json!({ "limitedOvers": true });
        assert!(!is_test_match(&lo_true, "India v Australia"));

        // Numeric caps (f64 and u64) mean limited-overs => not a Test.
        let lo_f64 = serde_json::json!({ "limitedOvers": 20.0 });
        assert!(!is_test_match(&lo_f64, "India v Australia"));
        let lo_u64 = serde_json::json!({ "limitedOvers": 50 });
        assert!(!is_test_match(&lo_u64, "India v Australia"));

        // Title fallback needs a whole word: "Contest"/"Latest" must not match.
        let empty = serde_json::json!({});
        assert!(is_test_match(&empty, "1st Test at Galle"));
        assert!(!is_test_match(&empty, "Latest contest: greatest hits"));
        assert!(!is_test_match(&empty, "3rd T20I at Hyderabad"));
    }

    #[test]
    fn test_parse_match_detail_missing_detail_state() {
        // Pre-kickoff shape without `detail` must still parse (not wipe out).
        let json = serde_json::json!({
            "header": {
                "name": "India v Australia",
                "description": "3rd T20I at Hyderabad",
                "competitions": [{
                    "limitedOvers": 20.0,
                    "status": { "type": { "state": "pre" } },
                    "competitors": [
                        {
                            "team": { "id": "6", "displayName": "India", "abbreviation": "IND" },
                            "score": "Yet to bat"
                        },
                        {
                            "team": { "id": "2", "displayName": "Australia", "abbreviation": "AUS" },
                            "score": "Yet to bat"
                        }
                    ]
                }]
            }
        });

        let score = parse_match_detail(&json, "14135", "1413511").expect("should parse");
        assert_eq!(score.status, MatchStatus::Scheduled);
        assert!(!score.is_test);
    }

    #[test]
    fn test_parse_match_detail_target_reached_clears_need() {
        // Scores level with the target: no runs needed, no required rate.
        let json = serde_json::json!({
            "header": {
                "name": "India v Australia",
                "description": "3rd T20I at Hyderabad",
                "competitions": [{
                    "limitedOvers": 20,
                    "status": {
                        "type": {
                            "state": "in",
                            "detail": "Live"
                        }
                    },
                    "competitors": [
                        {
                            "team": { "id": "6", "displayName": "India", "abbreviation": "IND" },
                            "score": "187/4",
                            "linescores": [{
                                "score": "187/4",
                                "isCurrent": true,
                                "runs": 187,
                                "wickets": 4,
                                "overs": 19.0,
                                "isBatting": true,
                                "target": 187
                            }]
                        },
                        {
                            "team": { "id": "2", "displayName": "Australia", "abbreviation": "AUS" },
                            "score": "186/7",
                            "linescores": [{
                                "score": "186/7",
                                "isCurrent": false,
                                "runs": 186,
                                "wickets": 7,
                                "overs": 20.0,
                                "isBatting": false
                            }]
                        }
                    ]
                }]
            }
        });

        let score = parse_match_detail(&json, "14135", "1413511").expect("should parse");
        assert!(!score.is_test);
        assert_eq!(score.target, Some(187));
        assert_eq!(score.runs_needed, None);
        assert_eq!(score.rrr, None);
    }

    #[test]
    fn test_parse_boundary_wide_suppressed() {
        // `scoreValue: 4` off a wide must not notify as a batter boundary.
        let mut last_ball_id: Option<String> = None;
        let json = serde_json::json!({
            "header": {
                "competitions": [{
                    "commentaries": {
                        "555": {
                            "shortText": "14.5 Starc to Sharma, wide, FOUR!",
                            "homeScore": "155/3",
                            "scoreValue": 4,
                            "over": { "overs": 14.5 },
                            "team": { "abbreviation": "IND" }
                        }
                    }
                }]
            }
        });
        assert!(parse_latest_event(&json, &mut last_ball_id).is_none());

        // Same shape with the explicit boundary flag and no wide marker emits.
        let json2 = serde_json::json!({
            "header": {
                "competitions": [{
                    "commentaries": {
                        "555": {
                            "shortText": "14.5 Starc to Sharma, wide, FOUR!",
                            "homeScore": "155/3",
                            "scoreValue": 4,
                            "over": { "overs": 14.5 },
                            "team": { "abbreviation": "IND" }
                        },
                        "556": {
                            "shortText": "14.6 Starc to Kohli, FOUR, through covers!",
                            "homeScore": "159/3",
                            "scoreValue": 4,
                            "boundary": true,
                            "over": { "overs": 14.6 },
                            "team": { "abbreviation": "IND" },
                            "batsman": {
                                "athlete": { "displayName": "Virat Kohli" }
                            }
                        }
                    }
                }]
            }
        });
        let event = parse_latest_event(&json2, &mut last_ball_id).expect("should parse four");
        assert_eq!(event.title, "FOUR!");
        assert!(event.description.contains("Virat Kohli"));
    }

    #[test]
    fn test_parse_match_detail_t20_chase() {
        let json = serde_json::json!({
            "header": {
                "name": "India v Australia",
                "description": "3rd T20I, Australia tour of India at Hyderabad, Sep 25 2026",
                "competitions": [{
                    "limitedOvers": 20.0,
                    "status": {
                        "type": {
                            "state": "in",
                            "detail": "Live"
                        }
                    },
                    "competitors": [
                        {
                            "team": { "id": "6", "displayName": "India", "abbreviation": "IND" },
                            "score": "152/2",
                            "linescores": [{
                                "isCurrent": true,
                                "runs": 152,
                                "wickets": 2,
                                "overs": 15.0,
                                "isBatting": true,
                                "target": 187
                            }]
                        },
                        {
                            "team": { "id": "2", "displayName": "Australia", "abbreviation": "AUS" },
                            "score": "186/7",
                            "linescores": [{
                                "isCurrent": false,
                                "runs": 186,
                                "wickets": 7,
                                "overs": 20.0,
                                "isBatting": false
                            }]
                        }
                    ]
                }]
            }
        });

        let score = parse_match_detail(&json, "14135", "1413511").expect("should parse");
        assert_eq!(score.match_id, "1413511");
        assert_eq!(score.team1.abbreviation, "IND");
        assert_eq!(score.team1.runs, 152);
        assert_eq!(score.team1.overs, 15.0);
        assert_eq!(score.batting_team, BATTING_TEAM1);
        assert_eq!(score.target, Some(187));
        assert_eq!(score.runs_needed, Some(35));
        // CRR: 152 runs in 15.0 overs (90 balls) = 10.1333
        assert!((score.crr - 10.133).abs() < 0.01);
        // RRR: 35 runs needed in 5.0 overs (30 balls) = 7.0
        assert_eq!(score.rrr, Some(7.0));
    }

    #[test]
    fn test_parse_cricket_wicket_event() {
        let mut last_ball_id: Option<String> = None;
        let json = serde_json::json!({
            "header": {
                "competitions": [{
                    "commentaries": {
                        "123456789": {
                            "shortText": "14.2 Starc to Kohli, OUT",
                            "homeScore": "145/3",
                            "over": { "overs": 14.2 },
                            "team": { "abbreviation": "IND" },
                            "dismissal": {
                                "dismissal": true,
                                "text": "c Smith b Starc",
                                "batsman": {
                                    "athlete": {
                                        "displayName": "Virat Kohli"
                                    }
                                }
                            }
                        }
                    }
                }]
            }
        });

        // First poll seeds dedup state and suppresses stale event by design.
        assert!(parse_latest_event(&json, &mut last_ball_id).is_none());
        assert_eq!(last_ball_id, Some("123456789".to_string()));

        // New ball emits.
        let json2 = serde_json::json!({
            "header": {
                "competitions": [{
                    "commentaries": {
                        "123456789": {
                            "shortText": "14.2 Starc to Kohli, OUT",
                            "homeScore": "145/3",
                            "over": { "overs": 14.2 },
                            "team": { "abbreviation": "IND" },
                            "dismissal": {
                                "dismissal": true,
                                "text": "c Smith b Starc",
                                "batsman": {
                                    "athlete": {
                                        "displayName": "Virat Kohli"
                                    }
                                }
                            }
                        },
                        "123456790": {
                            "shortText": "14.3 Starc to Sharma, OUT",
                            "homeScore": "145/4",
                            "over": { "overs": 14.3 },
                            "team": { "abbreviation": "IND" },
                            "dismissal": {
                                "dismissal": true,
                                "text": "b Starc",
                                "batsman": {
                                    "athlete": {
                                        "displayName": "Rohit Sharma"
                                    }
                                }
                            }
                        }
                    }
                }]
            }
        });
        let event = parse_latest_event(&json2, &mut last_ball_id).expect("should parse event");
        assert_eq!(event.event_type, MatchEventType::Wicket);
        assert_eq!(event.title, "Wicket!");
        assert!(event.description.contains("Rohit Sharma"));
        assert_eq!(last_ball_id, Some("123456790".to_string()));

        // Repeated fetch with same ball_id should return None
        let duplicate = parse_latest_event(&json2, &mut last_ball_id);
        assert!(duplicate.is_none());
    }

    #[test]
    fn test_parse_cricket_boundary_event() {
        let mut last_ball_id: Option<String> = None;
        let json = serde_json::json!({
            "header": {
                "competitions": [{
                    "commentaries": {
                        "987654321": {
                            "shortText": "14.3 Starc to Sharma, SIX, over long-on!",
                            "homeScore": "151/3",
                            "scoreValue": 6,
                            "boundary": true,
                            "over": { "overs": 14.3 },
                            "team": { "abbreviation": "IND" },
                            "batsman": {
                                "athlete": {
                                    "displayName": "Rohit Sharma"
                                }
                            }
                        }
                    }
                }]
            }
        });

        // First poll seeds dedup state and suppresses stale event by design.
        assert!(parse_latest_event(&json, &mut last_ball_id).is_none());

        let json2 = serde_json::json!({
            "header": {
                "competitions": [{
                    "commentaries": {
                        "987654321": {
                            "shortText": "14.3 Starc to Sharma, SIX, over long-on!",
                            "homeScore": "151/3",
                            "scoreValue": 6,
                            "boundary": true,
                            "over": { "overs": 14.3 },
                            "team": { "abbreviation": "IND" },
                            "batsman": {
                                "athlete": {
                                    "displayName": "Rohit Sharma"
                                }
                            }
                        },
                        "987654322": {
                            "shortText": "14.4 Starc to Kohli, FOUR, through covers!",
                            "homeScore": "155/3",
                            "scoreValue": 4,
                            "boundary": true,
                            "over": { "overs": 14.4 },
                            "team": { "abbreviation": "IND" },
                            "batsman": {
                                "athlete": {
                                    "displayName": "Virat Kohli"
                                }
                            }
                        }
                    }
                }]
            }
        });
        let event = parse_latest_event(&json2, &mut last_ball_id).expect("should parse four");
        assert_eq!(event.event_type, MatchEventType::Boundary);
        assert_eq!(event.title, "FOUR!");
        assert!(event.description.contains("Virat Kohli"));
    }

    #[test]
    fn test_parse_soccer_match_detail() {
        let json = serde_json::json!({
            "header": {
                "name": "Arsenal vs Chelsea",
                "competitions": [{
                    "status": {
                        "type": {
                            "state": "in",
                            "detail": "68'"
                        }
                    },
                    "competitors": [
                        {
                            "team": { "id": "359", "displayName": "Arsenal", "abbreviation": "ARS" },
                            "score": "2",
                            "winner": false
                        },
                        {
                            "team": { "id": "363", "displayName": "Chelsea", "abbreviation": "CHE" },
                            "score": "1",
                            "winner": false
                        }
                    ]
                }]
            }
        });

        let score = parse_soccer_match_detail(&json, "eng.1", "700100").expect("should parse");
        assert_eq!(score.match_id, "700100");
        assert_eq!(score.series_id, "eng.1");
        assert_eq!(score.team1.abbreviation, "ARS");
        assert_eq!(score.team1.runs, 2);
        assert_eq!(score.team2.abbreviation, "CHE");
        assert_eq!(score.team2.runs, 1);
        assert_eq!(score.status, MatchStatus::Live);
        assert_eq!(score.soccer_clock, Some("68'".to_string()));
        assert_eq!(score.sport, SportType::Soccer);
    }

    #[test]
    fn test_parse_soccer_goal_event() {
        let mut last_event_id: Option<String> = None;
        let json = serde_json::json!({
            "header": {
                "competitions": [{
                    "competitors": [
                        { "score": "2" },
                        { "score": "1" }
                    ]
                }]
            },
            "keyEvents": [
                {
                    "id": "evt-goal-1",
                    "type": { "type": "goal" },
                    "scoringPlay": true,
                    "shortText": "Bukayo Saka (Arsenal) scores right footed shot",
                    "clock": { "displayValue": "54'" }
                }
            ]
        });

        // First poll seeds dedup state and suppresses stale event by design.
        assert!(parse_soccer_latest_event(&json, &mut last_event_id).is_none());
        assert_eq!(last_event_id, Some("evt-goal-1".to_string()));

        // New event id emits with a filled scoreline.
        let json2 = serde_json::json!({
            "header": {
                "competitions": [{
                    "competitors": [
                        { "score": "2" },
                        { "score": "1" }
                    ]
                }]
            },
            "keyEvents": [
                {
                    "id": "evt-goal-1",
                    "type": { "type": "goal" },
                    "scoringPlay": true,
                    "shortText": "Bukayo Saka (Arsenal) scores right footed shot",
                    "clock": { "displayValue": "54'" }
                },
                {
                    "id": "evt-goal-2",
                    "type": { "type": "goal" },
                    "scoringPlay": true,
                    "shortText": "Kai Havertz (Arsenal) heads in the corner",
                    "clock": { "displayValue": "67'" }
                }
            ]
        });
        let event =
            parse_soccer_latest_event(&json2, &mut last_event_id).expect("should parse goal");
        assert_eq!(event.event_type, MatchEventType::Boundary);
        assert_eq!(event.title, "GOAL!");
        assert!(event.description.contains("Kai Havertz"));
        assert!(event.description.contains("67'"));
        assert_eq!(event.score, "2-1");
        assert_eq!(last_event_id, Some("evt-goal-2".to_string()));

        // Repeated fetch should be deduplicated
        assert!(parse_soccer_latest_event(&json2, &mut last_event_id).is_none());
    }

    #[test]
    fn test_parse_soccer_red_card_event() {
        let mut last_event_id: Option<String> = None;
        let json = serde_json::json!({
            "keyEvents": [
                {
                    "id": "evt-red-1",
                    "type": { "type": "red-card" },
                    "shortText": "Nicolas Jackson (Chelsea) shown red card for serious foul",
                    "clock": { "displayValue": "78'" }
                }
            ]
        });

        // First poll seeds dedup state and suppresses stale event by design.
        assert!(parse_soccer_latest_event(&json, &mut last_event_id).is_none());
        assert_eq!(last_event_id, Some("evt-red-1".to_string()));

        let json2 = serde_json::json!({
            "keyEvents": [
                {
                    "id": "evt-red-1",
                    "type": { "type": "red-card" },
                    "shortText": "Nicolas Jackson (Chelsea) shown red card for serious foul",
                    "clock": { "displayValue": "78'" }
                },
                {
                    "id": "evt-red-2",
                    "type": { "type": "red-card" },
                    "shortText": "Moises Caicedo (Chelsea) shown red card for serious foul",
                    "clock": { "displayValue": "85'" }
                }
            ]
        });
        let event =
            parse_soccer_latest_event(&json2, &mut last_event_id).expect("should parse red card");
        assert_eq!(event.event_type, MatchEventType::Wicket);
        assert_eq!(event.title, "RED CARD!");
        assert!(event.description.contains("Moises Caicedo"));
    }

    #[test]
    fn test_parse_all_live_indian_matches_filters_india() {
        let json = serde_json::json!({
            "sports": [{
                "slug": "cricket",
                "leagues": [{
                    "id": "8048",
                    "name": "India tour of England",
                    "events": [
                        {
                            "id": "1413511",
                            "status": "in",
                            "name": "England vs India",
                            "date": "2026-09-28T14:00Z",
                            "competitors": [
                                { "id": "1", "displayName": "England" },
                                { "id": "6", "displayName": "India" }
                            ]
                        },
                        {
                            "id": "1413512",
                            "status": "in",
                            "name": "Australia vs South Africa",
                            "date": "2026-09-28T14:00Z",
                            "competitors": [
                                { "id": "2", "displayName": "Australia" },
                                { "id": "3", "displayName": "South Africa" }
                            ]
                        },
                        {
                            "id": "1413513",
                            "status": "post",
                            "name": "India vs Sri Lanka",
                            "date": "2026-09-27T14:00Z",
                            "competitors": [
                                { "id": "6", "displayName": "India" },
                                { "id": "8", "displayName": "Sri Lanka" }
                            ]
                        }
                    ]
                }]
            }]
        });
        let out = parse_all_live_indian_matches(&json);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, "1413511");
        assert_eq!(out[0].0, "8048");
    }

    #[test]
    fn test_parse_all_live_indian_matches_name_fallback() {
        // No numeric id match: `displayName` containing "india" still counts.
        let json = serde_json::json!({
            "sports": [{
                "slug": "cricket",
                "leagues": [{
                    "id": "8048",
                    "name": "World Cup",
                    "events": [{
                        "id": "999",
                        "status": "pre",
                        "name": "India Women vs England Women",
                        "date": "2026-09-28T14:00Z",
                        "competitors": [
                            { "id": "99", "displayName": "India Women" },
                            { "id": "100", "displayName": "England Women" }
                        ]
                    }]
                }]
            }]
        });
        let out = parse_all_live_indian_matches(&json);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, "999");
    }

    #[test]
    fn test_parse_soccer_matches_live_and_pre_only() {
        let json = serde_json::json!({
            "sports": [{
                "slug": "soccer",
                "leagues": [{
                    "slug": "eng.1",
                    "id": "23",
                    "name": "Premier League",
                    "events": [
                        {
                            "id": "700100",
                            "status": "in",
                            "name": "Arsenal vs Chelsea",
                            "date": "2026-09-28T16:00Z",
                            "competitors": [
                                { "displayName": "Arsenal" },
                                { "displayName": "Chelsea" }
                            ]
                        },
                        {
                            "id": "700101",
                            "status": "pre",
                            "name": "Liverpool vs City",
                            "date": "2026-09-29T16:00Z",
                            "competitors": [
                                { "displayName": "Liverpool" },
                                { "displayName": "Man City" }
                            ]
                        },
                        {
                            "id": "700102",
                            "status": "post",
                            "name": "Spurs vs Villa",
                            "date": "2026-09-27T16:00Z",
                            "competitors": [
                                { "displayName": "Spurs" },
                                { "displayName": "Villa" }
                            ]
                        }
                    ]
                }]
            }]
        });
        let out = parse_soccer_matches(&json);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "eng.1");
        assert_eq!(out[0].1, "700100");
        assert!(out[0].2.contains("Arsenal"));
        assert_eq!(out[1].1, "700101");
    }

    #[test]
    fn test_parse_soccer_matches_object_status_and_top_level_events() {
        // Test ESPN payload with object status and competitions.competitors nesting
        let json = serde_json::json!({
            "events": [
                {
                    "id": "800101",
                    "status": {
                        "type": {
                            "state": "in",
                            "name": "STATUS_IN_PROGRESS"
                        }
                    },
                    "competitions": [
                        {
                            "competitors": [
                                { "displayName": "Real Madrid" },
                                { "displayName": "Barcelona" }
                            ]
                        }
                    ]
                },
                {
                    "id": "800102",
                    "fullStatus": {
                        "type": {
                            "state": "pre",
                            "name": "STATUS_SCHEDULED"
                        }
                    },
                    "competitions": [
                        {
                            "competitors": [
                                { "displayName": "Bayern Munich" },
                                { "displayName": "Dortmund" }
                            ]
                        }
                    ]
                }
            ],
            "leagues": [
                {
                    "slug": "esp.1",
                    "name": "La Liga"
                }
            ]
        });
        let out = parse_soccer_matches(&json);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "esp.1");
        assert_eq!(out[0].1, "800101");
        assert_eq!(out[0].2, "Real Madrid vs Barcelona");
        assert_eq!(out[0].3, "in");
        assert_eq!(out[1].1, "800102");
        assert_eq!(out[1].2, "Bayern Munich vs Dortmund");
        assert_eq!(out[1].3, "pre");
    }

    #[test]
    fn test_placeholder_commentary_key_skipped() {
        // Only the stub key present: no event must fire.
        let mut last: Option<String> = None;
        let json = serde_json::json!({
            "header": {
                "competitions": [{
                    "commentaries": {
                        "999999999999999": {
                            "shortText": "stub",
                            "homeScore": "0/0"
                        }
                    }
                }]
            }
        });
        assert!(parse_latest_event(&json, &mut last).is_none());
        assert!(last.is_none());
    }
}
