const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const alertCard = document.getElementById("alert-card");
const eventTitle = document.getElementById("event-title");
const eventDesc = document.getElementById("event-desc");
const eventScore = document.getElementById("event-score");
const closeBtn = document.getElementById("close-btn");

// Close button invokes Rust command to hide window
closeBtn.addEventListener("click", () => {
  invoke("hide_mini_popup");
});

// Helper to update DOM based on event payload
function updateEventDetails(payload) {
  if (!payload) return;

  eventTitle.textContent = payload.title;
  eventDesc.textContent = payload.description;
  eventScore.textContent = payload.score;

  // Set event card style class
  alertCard.className = "alert-card";
  if (payload.event_type === "Wicket") {
    alertCard.classList.add("wicket");
  } else if (payload.event_type === "Boundary") {
    alertCard.classList.add("boundary");
  }
}

// Fetch latest event immediately on load (covers race conditions)
async function loadLatestEvent() {
  try {
    const event = await invoke("get_latest_event");
    updateEventDetails(event);
  } catch (err) {
    console.error("Failed to load latest event:", err);
  }
}

// Listen for live event updates from the Rust background thread
listen("match-event", (event) => {
  updateEventDetails(event.payload);
});

// Initial load
loadLatestEvent();
