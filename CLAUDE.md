# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**RelichHelper** — A cross-platform Warframe relic companion app (Linux + Windows), alternative to alecaframe.com. Scope is deliberately narrow: **relic management only** — drop optimization, owned-item tracking, and a **live overlay during void fissure missions**.

Warframe runs natively on Windows and via **Steam + Proton/Wine** on Linux. Priorities:
1. Drop optimization (which relic to open for best outcome)
2. Owned-item tracking (which prime parts / built items are missing)
3. Void Trace / refinement recommendations

The live overlay, per drop, must show:
- **Vaulted status** of the reward
- Whether the **built item is already owned**

## Core Architectural Decisions

- **No game manipulation, no memory reading.** Inventory/process-memory reading (`/proc/{pid}/mem`, `ReadProcessMemory`) was **rejected** — fragile against patches and an account-ban risk. The credential-based reverse-engineered in-game API is likewise rejected (explicit ban risk).
- **EE.log is the primary live data source.** Verified empirically against a real log (12.06.2026): the relic reward workflow IS logged in plaintext (contrary to the wiki). This makes the live feature possible **without OCR and without memory reading**.
- **OCR (built in-house, Tesseract) is complementary.** EE.log only logs the *local player's own* relic roll by item path; squadmates' rolls are not. OCR fills exactly that gap. Built with permissive crates (`xcap` for capture, `tesseract`/`leptess`, `strsim` for matching) to keep the project MIT — no GPL `wfinfo-ng` code is copied (referenced for approach only).
- **Two languages only:** Rust (native agent + overlay + OCR) and TypeScript (web UI + cloud service). Python was dropped.
- **MIT-licensed.** OCR is built in-house rather than forking GPL `wfinfo-ng`, keeping the whole project permissive.

## EE.log — Verified Grammar (primary data source)

Paths (backend must auto-detect):
- Linux/Proton: `…/steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log` (Steam dir may be `~/.steam/steam/...`, `~/.local/share/Steam/...`, a non-default library like `/mnt/Games/SteamLibrary/...`, or Flatpak `~/.var/app/com.valvesoftware.Steam/...`)
- Windows: `%LOCALAPPDATA%\Warframe\EE.log`

Line format: `<elapsed_seconds> <source> [<level>]: <message>`

| Event | Pattern | Extracts |
|---|---|---|
| Identity | `Logged in <name> (<accountId>)` | own `accountId` → identifies own roll |
| Relic refine/select | `Dialog::CreateOkCancel(description=Refine <Relic> to <TIER>? It will cost <n>.` | relic, refinement tier, trace cost |
| Reward screen open | `VoidProjections: OpenVoidProjectionRewardScreenRMI` + `ProjectionRewardChoice.lua: Relic rewards initialized` | overlay trigger |
| **Own drop** | `VoidProjections: <accountId> gets reward <ItemPath>` | exact item path, language-independent |
| Squadmate drops | `VoidProjections: Client got reward info from <id>` | **no path** → needs OCR |
| Decision countdown | `ProjectionsCountdown.lua: Initialize timer nil\t15` … `Countdown timer expired` | the 15s decision window |
| Screen close | `ProjectionRewardChoice.lua: Relic reward screen shut down` | close overlay |
| Mission end | `EndOfMatch.lua: Mission Succeeded` | tracking |

**Known caveat:** the game buffers log writes; the reward event can appear *after* the screen is gone (documented by wfinfo-ng). Mitigate by observing file flushes and using screen-detection as a secondary OCR trigger.

## Reference Data (drop tables, vaulted status, item names)

- **Authoritative source — official DE drop table** (single HTML, ~4 MB):
  `https://warframe-web-assets.nyc3.cdn.digitaloceanspaces.com/uploads/cms/hnfvc0o3jnfvc873njb03enrf56.html`
  - `<h3 id="relicRewards">` ("Relics:") — every relic at all 4 tiers (Intact/Exceptional/Flawless/Radiant), 6 drops each with exact percentages. Parse `<tr><th colspan="2">Axi A1 Relic (Radiant)</th></tr>` followed by 6 `<td>Item</td><td>Rarity (x%)</td>` rows. **Take refinement odds directly — do not compute them.**
  - `<h3 id="missionRewards">` ("Missions:") — where each relic farms.
