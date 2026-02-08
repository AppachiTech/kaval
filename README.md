# Kaval — Guard your ports.

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

```sh
# Build from source
cargo build --release
sudo cp target/release/kav /usr/local/bin/
```

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

## License

MIT — Madhubalan Appachi
