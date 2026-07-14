use crate::models::{MatchScore, MatchStatus, TeamScore, MatchEvent, MatchEventType};

pub fn parse_live_indian_match(value: &serde_json::Value) -> Option<(String, String)> {
    let sports = value.get("sports")?.as_array()?;
    for sport in sports {
        if sport.get("slug")?.as_str()? == "cricket" {
            let leagues = sport.get("leagues")?.as_array()?;
            for league in leagues {
                let series_id = league.get("id")?.as_str()?;
                let events = league.get("events")?.as_array()?;
                for event in events {
                    let match_id = event.get("id")?.as_str()?;
                    let competitors = event.get("competitors")?.as_array()?;
                    
                    let mut is_india_match = false;
                    for comp in competitors {
                        let id = comp.get("id")?.as_str()?;
                        let display_name = comp.get("displayName")?.as_str()?;
                        
                        if id == "6" || display_name.to_lowercase() == "india" {
                            is_india_match = true;
                            break;
                        }
                    }
                    
                    if is_india_match {
                        return Some((series_id.to_string(), match_id.to_string()));
                    }
                }
            }
        }
    }
    None
}

pub fn parse_match_detail(value: &serde_json::Value, series_id: &str, match_id: &str) -> Option<MatchScore> {
    let header = value.get("header")?;
    let match_title_base = header.get("name")?.as_str()?;
    let match_desc = header.get("description")?.as_str()?;
    let match_title = format!("{} • {}", match_title_base, match_desc);

    let competitions = header.get("competitions")?.as_array()?;
    let comp = competitions.get(0)?;
    
    let status = comp.get("status")?;
    let state = status.get("type")?.get("state")?.as_str()?;
    let detail = status.get("type")?.get("detail")?.as_str()?.to_lowercase();
    
    let mut status_enum = match state {
        "in" => MatchStatus::Live,
        "pre" => MatchStatus::Scheduled,
        "post" => MatchStatus::Completed,
        _ => MatchStatus::NoMatch,
    };
    
    if status_enum == MatchStatus::Live {
        if detail.contains("delay") || detail.contains("lunch") || detail.contains("tea") 
            || detail.contains("stumps") || detail.contains("rain") || detail.contains("break") {
            status_enum = MatchStatus::Break;
        }
    }

    let competitors_arr = comp.get("competitors")?.as_array()?;
    if competitors_arr.len() < 2 {
        return None;
    }

    let team1 = parse_competitor(&competitors_arr[0]);
    let team2 = parse_competitor(&competitors_arr[1]);

    let batting_team = if team1.is_batting {
        1
    } else if team2.is_batting {
        2
    } else {
        0
    };

    // Calculate CRR
    let crr = if batting_team == 1 {
        calculate_crr(team1.runs, team1.overs)
    } else if batting_team == 2 {
        calculate_crr(team2.runs, team2.overs)
    } else {
        0.0
    };

    // Find if there is a target
    let mut target = None;
    let mut runs_needed = None;
    let mut rrr = None;

    let limited_overs = comp.get("limitedOvers").and_then(|v| v.as_f64()).unwrap_or(50.0) as f32;

    // Check if team 1 is chasing
    if batting_team == 1 {
        if let Some(t) = get_target(&competitors_arr[0]) {
            target = Some(t);
            if team1.runs < t {
                let runs = t - team1.runs;
                runs_needed = Some(runs);
                rrr = Some(calculate_rrr(runs, limited_overs, team1.overs));
            } else {
                runs_needed = Some(0);
                rrr = Some(0.0);
            }
        }
    } else if batting_team == 2 {
        if let Some(t) = get_target(&competitors_arr[1]) {
            target = Some(t);
            if team2.runs < t {
                let runs = t - team2.runs;
                runs_needed = Some(runs);
                rrr = Some(calculate_rrr(runs, limited_overs, team2.overs));
            } else {
                runs_needed = Some(0);
                rrr = Some(0.0);
            }
        }
    }

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
    })
}

fn parse_competitor(comp: &serde_json::Value) -> TeamScore {
    let team = comp.get("team").unwrap();
    let id = team.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let name = team.get("displayName").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let abbreviation = team.get("abbreviation").and_then(|v| v.as_str()).unwrap_or("").to_string();
    
    let mut score_str = comp.get("score").and_then(|v| v.as_str()).unwrap_or("Yet to bat").to_string();
    let mut runs = 0;
    let mut wickets = 0;
    let mut overs = 0.0;
    let mut is_batting = false;

    if let Some(linescores) = comp.get("linescores").and_then(|v| v.as_array()) {
        let active_linescore = linescores.iter().find(|l| {
            l.get("isCurrent").and_then(|v| v.as_bool()).unwrap_or(false) 
            || l.get("isCurrent").and_then(|v| v.as_u64()).map(|n| n == 1).unwrap_or(false)
        }).or_else(|| linescores.last());
            
        if let Some(linescore) = active_linescore {
            runs = linescore.get("runs").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            wickets = linescore.get("wickets").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            overs = linescore.get("overs").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            is_batting = linescore.get("isBatting").and_then(|v| v.as_bool()).unwrap_or(false);
            if let Some(s) = linescore.get("score").and_then(|v| v.as_str()) {
                score_str = s.to_string();
            }
        }
    }

    TeamScore {
        id,
        name,
        abbreviation,
        score: score_str,
        runs,
        wickets,
        overs,
        is_batting,
    }
}