- **Vaulted = derived:** a relic with contents in `relicRewards` but appearing in **no** current `missionRewards` source is vaulted. No separate vault list needed.
- **Path↔name mapping (critical):** EE.log gives internal paths (`…/LexPrimeBarrel`); the official table gives display names ("Lex Prime Barrel"). Bridge via [`WFCD/warframe-items`](https://github.com/WFCD/warframe-items) / DE Public Export. Same data backs OCR name-matching (handles localization).
- **warframestat.us** (`docs.warframestat.us`) — supplementary for ducat/plat values and localized names.
- Cache everything in SQLite → offline after first sync. Re-parse the official HTML periodically (patches change drop tables).

## Repository Layout

```
agent/      # Rust: EE.log watcher, in-house OCR (Tesseract), Tauri 2 overlay
web/        # TypeScript: React/Svelte frontend (web app + Tauri webview)
service/    # TypeScript: hosted live service (launches local agent; later matchmaking)
data/       # local SQLite cache, parsed reference data
.claude/    # everything-claude-code framework (agents, rules, commands, skills, hooks)
```

## Owned-Item Tracking

Three combined sources (no Overwolf, no memory, no credential API):
1. **OCR inventory sync (primary):** page-snapshot workflow — user opens a screen with visible names+counts (relic refinement screen for relics; Foundry/components for prime parts), scrolls a page, presses a hotkey → agent OCRs + dedupes by item name. Pick target screens deliberately (many grids show icons without text).
2. **Log-derived finds:** own reward path, mission rewards.
3. **Manual** as fallback/correction.
- Optional ToS-compliant import: AlecaFrame relic-token API (relic counts only; requires the user runs AlecaFrame on Windows). Not a required path.

## Development Workflow / Tooling

This repo integrates the **`everything-claude-code`** framework project-level in `.claude/` (agents, rules, commands, skills, hooks). Hooks run automatically (Node.js) and were reviewed as benign before integration; `${CLAUDE_PROJECT_DIR}/.claude/scripts/...` paths are wired in `.claude/settings.json`.

Use during development:
- `/plan` + `architect` agent → per-component design
- `/tdd` + `tdd-guide` → test-drive the EE.log parser and drop-table parser (clear inputs from the real log/HTML)
- `security-reviewer` → inventory sources, token handling, hook scripts
- `/code-review`, `/refactor-clean`, `/verify`, `/e2e` → quality and end-to-end verification

Hook notes: dev servers are expected to run in tmux; JS/TS edits auto-run Prettier + `tsc`; non-essential `.md` creation is blocked (README/CLAUDE/AGENTS/CONTRIBUTING allowed).

## Overlay (Tauri) — platform notes

- **Windows / Linux X11:** `always_on_top: true` + `decorations: false`. Reliable.
- **Linux / Wayland:** fullscreen overlay support is inconsistent; XWayland is the safe fallback. Check `echo $XDG_SESSION_TYPE`. Screenshots use the `xcap` crate (X11 + Wayland) — the same capture problem wfinfo-ng solves.

## System Dependencies (Linux)

```bash
# Fedora/Nobara
sudo dnf install webkit2gtk4.1-devel openssl-devel libappindicator-gtk3 tesseract libXrandr-devel
# Debian/Ubuntu
sudo apt install libwebkit2gtk-4.1-dev libssl-dev libappindicator3-dev tesseract-ocr libxrandr-dev
```
Windows: no extra system packages; Tauri bundles WebView2.

## Open Questions

- Whether the *final* picked reward (after the 15s) is logged separately — affects how complete log-derived ownership can be.
- `gets reward` line behavior in solo vs. as host (verify).
