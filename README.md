# RelichHelper

A cross-platform **Warframe relic companion** for **Linux and Windows** — a focused, open alternative to AlecaFrame. It does relic management only: drop optimization, owned-item tracking, and a live overlay during void fissure missions.

> 🇬🇧 English below · 🇩🇪 [Deutsche Version weiter unten](#deutsch)

---

## English

### What it does

- **Drop optimization** — which relic to open for the best outcome (refinement-aware odds).
- **Owned-item tracking** — which prime parts / built items you are still missing.
- **Live overlay** during fissure reward selection that shows, per drop:
  - **Vaulted status** of the reward
  - whether the **built item is already owned**
- **Void Trace / refinement** recommendations.

### How it gets data (no game manipulation)

- **EE.log is the primary live source.** Verified empirically against a real log: Warframe writes the relic-reward workflow to `EE.log` in plaintext (own roll by exact item path, reward-screen open/close, the 15s decision countdown, mission end). No memory reading, no credential API — both were rejected as fragile and account-risky.
- **OCR (planned)** only fills the one gap the log leaves: squadmates' rolls are logged without an item path, so the live overlay reads those four reward tiles from the screen.
- **Reference data** comes from the official Digital Extremes drop table (relic contents per refinement tier + farming sources); vaulted status is derived from it.

### Project status

| Phase | Component | Status |
|------:|-----------|--------|
| 0 | Monorepo scaffold + dev tooling | ✅ done |
| 1 | EE.log parser + path resolution (Rust) | ✅ done, verified against a real log |
| 2 | Reference-data sync (drop tables → SQLite, path↔name) | ✅ done, verified end-to-end |
| 3 | Data-driven relic model (drop table + vaulted/owned) | ✅ done, verified end-to-end |
| 4 | Owned-item tracking (manual + log-derived) | ✅ done, verified end-to-end (OCR sync in phase 5) |
| 5 | OCR layer (reward screen + inventory sync) | 🔄 fuzzy matcher done & verified; capture/recognize feature-gated (needs local Tesseract) |
| 6 | Tauri overlay | ⬜ planned |
| 7 | Web app + hosted live service (+ matchmaking) | ⬜ planned |

**Phase 1 highlights:** parses identity, relic refine (relic + tier), own reward path, squadmate reward info, decision countdown, screen open/close and mission end. EE.log is auto-located across all Steam library roots (including custom drives via `libraryfolders.vdf` and Flatpak) and Windows, with a persisted manual-path override as fallback. 16 tests pass.

### Architecture

Two languages, by design:

```
Warframe (Proton/native) ──► EE.log
                                │
                      ┌─────────▼─────────┐
                      │  agent/ (Rust)     │  EE.log watcher · OCR (planned) · Tauri overlay
                      └─────────┬─────────┘
                                │ IPC / WebSocket
                  ┌─────────────┴─────────────┐
                  ▼                           ▼
            web/ (TypeScript)          service/ (TypeScript)
            React/Svelte UI            hosted live service · later: matchmaking
```

```
agent/      # Rust: EE.log watcher, OCR core, Tauri 2 overlay
web/        # TypeScript: frontend (web app + Tauri webview)
service/    # TypeScript: hosted live service
data/       # local SQLite cache, parsed reference data
.claude/    # everything-claude-code dev framework (agents, rules, hooks)
```

### Getting started (agent, Phase 1)

Requires Rust ≥ 1.74.

```bash
# from the repo root
cargo test  --manifest-path agent/Cargo.toml      # run the test suite
cargo run   --manifest-path agent/Cargo.toml -- locate   # show probed EE.log paths
cargo run   --manifest-path agent/Cargo.toml -- watch     # follow live, one JSON event per line
cargo run   --manifest-path agent/Cargo.toml -- parse [FILE]   # batch-parse a log
cargo run   --manifest-path agent/Cargo.toml -- sync HTML [DB]  # drop-table HTML -> SQLite cache
cargo run   --manifest-path agent/Cargo.toml -- resolve PATH    # reward path -> item + vault + sources
cargo run   --manifest-path agent/Cargo.toml -- relic NAME [TIER]  # relic drop table (vault/owned annotated)
cargo run   --manifest-path agent/Cargo.toml -- own list|add|remove|from-log  # owned-item tracking
cargo run   --manifest-path agent/Cargo.toml -- replay [LOG] [DB]  # enriched overlay feed (one-shot)
cargo run   --manifest-path agent/Cargo.toml -- daemon [LOG] [DB]  # enriched overlay feed (live)
cargo run   --manifest-path agent/Cargo.toml -- match "OCR text" [DB]  # snap OCR text to a known item name
```

The `replay`/`daemon` feed is the data stream the overlay (phase 6) and web app (phase 7) consume: per reward roll it emits a self-contained event with vault status, ownership, and relic sources already resolved — reconstructed entirely from `EE.log` + the local caches, no OCR.

Linux system dependencies (for later phases — overlay & OCR):

```bash
# Fedora/Nobara
sudo dnf install webkit2gtk4.1-devel openssl-devel libappindicator-gtk3 tesseract libXrandr-devel
# Debian/Ubuntu
sudo apt install libwebkit2gtk-4.1-dev libssl-dev libappindicator3-dev tesseract-ocr libxrandr-dev
```

### License

**MIT** (see [`LICENSE`](LICENSE)). The project stays permissive: the planned OCR layer is built in-house (Tesseract via `xcap`/`leptess` + fuzzy matching), so no GPL code is incorporated. [`wfinfo-ng`](https://github.com/knoellle/wfinfo-ng) is referenced for approach only.

### Disclaimer

Not affiliated with Digital Extremes. Warframe, relic/drop data, and item names are property of Digital Extremes. This tool only reads local log files and public data; it does not modify the game or read process memory.

---

## Deutsch

Ein plattformübergreifender **Warframe-Relikt-Begleiter** für **Linux und Windows** — eine fokussierte, offene Alternative zu AlecaFrame. Ausschließlich Relikt-Management: Drop-Optimierung, Besitz-Tracking und ein Live-Overlay während Void-Fissure-Missionen.

### Was es kann

- **Drop-Optimierung** — welches Relikt sich am meisten zu öffnen lohnt (mit Refinement-Wahrscheinlichkeiten).
- **Besitz-Tracking** — welche Prime-Teile / gebauten Items dir noch fehlen.
- **Live-Overlay** bei der Fissure-Belohnungswahl, das pro Drop zeigt:
  - **Vaulted-Status** der Belohnung
  - ob das **gebaute Item bereits im Besitz** ist
- **Void-Trace- / Refinement-**Empfehlungen.

### Woher die Daten kommen (keine Spielmanipulation)

- **EE.log ist die primäre Live-Quelle.** Empirisch an einer echten Log verifiziert: Warframe schreibt den Relikt-Belohnungsablauf im Klartext in `EE.log` (eigener Roll als exakter Item-Pfad, Belohnungsscreen auf/zu, der 15s-Entscheidungs-Countdown, Missionsende). Kein Memory-Reading, keine Credential-API — beides als fragil und account-gefährdend verworfen.
- **OCR (geplant)** füllt nur die eine Lücke des Logs: Rolls der Mitspieler werden ohne Item-Pfad geloggt, daher liest das Overlay diese vier Belohnungsfelder vom Bildschirm.
- **Referenzdaten** stammen aus der offiziellen Digital-Extremes-Drop-Tabelle (Relikt-Inhalte je Refinement-Stufe + Farm-Quellen); der Vaulted-Status wird daraus abgeleitet.

### Projektstand

| Phase | Komponente | Status |
|------:|------------|--------|
| 0 | Monorepo-Scaffold + Dev-Tooling | ✅ fertig |
| 1 | EE.log-Parser + Pfad-Auflösung (Rust) | ✅ fertig, gegen echte Log verifiziert |
| 2 | Referenzdaten-Sync (Drop-Tabellen → SQLite, Pfad↔Name) | ✅ fertig, end-to-end verifiziert |
| 3 | Datengetriebenes Relikt-Modell (Drop-Tabelle + Vaulted/Owned) | ✅ fertig, end-to-end verifiziert |
| 4 | Besitz-Tracking (manuell + log-abgeleitet) | ✅ fertig, end-to-end verifiziert (OCR-Sync in Phase 5) |
| 5 | OCR-Schicht (Belohnungsscreen + Inventar-Sync) | 🔄 Fuzzy-Matcher fertig & verifiziert; Capture/Recognize feature-gated (braucht lokales Tesseract) |
| 6 | Tauri-Overlay | ⬜ geplant |
| 7 | Web-App + gehosteter Live-Service (+ Matchmaking) | ⬜ geplant |

**Phase-1-Highlights:** parst Identität, Relikt-Refinement (Relikt + Stufe), eigenen Belohnungspfad, Mitspieler-Info, Entscheidungs-Countdown, Screen auf/zu und Missionsende. EE.log wird über alle Steam-Library-Roots automatisch gefunden (inkl. Custom-Laufwerke via `libraryfolders.vdf` und Flatpak) sowie unter Windows, mit persistiertem manuellem Pfad-Override als Fallback. 16 Tests grün.

### Architektur

Bewusst zwei Sprachen:

```
Warframe (Proton/nativ) ──► EE.log
                              │
                    ┌─────────▼─────────┐
                    │  agent/ (Rust)     │  EE.log-Watcher · OCR (geplant) · Tauri-Overlay
                    └─────────┬─────────┘
                              │ IPC / WebSocket
                ┌─────────────┴─────────────┐
                ▼                           ▼
          web/ (TypeScript)          service/ (TypeScript)
          React/Svelte-UI            gehosteter Live-Service · später: Matchmaking
```

```
agent/      # Rust: EE.log-Watcher, OCR-Kern, Tauri-2-Overlay
web/        # TypeScript: Frontend (Web-App + Tauri-Webview)
service/    # TypeScript: gehosteter Live-Service
data/       # lokaler SQLite-Cache, geparste Referenzdaten
.claude/    # everything-claude-code Dev-Framework (Agents, Rules, Hooks)
```

### Loslegen (Agent, Phase 1)

Benötigt Rust ≥ 1.74.

```bash
# vom Repo-Root
cargo test  --manifest-path agent/Cargo.toml      # Testsuite ausführen
cargo run   --manifest-path agent/Cargo.toml -- locate   # geprüfte EE.log-Pfade anzeigen
cargo run   --manifest-path agent/Cargo.toml -- watch     # live folgen, ein JSON-Event pro Zeile
cargo run   --manifest-path agent/Cargo.toml -- parse [DATEI]   # Log batch-parsen
cargo run   --manifest-path agent/Cargo.toml -- sync HTML [DB]  # Drop-Table-HTML -> SQLite-Cache
cargo run   --manifest-path agent/Cargo.toml -- resolve PATH    # Reward-Pfad -> Item + Vault + Quellen
cargo run   --manifest-path agent/Cargo.toml -- relic NAME [TIER]  # Relikt-Drop-Tabelle (Vault/Owned annotiert)
cargo run   --manifest-path agent/Cargo.toml -- own list|add|remove|from-log  # Besitz-Tracking
cargo run   --manifest-path agent/Cargo.toml -- replay [LOG] [DB]  # angereicherter Overlay-Feed (einmalig)
cargo run   --manifest-path agent/Cargo.toml -- daemon [LOG] [DB]  # angereicherter Overlay-Feed (live)
cargo run   --manifest-path agent/Cargo.toml -- match "OCR-Text" [DB]  # OCR-Text auf bekannten Item-Namen abbilden
```

Der `replay`/`daemon`-Feed ist der Datenstrom, den Overlay (Phase 6) und Web-App (Phase 7) konsumieren: pro Reward-Roll ein in sich geschlossenes Event mit bereits aufgelöstem Vault-Status, Besitz und Relikt-Quellen — vollständig aus `EE.log` + lokalen Caches rekonstruiert, ohne OCR.

Linux-Systemabhängigkeiten (für spätere Phasen — Overlay & OCR):

```bash
# Fedora/Nobara
sudo dnf install webkit2gtk4.1-devel openssl-devel libappindicator-gtk3 tesseract libXrandr-devel
# Debian/Ubuntu
sudo apt install libwebkit2gtk-4.1-dev libssl-dev libappindicator3-dev tesseract-ocr libxrandr-dev
```

### Lizenz

**MIT** (siehe [`LICENSE`](LICENSE)). Das Projekt bleibt permissiv: Die geplante OCR-Schicht wird selbst gebaut (Tesseract via `xcap`/`leptess` + Fuzzy-Matching), es wird also kein GPL-Code übernommen. [`wfinfo-ng`](https://github.com/knoellle/wfinfo-ng) dient nur als Referenz für den Ansatz.

### Haftungsausschluss

Nicht mit Digital Extremes verbunden. Warframe, Relikt-/Drop-Daten und Item-Namen sind Eigentum von Digital Extremes. Dieses Tool liest nur lokale Log-Dateien und öffentliche Daten; es verändert das Spiel nicht und liest keinen Prozessspeicher.
