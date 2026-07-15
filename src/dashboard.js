const { invoke } = window.__TAURI__.core;

let currentSport = "cricket";
let activeMatch = null; // { sport, seriesId, matchId }

const tabCricket = document.getElementById("tab-cricket");
const tabFootball = document.getElementById("tab-football");
const liveListEl = document.getElementById("live-list");
const upcomingListEl = document.getElementById("upcoming-list");

// Tab handlers
tabCricket.addEventListener("click", () => {
  currentSport = "cricket";
  tabCricket.classList.add("active");
  tabFootball.classList.remove("active");
  refresh();
});

tabFootball.addEventListener("click", () => {
  currentSport = "soccer";
  tabFootball.classList.add("active");
  tabCricket.classList.remove("active");
  refresh();
});

async function trackMatch(sport, seriesId, matchId) {
  try {
    await invoke("select_match", { sport, seriesId, matchId });
    await refresh();
  } catch (err) {
    console.error("Failed to select match:", err);
  }
}

async function refresh() {
  try {
    // 1. Fetch currently active match
    const active = await invoke("get_active_match");
    if (active) {
      activeMatch = {
        sport: active[0],
        seriesId: active[1],
        matchId: active[2]
      };
    } else {
      activeMatch = null;
    }

    // 2. Fetch all discovered matches
    const matches = await invoke("get_discovered_matches") || [];
    
    // 3. Filter by current sport
    const sportMatches = matches.filter(m => m[0] === currentSport);

    let liveHtml = "";
    let upcomingHtml = "";

    sportMatches.forEach(([sport, seriesId, matchId, cleanTitle, status, leagueName]) => {
      const isLive = status === "in";

      const isActive = activeMatch && 
                       activeMatch.sport === sport && 
                       activeMatch.seriesId === seriesId && 
                       activeMatch.matchId === matchId;

      const actionHtml = isActive 
        ? `<span class="tracked-badge">Tracked</span>`
        : `<button class="track-btn" onclick="window.selectAndTrack('${sport}', '${seriesId}', '${matchId}')">Track</button>`;

      const cardHtml = `
        <div class="match-card">
          <div class="match-info">
            <span class="match-title">${cleanTitle}</span>
            <span class="match-series">${leagueName.toUpperCase()}</span>
          </div>
          <div>
            ${actionHtml}
          </div>
        </div>
      `;

      if (isLive) {
        liveHtml += cardHtml;
      } else {
        upcomingHtml += cardHtml;
      }
    });

    // Populate lists or show empty states
    liveListEl.innerHTML = liveHtml || `<div class="empty-state">No live matches currently</div>`;
    upcomingListEl.innerHTML = upcomingHtml || `<div class="empty-state">No upcoming matches currently</div>`;

  } catch (err) {
    console.error("Refresh error:", err);
  }
}

// Attach selection to window scope for onclick inline binding
window.selectAndTrack = (sport, seriesId, matchId) => {
  trackMatch(sport, seriesId, matchId);
};

// Initial load and periodic polling
refresh();
setInterval(refresh, 2000);
