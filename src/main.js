const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ─── Helpers ───
function cleanTitle(title) {
  if (!title) return "";
  // Split at bullet points or commas to isolate team names
  let clean = title.split(/[•,]/)[0].trim();
  return clean;
}

function cleanScoreString(scoreStr, sport) {
  if (!scoreStr) return sport === "Soccer" ? "0" : "Yet to bat";
  let cleaned = scoreStr
    .replace(/,\s*target\s*\d+/i, '')
    .replace(/\s*target\s*:?\s*\d+/i, '')
    .trim();
  // If we ended up with empty parentheses like "xxx ()", clean them up
  cleaned = cleaned.replace(/\s*\(\s*\)/g, '');
  return cleaned;
}

// ─── DOM refs ───
const scoreboardCard = document.getElementById("scoreboard-card");
const noMatchCard    = document.getElementById("no-match-card");

const matchTitleEl   = document.getElementById("match-title");
const liveIndicatorEl = document.getElementById("live-indicator");

const team1NameEl    = document.getElementById("team1-name");
const team1ScoreEl   = document.getElementById("team1-score");
const team1BattingDot = document.getElementById("team1-batting-dot");

const team2NameEl    = document.getElementById("team2-name");
const team2ScoreEl   = document.getElementById("team2-score");
const team2BattingDot = document.getElementById("team2-batting-dot");

const crrValEl       = document.getElementById("crr-val");
const rrrContainer   = document.getElementById("rrr-container");
const rrrValEl       = document.getElementById("rrr-val");
const rrrSep         = document.getElementById("rrr-sep");
const needContainer  = document.getElementById("need-container");
const needValEl      = document.getElementById("need-val");
const needSep        = document.getElementById("need-sep");

const targetContainer = document.getElementById("target-container");
const targetValEl     = document.getElementById("target-val");

const statsLeftEl     = document.getElementById("stats-left-el");

// Football broadcast section refs
const cricketScores   = document.getElementById("cricket-scores");
const footballScores  = document.getElementById("football-scores");
const fbTeam1Name     = document.getElementById("fb-team1-name");
const fbTeam2Name     = document.getElementById("fb-team2-name");
const fbTeam1Score    = document.getElementById("fb-team1-score");
const fbTeam2Score    = document.getElementById("fb-team2-score");
const fbClock         = document.getElementById("fb-clock");
const fbSub           = document.getElementById("fb-sub");

// ─── Scoreboard polling ───
async function updateScoreboard() {
  try {
    const score = await invoke("get_score");
    if (!score) {
      scoreboardCard.classList.add("hidden");
      noMatchCard.classList.remove("hidden");
      return;
    }

    noMatchCard.classList.add("hidden");
    scoreboardCard.classList.remove("hidden");

    const rawTitle = cleanTitle(score.match_title);
    matchTitleEl.textContent = (score.sport === "Soccer" && rawTitle === "Soccer Match")
      ? "FOOTBALL"
      : rawTitle;

    // Toggle football vs cricket layout
    const isFootball = score.sport === "Soccer";
    if (isFootball) {
      cricketScores.classList.add("hidden");
      footballScores.classList.remove("hidden");
      scoreboardCard.classList.add("soccer-layout");
    } else {
      cricketScores.classList.remove("hidden");
      footballScores.classList.add("hidden");
      scoreboardCard.classList.remove("soccer-layout");
    }

    // Status indicator
    const isStale = score.status === "Live" && (Math.floor(Date.now() / 1000) - score.timestamp) > 15;
    if (isStale || !navigator.onLine) {
      liveIndicatorEl.textContent = "⚠ RECONNECTING";
      liveIndicatorEl.style.color = "#ffb703";
      scoreboardCard.classList.remove("event-win");
    } else if (score.status === "Live") {
      liveIndicatorEl.textContent = isFootball ? `LIVE · ${score.soccer_clock || ""}` : "LIVE";
      liveIndicatorEl.style.color = "#ff4a4a";
      scoreboardCard.classList.remove("event-win");
    } else if (score.status === "Break") {
      liveIndicatorEl.textContent = isFootball ? "HT" : "BREAK";
      liveIndicatorEl.style.color = "#ffb703";
      scoreboardCard.classList.remove("event-win");
    } else if (score.status === "Scheduled") {
      liveIndicatorEl.textContent = "UPCOMING";
      liveIndicatorEl.style.color = "#5a9bd5";
      scoreboardCard.classList.remove("event-win");
    } else if (score.status === "Completed") {
      liveIndicatorEl.textContent = "FINISHED";
      liveIndicatorEl.style.color = "#74c69d";
      scoreboardCard.classList.add("event-win");
    } else {
      liveIndicatorEl.textContent = "MATCH";
      liveIndicatorEl.style.color = "#6b6f7b";
      scoreboardCard.classList.remove("event-win");
    }

    // Football broadcast section
    if (isFootball) {
      fbTeam1Name.textContent  = (score.team1.name || score.team1.abbreviation || "T1").substring(0, 3).toUpperCase();
      fbTeam2Name.textContent  = (score.team2.name || score.team2.abbreviation || "T2").substring(0, 3).toUpperCase();
      fbTeam1Score.textContent = score.team1.score || "0";
      fbTeam2Score.textContent = score.team2.score || "0";
      fbClock.textContent      = score.soccer_clock || "-";
      fbSub.textContent        = "";
    } else {
      // Cricket rows
      const t1abbr = score.team1.abbreviation || score.team1.name || "T1";
      team1NameEl.textContent  = t1abbr;
      team1ScoreEl.textContent = cleanScoreString(score.team1.score, score.sport);
      team1BattingDot.classList.toggle("hidden", !score.team1.is_batting);

      const t2abbr = score.team2.abbreviation || score.team2.name || "T2";
      team2NameEl.textContent  = t2abbr;
      team2ScoreEl.textContent = cleanScoreString(score.team2.score, score.sport);
      team2BattingDot.classList.toggle("hidden", !score.team2.is_batting);
    }

    // Footer stats
    // Footer stats
    const statsRow = document.getElementById("stats-row");
    if (isFootball || score.status === "Scheduled" || score.status === "Completed") {
      if (statsRow) statsRow.classList.add("hidden");
    } else {
      if (statsRow) statsRow.classList.remove("hidden");
      if (statsLeftEl) statsLeftEl.classList.remove("hidden");

      if (crrValEl) crrValEl.textContent = score.crr.toFixed(2);

      const hasRrr = score.rrr !== null && score.rrr !== undefined;
      if (rrrContainer) rrrContainer.classList.toggle("hidden", !hasRrr);
      if (rrrSep) rrrSep.classList.toggle("hidden", !hasRrr);
      if (hasRrr && rrrValEl) rrrValEl.textContent = score.rrr.toFixed(2);

      const hasNeed = score.runs_needed !== null && score.runs_needed !== undefined;
      if (needContainer) needContainer.classList.toggle("hidden", !hasNeed);
      if (needSep) needSep.classList.toggle("hidden", !hasNeed);
      if (hasNeed && needValEl) {
        const chasingAbbr = score.batting_team === 1 ? score.team1.abbreviation : score.team2.abbreviation;
        needValEl.textContent = `${chasingAbbr} need ${score.runs_needed}`;
      }

      // Target (on the right in the footer)
      const hasTarget = score.target !== null && score.target !== undefined;
      if (targetContainer && targetValEl) {
        targetContainer.classList.toggle("hidden", !hasTarget);
        if (hasTarget) targetValEl.textContent = score.target;
      }
    }
  } catch (err) {
    console.error("Score fetch error:", err);
  }
}

