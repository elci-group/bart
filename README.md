<p align="center">
  <img src="bart-logo.png" alt="Bart Logo" width="500">
</p>

# Bart

Bart is a fast, highly visual command-line tool written in Rust for analyzing and understanding directory usage. It goes beyond combining `tree` and `du`, acting as a temporal filesystem profiler with interactive filtering, semantic code grouping, and beautiful emoji-based terminal output.

![Core Scan Demo](assets/vhs/scan_core.gif)

## Features

- **Massive Parallel Scanning**: Uses `rayon` to perform incredibly fast, I/O-bound parallel directory traversal.
- **Visual Disk Usage**: See file and directory sizes mapped to visual bars.
- **Hotspot Detection**: Heavy nodes automatically turn yellow (>20% of parent) or red (>50% of parent), letting you spot bloat instantly.
- **Smart Ignore System**: Automatically respects `.gitignore` rules and hides `.git`, `target`, and `node_modules` by default (bypass with `--no-ignore`).
- **Emoji Summaries**: Directories recursively aggregate their contents and display sorted emoji counts (e.g., `261 ⚙️ + 4 🦀 📁`).
- **Differential Mode**: Run `bart --diff` to compare the current filesystem state against the previous scan, showing new files, deleted files, and exact size changes (`Δ`).
- **Semantic Breakdown**: Run `bart --explain` to group a directory's size by language (Rust, Python, JS/TS), vendored dependencies, or build artifact stages (Deps, Incremental Cache, Binaries).
- **Interactive TUI Filter**: Run `bart --filter` to open a terminal UI where you can cycle through detected file formats, traverse files with arrow keys, and instantly open, edit, or remove them.
- **Smart Indexing & Caching**: Bart uses a background observability daemon to maintain near-instant directory caches.
- **Automated Project Detection**: Intelligent heuristics (Cargo, npm, Go, Git) automatically detect and index new projects for low-latency access.
- **Exporting**: Export the directory tree as structured data for downstream integrations via `--json` or `--csv`.

## Differential Mode
Compare the current filesystem state against the previous scan to see exactly where your storage went.
![Differential Scan Demo](assets/vhs/differential.gif)

## Interactive TUI Filter
Browse your filesystem with a keyboard-driven interface. Filter by extension, navigate instantly, and perform file actions without leaving the tool.
![Interactive Filter Demo](assets/vhs/interactive_tui.gif)

## Observability Daemon
Bart includes a powerful dual-daemon system for high-performance filesystem monitoring.
![Daemon Demo](assets/vhs/daemon_observability.gif)

### Inner Daemon (Tracking)
Monitors indexed directories in real-time. Any file change (create, modify, delete) triggers an immediate, parallelized re-scan of the project root, keeping your cache (`.toon`) perfectly synchronized.

### Outer Daemon (Discovery)
Background discovery service that identifies massive unindexed directories (>100MB) and high-value project roots. It suggests new paths to track or automatically indexes them based on your configuration.

### Daemon Commands

```bash
# Start the background daemon
bart daemon start

# Check daemon and discovery status
bart daemon status

# Add a directory to the watch list
bart index add <path>

# Toggle intelligent auto-indexing of new projects
bart index auto true
```

## Installation

Ensure you have Rust and Cargo installed.

```bash
cargo build --release
sudo cp target/release/bart /usr/local/bin/
```
*(Optional: Run `strip target/release/bart` to heavily reduce binary size).*

## Usage

```bash
bart [OPTIONS] [PATH]
```

### Options

- `-d, --depth <DEPTH>`  
  Maximum depth to display (default: 1).

- `-n, --limit <LIMIT>`  
  Number of top entries to show per directory (0 for all).

- `-s, --sort <SORT>`  
  Sort by `size` or `name` (default: size).

- `-f, --filter`  
  Launch the interactive TUI to filter by file format and perform actions (open, edit, remove).

- `--diff`  
  Compare the current directory against the previous scan, displaying a differential size breakdown.

- `--explain`  
  Perform a deep semantic breakdown showing exactly *why* a directory is so large (e.g., Build Artifacts, Source Code, Version Control).

- `--no-ignore`  
  Do not respect `.gitignore` rules and include typically ignored directories (`.git`, `node_modules`, `target`).

- `--json` / `--csv`  
  Export the entire directory structure to structured JSON or CSV format.

- `-h, --help`  
  Print help.

- `-w, --watch`  
  Watch directory for live updates.

### Examples

**Scan current directory (depth 1):**
```bash
bart
```

**Find out exactly why your project is so heavy:**
```bash
bart --explain
```

**Compare how much space was just added/removed since your last scan:**
```bash
bart --diff
```

**Launch interactive format filtering:**
```bash
bart -f
```

**Export as JSON ignoring depth limits:**
```bash
bart --json --depth 999 > report.json
```
