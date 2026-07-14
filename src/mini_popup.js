const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const alertCard = document.getElementById("alert-card");
const eventTitle = document.getElementById("event-title");
const eventDesc = document.getElementById("event-desc");
const eventScore = document.getElementById("event-score");
const closeBtn = document.getElementById("close-btn");

const currentWindow = getCurrentWindow();
let dismissTimeout = null;

// Handle manual close
closeBtn.addEventListener("click", () => {
  if (dismissTimeout) clearTimeout(dismissTimeout);
  currentWindow.hide();
});

// Listen for play events from the Rust backend
listen("match-event", (event) => {
  const payload = event.payload;
  if (!payload) return;

  // Clear previous auto-dismiss timer
  if (dismissTimeout) clearTimeout(dismissTimeout);

  // Update text content
  eventTitle.textContent = payload.title;
  eventDesc.textContent = payload.description;
  eventScore.textContent = payload.score;

  // Reset classes and apply type-specific styling
  alertCard.className = "alert-card";
  
  if (payload.event_type === "Wicket") {
    alertCard.classList.add("wicket");
  } else if (payload.event_type === "Boundary") {
    alertCard.classList.add("boundary");
  } else if (payload.event_type === "OverComplete") {
    alertCard.classList.add("over-complete");
  }

  // Auto-dismiss after 5 seconds
  dismissTimeout = setTimeout(() => {
    currentWindow.hide();
  }, 5000);
});
