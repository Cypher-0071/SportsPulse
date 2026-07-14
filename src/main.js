const { invoke } = window.__TAURI__.core;

const scoreboardCard = document.getElementById("scoreboard-card");
const noMatchCard = document.getElementById("no-match-card");

const matchTitleEl = document.getElementById("match-title");
const liveIndicatorEl = document.getElementById("live-indicator");

const team1NameEl = document.getElementById("team1-name");
const team1ScoreEl = document.getElementById("team1-score");
const team1BattingDot = document.getElementById("team1-batting-dot");

const team2NameEl = document.getElementById("team2-name");
const team2ScoreEl = document.getElementById("team2-score");
const team2BattingDot = document.getElementById("team2-batting-dot");

const crrValEl = document.getElementById("crr-val");
const rrrContainer = document.getElementById("rrr-container");
const rrrValEl = document.getElementById("rrr-val");
const needContainer = document.getElementById("need-container");
const needValEl = document.getElementById("need-val");

async function updateScoreboard() {
  try {
    const score = await invoke("get_score");
    if (!score) {
      // Toggle views: show placeholder, hide scoreboard
      scoreboardCard.classList.add("hidden");
      noMatchCard.classList.remove("hidden");
      return;
    }

    // Toggle views: show scoreboard, hide placeholder
    noMatchCard.classList.add("hidden");
    scoreboardCard.classList.remove("hidden");

    // Populate data
    matchTitleEl.textContent = score.match_title;
    
    // Status text (check if backend has stale data - offline handling)
    const isStale = (Math.floor(Date.now() / 1000) - score.timestamp) > 15;
    
    if (isStale || !navigator.onLine) {
      liveIndicatorEl.textContent = "⚠️ RECONNECTING";
      liveIndicatorEl.style.background = "rgba(255, 183, 3, 0.2)";
      liveIndicatorEl.style.color = "#ffb703";
      liveIndicatorEl.style.borderColor = "rgba(255, 183, 3, 0.4)";
    } else if (score.status === "Live") {
      liveIndicatorEl.textContent = "🔴 LIVE";
      liveIndicatorEl.style.background = "rgba(224, 36, 36, 0.2)";
      liveIndicatorEl.style.color = "#ff4a4a";
      liveIndicatorEl.style.borderColor = "rgba(224, 36, 36, 0.4)";
    } else if (score.status === "Break") {
      liveIndicatorEl.textContent = "⏸ BREAK";
      liveIndicatorEl.style.background = "rgba(255, 183, 3, 0.2)";
      liveIndicatorEl.style.color = "#ffb703";
      liveIndicatorEl.style.borderColor = "rgba(255, 183, 3, 0.4)";
    } else if (score.status === "Scheduled") {
      liveIndicatorEl.textContent = "📅 UPCOMING";
      liveIndicatorEl.style.background = "rgba(0, 180, 216, 0.2)";
      liveIndicatorEl.style.color = "#00b4d8";
      liveIndicatorEl.style.borderColor = "rgba(0, 180, 216, 0.4)";
    } else if (score.status === "Completed") {
      liveIndicatorEl.textContent = "🏁 FINISHED";
      liveIndicatorEl.style.background = "rgba(79, 119, 45, 0.2)";
      liveIndicatorEl.style.color = "#74c69d";
      liveIndicatorEl.style.borderColor = "rgba(79, 119, 45, 0.4)";
    } else {
      liveIndicatorEl.textContent = "🏏 MATCH";
    }

    // Team 1
    team1NameEl.textContent = score.team1.displayName || score.team1.name || "TEAM 1";
    team1ScoreEl.textContent = score.team1.score || "Yet to bat";
    if (score.team1.is_batting) {
      team1BattingDot.classList.remove("hidden");
    } else {
      team1BattingDot.classList.add("hidden");
    }

    // Team 2
    team2NameEl.textContent = score.team2.displayName || score.team2.name || "TEAM 2";
    team2ScoreEl.textContent = score.team2.score || "Yet to bat";
    if (score.team2.is_batting) {
      team2BattingDot.classList.remove("hidden");
    } else {
      team2BattingDot.classList.add("hidden");
    }

    // CRR
    crrValEl.textContent = score.crr.toFixed(2);

    // RRR
    if (score.rrr !== null && score.rrr !== undefined) {
      rrrContainer.classList.remove("hidden");
      rrrValEl.textContent = score.rrr.toFixed(2);
    } else {
      rrrContainer.classList.add("hidden");
    }

    // Runs needed
    if (score.runs_needed !== null && score.runs_needed !== undefined) {
      needContainer.classList.remove("hidden");
      
      const chasingTeam = score.batting_team === 1 ? score.team1.abbreviation : score.team2.abbreviation;
      needValEl.textContent = `${chasingTeam} needs ${score.runs_needed} runs`;
    } else {
      needContainer.classList.add("hidden");
    }
  } catch (err) {
    console.error("Failed to fetch score from Tauri backend:", err);
  }
}

// Initial update and register interval
updateScoreboard();
setInterval(updateScoreboard, 500);
