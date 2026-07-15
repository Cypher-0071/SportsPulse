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

const soccerClockContainer = document.getElementById("soccer-clock-container");
const soccerClockValEl     = document.getElementById("soccer-clock-val");
const statsLeftEl          = document.querySelector(".stats-left");

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

    matchTitleEl.textContent = cleanTitle(score.match_title);

    // Toggle soccer-layout styling
    if (score.sport === "Soccer") {
      scoreboardCard.classList.add("soccer-layout");
    } else {
      scoreboardCard.classList.remove("soccer-layout");
    }

    // Status indicator
    const isStale = (Math.floor(Date.now() / 1000) - score.timestamp) > 15;
    if (isStale || !navigator.onLine) {
      liveIndicatorEl.textContent = "⚠ RECONNECTING";
      liveIndicatorEl.style.color = "#ffb703";
      scoreboardCard.classList.remove("event-win");
    } else if (score.status === "Live") {
      liveIndicatorEl.textContent = score.sport === "Soccer" ? `LIVE · ${score.soccer_clock || ""}` : "LIVE";
      liveIndicatorEl.style.color = "#ff4a4a";
      scoreboardCard.classList.remove("event-win");
    } else if (score.status === "Break") {
      liveIndicatorEl.textContent = score.sport === "Soccer" ? "HT" : "BREAK";
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

    // Team 1
    const t1abbr = score.team1.abbreviation || score.team1.name || "T1";
    team1NameEl.textContent  = t1abbr;
    team1ScoreEl.textContent = cleanScoreString(score.team1.score, score.sport);
    team1BattingDot.classList.toggle("hidden", !score.team1.is_batting);

    // Team 2
    const t2abbr = score.team2.abbreviation || score.team2.name || "T2";
    team2NameEl.textContent  = t2abbr;
    team2ScoreEl.textContent = cleanScoreString(score.team2.score, score.sport);
    team2BattingDot.classList.toggle("hidden", !score.team2.is_batting);

    // Stats
    if (score.sport === "Soccer") {
      if (statsLeftEl) statsLeftEl.classList.add("hidden");
      if (targetContainer) targetContainer.classList.add("hidden");
      if (soccerClockContainer) {
        soccerClockContainer.classList.remove("hidden");
        if (soccerClockValEl) {
          soccerClockValEl.textContent = score.soccer_clock || "-";
        }
      }
    } else {
      if (statsLeftEl) statsLeftEl.classList.remove("hidden");
      if (soccerClockContainer) soccerClockContainer.classList.add("hidden");

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
