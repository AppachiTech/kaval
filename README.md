# Kaval — Guard your ports.

[![CI](https://github.com/AppachiTech/kaval/actions/workflows/ci.yml/badge.svg)](https://github.com/AppachiTech/kaval/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Kaval** (காவல், Tamil for "Guard/Watch") is a developer-focused port and process manager TUI built in Rust.

Stop running `lsof -i :3000 | grep LISTEN`. Kaval shows all listening ports, maps them to processes, identifies known dev tools, and lets you kill with a keystroke.

## Features

- **See everything at a glance** — all listening ports with process name, PID, CPU, memory, uptime
- **Smart service detection** — recognizes Vite, Next.js, PostgreSQL, Redis, Django, Docker, and 30+ more
- **Interactive TUI** — filter, sort, navigate, kill processes — all from one screen
- **One-shot commands** — `kav list`, `kav check 3000`, `kav kill 3000` for scripting
- **JSON output** — `kav list --json` for piping to other tools
- **Color-coded** — green for dev servers, yellow for databases, purple for caches

## Install

**Homebrew (recommended):**

```sh
brew update && brew tap AppachiTech/kaval && brew install kaval
```

**Manual (macOS):**

```sh
curl -sL https://downloads.appachi.tech/macos/kav-macos-latest.tar.gz | tar xz
sudo mv kav /usr/local/bin/
```

**Manual (Linux):**

```sh
curl -sL https://downloads.appachi.tech/linux/kav-linux-latest.tar.gz | tar xz
sudo mv kav /usr/local/bin/
```

**Build from source:**

```sh
cargo build --release
sudo cp target/release/kav /usr/local/bin/
```

## Update

```sh
brew upgrade kaval                # Homebrew
```

Manual install users can re-run the install command — the URL always points to the latest release.

## Usage

```sh
kav                  # Launch interactive TUI
kav list             # Print all listening ports
kav list --json      # JSON output
kav check 3000       # What's on port 3000?
kav kill 3000        # Kill process on port 3000
kav kill 3000 -f     # Force kill (SIGKILL)
```

## TUI Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `↑/↓` or `j/k` | Navigate |
| `/` | Filter by port, name, or service |
| `x` | Kill selected process (with confirmation) |
| `K` | Force kill (SIGKILL, no confirmation) |
| `d` | Toggle detail pane |
| `s` | Cycle sort (Port → Name → CPU → Mem) |
| `t` | Toggle TCP/UDP filter |
| `r` | Force refresh |
| `q` / `Esc` | Quit |

## Privacy

- **Zero storage:** Kaval writes nothing to disk. No config, no logs, no database.
- **Zero network:** Kaval makes no network connections of any kind.
- **Zero telemetry:** No analytics, no crash reports, no data collection.

## Links

- [Website](https://www.appachi.tech/kaval/)
- [FAQ](https://www.appachi.tech/kaval/faq)
- [Privacy Policy](https://www.appachi.tech/kaval/privacy/)
- [Contributing](CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)
- [Security Policy](SECURITY.md)

## License

MIT — [Madhubalan Appachi](https://www.linkedin.com/in/madhuappachi/)