function getCleanEventDetail(description, eventType) {
  if (!description) return "";

  // Soccer cleanup
  if (description.includes(" Own Goal")) {
    return description.split(" Own Goal")[0].trim();
  }
  if (description.includes(" Goal")) {
    return description.split(" Goal")[0].trim();
  }
  if (description.includes(" Penalty")) {
    return description.split(" Penalty")[0].trim();
  }
  if (description.includes(" Red Card")) {
    return description.split(" Red Card")[0].trim();
  }
  if (description.includes(" Yellow Card")) {
    return description.split(" Yellow Card")[0].trim();
  }

  // Cricket cleanup
  if (eventType === "Wicket") {
    if (description.includes(":")) {
      return description.split(":")[0].trim();
    }
    return description;
  }

  if (eventType === "Boundary") {
    if (description.includes(" to ")) {
      let afterTo = description.split(" to ")[1];
      if (afterTo && afterTo.includes(",")) {
        return afterTo.split(",")[0].trim();
      }
      return afterTo || description;
    }
  }

  return description;
}

// ─── In-card event flash ───
let eventOverlayTimeout = null;

function flashEvent(payload) {
  if (!payload) return;

  scoreboardCard.classList.remove("event-four", "event-six", "event-wicket", "event-goal", "event-redcard");
  const existing = scoreboardCard.querySelector(".event-overlay");
  if (existing) existing.remove();
  if (eventOverlayTimeout) clearTimeout(eventOverlayTimeout);

  void scoreboardCard.offsetWidth; // force reflow to retrigger animation

  if (payload.event_type === "Boundary") {
    scoreboardCard.classList.remove("event-win");
    if (payload.title && payload.title.includes("GOAL")) {
      scoreboardCard.classList.add("event-goal");
    } else {
      const isSix = payload.title && payload.title.includes("SIX");
      scoreboardCard.classList.add(isSix ? "event-six" : "event-four");
    }
  } else if (payload.event_type === "Wicket") {
    scoreboardCard.classList.remove("event-win");
    if (payload.title === "RED CARD!") {
      scoreboardCard.classList.add("event-redcard");
    } else {
      scoreboardCard.classList.add("event-wicket");
    }
  } else if (payload.event_type === "Win") {
    scoreboardCard.classList.add("event-win");
  }

  const overlay = document.createElement("div");
  overlay.className = "event-overlay";
  const cleanDetail = getCleanEventDetail(payload.description, payload.event_type);
  overlay.textContent = `${payload.title}  ${cleanDetail}`;
  scoreboardCard.appendChild(overlay);

  const displayTime = payload.event_type === "Win" ? 8000 : 3000;
  eventOverlayTimeout = setTimeout(() => {
    overlay.classList.add("fade-out");
    setTimeout(() => {
      overlay.remove();
    }, 500);
  }, displayTime);
}

// Listen for match events from Rust
listen("match-event", (event) => flashEvent(event.payload));

// Start polling
updateScoreboard();
setInterval(updateScoreboard, 500);
