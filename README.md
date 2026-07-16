<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="SportsPulse Logo" />
</p>

<h1 align="center">SportsPulse</h1>

<p align="center">
  A lightweight, premium desktop scoreboard companion for real-time Cricket and Football (Soccer) match tracking.
</p>

---

## ⚡ Features

- **Dynamic Scoreboard Overlay**: Instantly flips between a stacked scorecard for Cricket and a horizontal broadcast-style bar for Football (Soccer).
- **Match Dashboard**: Browse and track live or upcoming Indian cricket matches, international series, and major football leagues.
- **Smart Polling Engine**: Intelligently scales request intervals based on game status (e.g., fast polling for active live matches, slow/hibernating polling for completed or upcoming fixtures to save resources).
- **Seamless OS Integration**:
  - **Global Shortcut**: Toggle scoreboard visibility at any time with `Ctrl+Alt+Space`.
  - **System Tray**: Run in the background with a system tray menu to launch the dashboard or quit the app.
  - **Interactive Popup Notifications**: Mini-popups at the bottom-right of your screen alert you to major match events (wickets, boundaries, goals, and wins) even when the main scoreboard is hidden.

---

## 🛠️ Tech Stack

- **Backend**: Rust (Tauri v2, Tokio runtime, Reqwest, Serde)
- **Frontend**: Vanilla HTML5, CSS3, ES6 JavaScript
- **API**: Real-time summary integration via ESPN API

---

## 🚀 Getting Started

### Prerequisites

Make sure you have the following installed:
- [Rust & Cargo](https://www.rust-lang.org/tools/install)
- [Node.js](https://nodejs.org/) (with `pnpm` or your preferred package manager)

### Development

Install dependencies and start the development server:

```bash
# Install NPM packages
pnpm install

# Start the Tauri development environment
pnpm tauri dev
```

### Production Build

Compile the production binary:

```bash
pnpm tauri build
```

---

## ⌨️ Global Shortcuts

- **Toggle Scoreboard**: `Ctrl+Alt+Space`
