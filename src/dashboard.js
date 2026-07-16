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

let isFirstLoad = true;

async function refresh() {
  const loader = document.getElementById("loading-overlay");
  const sections = document.querySelectorAll(".section-title, #live-list, #upcoming-list");

  try {
    // Check if the background thread has finished its first full fetch
    const isReady = await invoke("is_initial_fetch_completed");
    if (!isReady) {
      if (isFirstLoad) {
        if (loader) loader.classList.remove("hidden");
        sections.forEach(s => s.classList.add("hidden"));
      }
      return;
    }

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

    // 4. Split and group matches
    const liveMatches = sportMatches.filter(m => m[4] === "in");
    const upcomingMatches = sportMatches.filter(m => m[4] !== "in");

function formatStartTime(rawIso) {
  if (!rawIso) return "";
  try {
    const dateObj = new Date(rawIso);
    if (isNaN(dateObj.getTime())) return "";
    
    // Explicitly format in India Standard Time (Asia/Kolkata)
    const optionsDate = { day: 'numeric', month: 'short', timeZone: 'Asia/Kolkata' };
    const optionsTime = { hour: '2-digit', minute: '2-digit', hour12: true, timeZone: 'Asia/Kolkata' };
    
    const dateStr = dateObj.toLocaleDateString("en-IN", optionsDate);
    const timeStr = dateObj.toLocaleTimeString("en-IN", optionsTime);
    
    return `${dateStr}, ${timeStr}`;
  } catch (e) {
    return "";
  }
}

    function generateGroupedHtml(matchesList, isLiveSection) {
      if (matchesList.length === 0) return "";
      
      const groups = {};
      matchesList.forEach(m => {
        const league = m[5] || "Other Series";
        if (!groups[league]) {
          groups[league] = [];
        }
        groups[league].push(m);
      });

      let html = "";
      Object.keys(groups).sort().forEach(leagueName => {
        const leagueMatches = groups[leagueName];
        let cardsHtml = "";
        
        leagueMatches.forEach(([sport, seriesId, matchId, cleanTitle, status, leagueNameField, startTime]) => {
          const isActive = activeMatch && 
                           activeMatch.sport === sport && 
                           activeMatch.seriesId === seriesId && 
                           activeMatch.matchId === matchId;

          const actionHtml = isActive 
            ? `<button class="tracked-badge clickable-badge" onclick="window.untrack()" onmouseover="this.innerText='Untrack'" onmouseout="this.innerText='Tracked'">Tracked</button>`
            : `<button class="track-btn" onclick="window.selectAndTrack('${sport}', '${seriesId}', '${matchId}')">Track</button>`;

          let dateHtml = "";
          if (!isLiveSection && startTime) {
            const formattedTime = formatStartTime(startTime);
            if (formattedTime) {
              dateHtml = `<span class="match-time">${formattedTime}</span>`;
            }
          }

          cardsHtml += `
            <div class="match-card">
              <div class="match-info">
                <span class="match-title">${cleanTitle}</span>
                ${dateHtml}
              </div>
              <div>
                ${actionHtml}
              </div>
            </div>
          `;
        });

        const borderClass = isLiveSection ? "live-border" : "";
        html += `
          <div class="league-group">
            <div class="league-group-title ${borderClass}">${leagueName.toUpperCase()}</div>
            <div class="grid">
              ${cardsHtml}
            </div>
          </div>
        `;
      });

      return html;
    }

    liveListEl.innerHTML = generateGroupedHtml(liveMatches, true) || `<div class="empty-state">No live matches currently</div>`;
    upcomingListEl.innerHTML = generateGroupedHtml(upcomingMatches, false) || `<div class="empty-state">No upcoming matches currently</div>`;

    if (isFirstLoad) {
      isFirstLoad = false;
      if (loader) loader.classList.add("hidden");
      sections.forEach(s => s.classList.remove("hidden"));
    }

  } catch (err) {
    console.error("Refresh error:", err);
    if (isFirstLoad) {
      isFirstLoad = false;
      if (loader) loader.classList.add("hidden");
      sections.forEach(s => s.classList.remove("hidden"));
    }
  }
}

// Attach selection to window scope for onclick inline binding
window.selectAndTrack = (sport, seriesId, matchId) => {
  trackMatch(sport, seriesId, matchId);
};

window.untrack = async () => {
  try {
    await invoke("untrack_match");
    await refresh();
  } catch (err) {
    console.error("Failed to untrack match:", err);
  }
};

// Initial load and periodic polling
refresh();
setInterval(refresh, 2000);