fn get_target(comp: &serde_json::Value) -> Option<u32> {
    let linescores = comp.get("linescores")?.as_array()?;
    let active_linescore = linescores.iter().find(|l| {
        l.get("isCurrent").and_then(|v| v.as_bool()).unwrap_or(false) 
        || l.get("isCurrent").and_then(|v| v.as_u64()).map(|n| n == 1).unwrap_or(false)
    }).or_else(|| linescores.last())?;

    let target = active_linescore.get("target").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if target > 0 {
        Some(target)
    } else {
        None
    }
}

fn calculate_crr(runs: u32, overs_float: f32) -> f32 {
    let completed_overs = overs_float.floor() as u32;
    let extra_balls = ((overs_float - completed_overs as f32) * 10.0 + 0.1).floor() as u32;
    let total_balls = completed_overs * 6 + extra_balls;
    if total_balls == 0 {
        0.0
    } else {
        (runs as f32 / total_balls as f32) * 6.0
    }
}

fn calculate_rrr(runs_needed: u32, total_overs: f32, current_overs: f32) -> f32 {
    let completed_overs = current_overs.floor() as u32;
    let extra_balls = ((current_overs - completed_overs as f32) * 10.0 + 0.1).floor() as u32;
    let current_balls = completed_overs * 6 + extra_balls;
    let total_balls = (total_overs * 6.0) as u32;
    if total_balls <= current_balls {
        0.0
    } else {
        let balls_remaining = total_balls - current_balls;
        (runs_needed as f32 / balls_remaining as f32) * 6.0
    }
}

pub fn parse_latest_event(value: &serde_json::Value, last_ball_id: &mut Option<String>) -> Option<MatchEvent> {
    let header = value.get("header")?;
    let competitions = header.get("competitions")?.as_array()?;
    let comp = competitions.get(0)?;
    let commentaries = comp.get("commentaries")?.as_object()?;
    
    let mut latest_key: Option<u64> = None;
    for key_str in commentaries.keys() {
        if let Ok(key_num) = key_str.parse::<u64>() {
            if latest_key.is_none() || Some(key_num) > latest_key {
                latest_key = Some(key_num);
            }
        }
    }
    
    let latest_key_str = latest_key?.to_string();
    if latest_key_str == "999999999999999" {
        return None;
    }
    
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
    
    let home_score = ball_data.get("homeScore").and_then(|v| v.as_str()).unwrap_or("");
    let over_num = ball_data.get("over").and_then(|o| o.get("overs").and_then(|v| v.as_f64())).unwrap_or(0.0);
    let team_abbr = ball_data.get("team").and_then(|t| t.get("abbreviation").and_then(|v| v.as_str())).unwrap_or("");
    let score_str = format!("{} {} ({} ov)", team_abbr, home_score, over_num);

    let dismissal = ball_data.get("dismissal");
    let is_dismissal = dismissal.and_then(|d| d.get("dismissal").and_then(|v| v.as_bool())).unwrap_or(false);
    if is_dismissal {
        let batsman_name = dismissal.and_then(|d| d.get("batsman").and_then(|b| b.get("athlete").and_then(|a| a.get("displayName").and_then(|v| v.as_str())))).unwrap_or("Batsman");
        let dismissal_text = dismissal.and_then(|d| d.get("text").and_then(|v| v.as_str())).unwrap_or("");
        let short_desc = ball_data.get("shortText").and_then(|v| v.as_str()).unwrap_or("");
        return Some(MatchEvent {
            event_type: MatchEventType::Wicket,
            title: "Wicket!".to_string(),
            description: format!("{}: {} ({})", batsman_name, dismissal_text, short_desc),
            score: score_str,
        });
    }

    let is_boundary = ball_data.get("boundary").and_then(|v| v.as_bool()).unwrap_or(false);
    let score_value = ball_data.get("scoreValue").and_then(|v| v.as_u64()).unwrap_or(0);
    if is_boundary && (score_value == 4 || score_value == 6) {
        let short_desc = ball_data.get("shortText").and_then(|v| v.as_str()).unwrap_or("");
        return Some(MatchEvent {
            event_type: MatchEventType::Boundary,
            title: if score_value == 6 { "SIX!" } else { "FOUR!" }.to_string(),
            description: short_desc.to_string(),
            score: score_str,
        });
    }

    let is_over_complete = ball_data.get("over").and_then(|o| o.get("complete").and_then(|v| v.as_bool())).unwrap_or(false);
    if is_over_complete {
        let over_info = ball_data.get("over");
        let over_number = over_info.and_then(|o| o.get("number").and_then(|v| v.as_u64())).unwrap_or(0);
        let runs_in_over = over_info.and_then(|o| o.get("runs").and_then(|v| v.as_u64())).unwrap_or(0);
        let wickets_in_over = over_info.and_then(|o| o.get("wickets").and_then(|v| v.as_u64())).unwrap_or(0);
        
        return Some(MatchEvent {
            event_type: MatchEventType::OverComplete,
            title: format!("End of Over {}", over_number),
            description: format!("Summary: {} runs, {} wickets", runs_in_over, wickets_in_over),
            score: score_str,
        });
    }

    None
}
