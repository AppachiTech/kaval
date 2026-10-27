# Kaval — Guard your ports.

[![CI](https://github.com/AppachiTech/kaval/actions/workflows/ci.yml/badge.svg)](https://github.com/AppachiTech/kaval/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/kaval)](https://crates.io/crates/kaval)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Kaval** (காவல், Tamil for "Guard/Watch") is a developer-focused port and process manager for macOS and Linux, with an interactive TUI and scriptable commands. The binary is `kav`.

Stop running `lsof -i :3000 | grep LISTEN`. Kaval shows every listening port, who owns it, which project and container it belongs to, and whether other machines can reach it — and frees a port safely when you need it back.

<p align="center">
  <img src="demo/demo.gif" alt="Kaval demo" width="100%">
</p>

## Features

- **Everything at a glance** — every TCP and UDP listener with its bind address, process, service, project, PID, CPU, memory and uptime
- **Knows your stack** — 140+ services detected from command lines, process names and ports: Vite, Next.js, Django, Uvicorn, Rails, Tomcat, PostgreSQL, Redis, Kafka and more; a trailing `?` marks a guess from the port alone
- **Exposure at a glance** — see which listeners are reachable from other machines (`BIND *`), and list only those with `--exposed`
- **Projects and containers** — each listener is tied to its git project, and Docker-published ports to their container and compose project
- **Safe kills** — waits until the port is really free, catches supervisors that restart what you stopped (`--parent`), stops containers with `docker stop`, and refuses to take down container runtimes or system services
- **Built for scripts and agents** — `kav wait`, `kav pick`, `kav check --free`, versioned JSON and meaningful exit codes
- **Interactive TUI** — filter, sort, multi-select, copy, probe, open in a browser and kill without leaving one screen; dark, light and no-colour themes

## Install

### Homebrew (macOS and Linux)

```sh
brew install AppachiTech/kaval/kaval
```

Installs `kav`, its man page and shell completions for bash, zsh and fish.

### Prebuilt binaries

| Platform | Download directory |
|----------|--------------------|
| macOS, Apple Silicon and Intel (one universal binary) | `macos` |
| Linux x86_64 | `linux` |
| Linux arm64 (aarch64) | `linux-arm64` |

The Linux binaries are statically linked, so they run on any distribution regardless of its glibc version. Each tarball contains `kav`, the man page `kav.1`, `LICENSE` and `uninstall.sh`.

Download, verify the checksum, and install:

```sh
P=macos   # or: linux, linux-arm64
curl -fsSLO "https://downloads.appachi.tech/$P/kav-$P-latest.tar.gz"
echo "$(curl -fsSL "https://downloads.appachi.tech/$P/kav-$P-latest.tar.gz.sha256")  kav-$P-latest.tar.gz" | shasum -a 256 -c -
tar -xzf "kav-$P-latest.tar.gz"
sudo install -m 755 kav /usr/local/bin/kav
sudo mkdir -p /usr/local/share/man/man1 && sudo install -m 644 kav.1 /usr/local/share/man/man1/kav.1
```

On Linux without `shasum`, use `sha256sum -c -` instead. The check must print `OK`.

Specific versions live under `https://downloads.appachi.tech/<dir>/archive/kav-<dir>-v<version>.tar.gz` (with a `.sha256` beside each). The same tarballs and checksums are attached to every [GitHub release](https://github.com/AppachiTech/kaval/releases), so you can compare the checksum against a second source.

### Cargo

```sh
cargo install kaval --locked
```

Needs Rust 1.95 or newer.

### From source

```sh
git clone https://github.com/AppachiTech/kaval.git && cd kaval
cargo build --release --locked
sudo install -m 755 target/release/kav /usr/local/bin/kav
```

### Shell completions and man page (manual installs)

Homebrew sets these up for you. Otherwise:

```sh
# zsh: then add `fpath=(~/.zfunc $fpath)` before `compinit` in ~/.zshrc
mkdir -p ~/.zfunc && kav completions zsh > ~/.zfunc/_kav
# bash (with bash-completion installed)
mkdir -p ~/.local/share/bash-completion/completions && kav completions bash > ~/.local/share/bash-completion/completions/kav
# fish
mkdir -p ~/.config/fish/completions && kav completions fish > ~/.config/fish/completions/kav.fish
# read the man page without installing it
kav man > kav.1 && man ./kav.1
```

`kav completions` also supports `elvish` and `powershell`.

## Update and uninstall

| Installed with | Update | Uninstall |
|----------------|--------|-----------|
| Homebrew | `brew upgrade kaval` | `brew uninstall kaval` |
| Tarball | repeat the install steps (the `-latest` URLs always point to the newest release) | `./uninstall.sh` from the tarball (removes `/usr/local/bin/kav` and the man page) |
| Cargo | `cargo install kaval --locked` | `cargo uninstall kaval` |

## Usage

```sh
kav                          # Interactive TUI (prints the list instead when not in a terminal)
kav list                     # Table of every listening port
kav list --exposed           # Only listeners other machines can reach
kav list -f postgres         # Filter: exact port or PID, or text in process, service, command, project, container or user
kav list --sort cpu -r       # Sort by port, name, cpu or mem; -r reverses
kav list --watch             # Refresh in place every 2 s (Ctrl+C to exit)
kav list --json              # JSON (schema 2)
kav check 3000 5432          # What's on these ports? Command line, directory, parent, connections
kav check 3000 --probe       # …and whether it answers (TCP connect, HTTP for web ports)
kav check 3000 --free -q     # Exit 0 only if the port is free (pre-flight check)
kav kill 3000                # Stop whatever holds port 3000 and wait until it's free
kav kill 3000 --parent       # Stop its supervisor instead (nodemon, a reloader…)
kav wait 3000                # Block until something listens on 3000
kav wait 3000 --free         # Block until port 3000 is free
kav pick                     # Print a free port (default range 3000-9999)
kav completions zsh          # Shell completion script
kav man                      # Man page (roff) on stdout
```

### `kav list`

```
  PORT   PROTO BIND         PROCESS              SERVICE            PROJECT        PID     CPU    MEM      UPTIME
──────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  3000   TCP   localhost    node                 Next.js            shop           48211   1.2%   182 MB   2h 4m
  5432   TCP   localhost    postgres             PostgreSQL         ·              1093    0.0%   38 MB    6d 1h
  6379   TCP   *            com.docker.backend   cache-redis        ·              21743   0.4%   244 MB   3h 12m
```

- **BIND** is `localhost` (this machine only), `*` (all interfaces — reachable from other machines) or the specific address the socket is bound to.
- **SERVICE** names what kav detected; for Docker-published ports it is the container name. A trailing `?` means the service was guessed from the port number alone.
- **PROJECT** is the git repository the process runs in (`·` when there is none). `kav check` shows a container's image and compose project.
- Columns drop by priority on narrow terminals. Without root, kav adds a note when other users' listeners are hidden.

`--tcp` / `--udp` limit the protocol, `--interval` sets the `--watch` refresh, and `--watch --json` streams one JSON document per line.

### `kav check`

```
$ kav check 3000 8080
  PORT   PROTO BIND         PROCESS              SERVICE            PROJECT        PID     CPU    MEM      UPTIME
──────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  3000   TCP   localhost    node                 Next.js            shop           48211   1.2%   182 MB   2h 4m
    cmd:       node /Users/me/code/shop/node_modules/.bin/next dev
    dir:       ~/code/shop
    parent:    npm (PID 48190)
    clients:   2 — Google Chrome (PID 5120), node (PID 48230)
Nothing listening on port 8080

1 of 2 ports active
```

Without root, a port held by another user's process is still reported as in use (kav detects it with a loopback connect); run `sudo kav check` to see who owns it. `--probe` adds a health line (`probe: HTTP 200 in 3 ms`), `--json` prints a document, and `--quiet` prints nothing so only the exit code answers.

### `kav kill`

```
$ kav kill 3000
Send SIGTERM to node (PID 48211) on port 3000? [y/N] y
Stopped node (PID 48211) — port 3000 is free
```

kav sends the signal, then waits up to `--timeout` seconds (default 5) until the process has exited — "Stopped" means the port really is free. If something takes the port again, kav says so, along with the parent that restarted it.

| Flag | Effect |
|------|--------|
| `-s, --signal <SIG>` | Signal to send: `TERM` (default), `KILL`, `INT`, `HUP`, `QUIT`, `USR1`, `USR2` or a number |
| `-f, --force` | Shorthand for `--signal KILL` |
| `-y, --yes` | Don't ask for confirmation |
| `--timeout <SECS>` | How long to wait for each process to exit |
| `--escalate` | Send SIGKILL if the process is still running after `--timeout` |
| `--no-wait` | Send the signal and return immediately |
| `--parent` | Stop the listener's parent instead (a supervisor that would restart it) |
| `--allow-protected` | Allow stopping container runtimes and system services |
| `--json` | Report results as JSON; never prompts |

kav asks before each kill when run in a terminal. With `--yes`, with `--json`, or when stdin is not a terminal (scripts, CI) it goes ahead without asking.

Ports published by a Docker container are freed with `docker stop <container>`, never by killing the container runtime's port forwarder (which would stop every container). Init, kav itself, critical system processes, container runtimes and — inside an SSH session — `sshd` are refused; `--allow-protected` overrides all but init and kav. Before signalling, kav re-checks the process start time, so a PID reused by an unrelated process is never hit.

### `kav wait` and `kav pick`

```sh
npm run dev & kav wait 3000 --timeout 60 && npm test   # tests start once the server listens
kav kill 3000 --yes && kav wait 3000 --free            # be sure before restarting
PORT=$(kav pick --range 3000-3999) npm run dev         # a port nothing else is using
kav pick -n 3                                          # three free ports, one per line
```

`kav wait` polls every 250 ms without binding the ports, so it never races a server that is starting. `--timeout 0` waits forever; `--udp` waits for UDP ports.

## Exit codes

| Command | 0 | 1 | 2 | 3 |
|---------|---|---|---|---|
| `kav list` | success (even with no ports) | — | error | — |
| `kav check` | at least one port in use (`--free`: every port free) | no port in use (`--free`: a port is busy) | error | — |
| `kav kill` | the ports were freed (idle ports are ignored) | nothing was listening, or every prompt was declined | a kill failed or was refused (permission, protected process) | still running after `--timeout`, or the port was taken again |
| `kav wait` | the condition was met | timed out | error | — |
| `kav pick` | enough free ports found | not enough free ports in the range | error | — |
| `kav completions`, `kav man` | always | — | — | — |

## Scripting and AI agents

Everything the TUI shows is available to scripts and coding agents through plain commands:

- `--json` on `list`, `check` and `kill` prints a versioned document ([schema 2](#json-output-schema-2)); `list --watch --json` streams NDJSON.
- Exit codes answer the question without parsing (`kav check 5432 -q && echo "db is up"`).
- Nothing prompts when stdin is not a terminal, or with `--yes` / `--json`.
- `kav wait` and `kav pick` replace sleep loops and hard-coded ports.

```sh
kav check 5432 --json | jq -r '.ports[0].listeners[0].command | join(" ")'
kav list --json | jq -r '.ports[] | select(.exposed) | "\(.port) \(.process)"'
kav list --json | jq '.ports | map(select(.cpu > 10)) | sort_by(-.cpu)'
kav kill 3000 --yes --json | jq -r '.results[].outcome'
```

There is no MCP server on purpose: an agent that can run shell commands already gets everything from `kav … --json` and the exit codes above, with nothing extra to install or keep running.

## JSON output (schema 2)

Every document has `schema` (currently `2`) and `version` (kav's version). Fields may be added within a schema version; renaming or removing one bumps the schema.

### `kav list --json`

```json
{
  "schema": 2,
  "version": "0.2.0",
  "complete": true,
  "ports": [
    {
      "port": 3000,
      "protocol": "TCP",
      "addresses": ["127.0.0.1"],
      "exposed": false,
      "pid": 48211,
      "process": "node",
      "command": ["node", "/Users/me/code/shop/node_modules/.bin/next", "dev"],
      "cwd": "/Users/me/code/shop",
      "project": "shop",
      "user": "me",
      "parent": { "pid": 48190, "name": "npm" },
      "service": "Next.js",
      "service_source": "command",
      "category": "dev_server",
      "container": null,
      "cpu": 1.2,
      "memory_mb": 182.4,
      "uptime_secs": 7440,
      "connections": 2
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `complete` | bool | `false` when some listeners' owners are hidden (other users' processes); run with sudo to see all |
| `timestamp` | integer | Unix seconds; only in `--watch` frames |
| `port` | integer | Port number |
| `protocol` | `"TCP"` \| `"UDP"` | TCP and UDP on the same port are separate entries |
| `addresses` | string[] | Bound addresses (`0.0.0.0` / `::` mean all interfaces) |
| `exposed` | bool | Reachable from other machines (bound to all interfaces or a non-loopback address) |
| `pid` | integer \| null | Owning process; `null` when kav can't see it (another user's — try sudo) |
| `process` | string \| null | Process name |
| `command` | string[] | Full command line (empty when unavailable) |
| `cwd` | string \| null | Working directory |
| `project` | string \| null | Git repository the process runs in |
| `user` | string \| null | Owner of the process |
| `parent` | object \| null | `{ "pid", "name" }` of the parent process |
| `service` | string \| null | Detected service |
| `service_source` | string \| null | How it was detected: `command`, `process`, `port` (a guess) or `container` |
| `category` | string | `dev_server`, `database`, `cache`, `queue`, `container`, `browser`, `system` or `unknown` |
| `container` | object \| null | `{ "id", "name", "image", "compose_project" }` for Docker-published ports |
| `cpu` | number | CPU usage, percent of one core |
| `memory_mb` | number | Resident memory in megabytes |
| `uptime_secs` | integer | Process uptime in seconds |
| `connections` | integer \| null | Established connections to the port (TCP only) |

### `kav check --json`

`{ "schema", "version", "ports": [ … ] }` with one entry per requested port:

| Field | Description |
|-------|-------------|
| `port` | The requested port |
| `in_use` | Whether anything listens on it |
| `owner_visible` | `false` when it is in use by a process kav can't see |
| `listeners` | Listener objects, as in `kav list` |
| `probe` | With `--probe`: `{ "result", "http_status", "latency_ms" }`; `result` is `http`, `tcp_open`, `refused`, `timeout` or `unsupported` |

### `kav kill --json`

`{ "schema", "version", "results": [ … ] }` with one entry per process or container acted on:

| Field | Description |
|-------|-------------|
| `ports` | Ports it held |
| `pid`, `process` | The process (`null` for a port nothing listened on) |
| `action` | `signal`, `docker_stop` or `none` |
| `signal` | e.g. `SIGTERM`, when a signal was sent |
| `outcome` | `exited`, `signal_sent` (`--no-wait`), `stopped` (container), `still_running`, `respawned`, `failed`, `refused`, `declined` or `not_found` |
| `message` | Why, when there is more to say |

### Migrating from 0.1

`kav list --json` used to print a bare array. It is now an object, so `jq '.[]'` becomes `jq '.ports[]'`. `process` is `null` for listeners whose owner is hidden, and TCP and UDP listeners on the same port are separate entries.

## Interactive TUI

Run `kav`. The table refreshes every 2 seconds (`--interval` to change). Press `?` for help.

| Key | Action |
|-----|--------|
| `↑/↓` `j/k`, mouse wheel | Move the selection (click selects a row) |
| `PgUp` / `PgDn`, `Home` / `End` | Page, jump to top or bottom |
| `/` | Filter by port, PID, name, service, command, project, container or user (`Enter` keeps it, `Esc` clears it) |
| `Esc` | Clear the filter, then the marks |
| `Enter` | Expand or collapse a browser's group of ports |
| `Space` | Mark rows for a multi-row kill |
| `Ctrl+X` | Stop the selected (or marked) processes with SIGTERM, after confirmation |
| `Ctrl+K` | Force kill (SIGKILL), after confirmation |
| `e` | Show only listeners exposed to the network |
| `d` / `Ctrl+D` | Toggle the detail pane; `[` `]` or `←` `→` scroll it |
| `s` / `Ctrl+S` | Cycle the sort: Port → Name → CPU → Mem; `S` reverses it |
| `t` / `Ctrl+T` | Cycle protocols: TCP+UDP → TCP → UDP |
| `r` / `Ctrl+R` | Refresh now |
| `y` | Copy… then `y`/`p` port, `i` PID, `u` URL, `c` command line, `d` working directory |
| `o` | Open in the browser (https for TLS ports; refuses UDP and databases) |
| `p` | Probe the listener: HTTP status or TCP accept, with latency |
| `Ctrl+Z` | Suspend (resume with `fg`) |
| `q` / `Ctrl+Q` / `Ctrl+C` | Quit |

In the kill dialog, `y` confirms and — when the listener has a supervising parent — `p` stops the parent instead; any other key cancels. After a kill the status line reports when the port is actually free, or that the process is still running.

The detail pane shows the bind address and whether it is reachable from other machines, the user, project and directory, parent, container, connections, notes (for example, about AirPlay) and the last probe result. Rows that appear while kav is running are marked `+` for ten seconds, and a new listener exposed to the network is announced in the status line.

`--no-mouse` leaves the mouse to your terminal (for selecting text).

## Themes and colour

| Setting | Values |
|---------|--------|
| `--theme`, or `KAV_THEME` | `auto` (default), `dark`, `light` |
| `--color` | `auto` (default; colour only on terminals), `always`, `never` |
| `NO_COLOR` | Set (to anything non-empty): no colours, in tables and in the TUI |
| `CLICOLOR_FORCE` | Set to a non-zero value: colour even when piped (`NO_COLOR` wins) |

`auto` picks the theme from `COLORFGBG` if your terminal sets it, otherwise asks the terminal for its background colour, otherwise uses dark. Terminals without true colour get the nearest 256-colour palette.

## Privacy

- **Nothing is stored.** Kaval writes no files: no config, no logs, no history, no cache.
- **No telemetry.** No analytics, crash reports or update checks.
- **Network: this machine only.** Kaval never connects to the internet. It makes TCP connections only to this machine's own addresses: to tell whether a port is in use when its owner is hidden from you (`check`, `kill`, `wait` without root), and to probe listeners when you ask for it (`kav check --probe`, `p` in the TUI), which sends `HEAD /` to web ports. `kav pick` briefly binds candidate ports to make sure they are free.
- **Docker.** To name containers, kav runs `docker ps`; `docker stop` runs only when you stop a container's port. These talk to whichever daemon your Docker CLI is configured for.
- **TUI helpers.** Copying uses `pbcopy`, `wl-copy`, `xclip` or `xsel` (or the terminal's OSC 52 clipboard); `o` opens your browser with `open` or `xdg-open`.

## Troubleshooting

**Some ports have no owner, or a note says other users' processes are hidden.** macOS shows you only your own processes' sockets, and Linux hides other users' process details. Run `sudo kav` (or `sudo kav list`) to see everything. Without sudo, `kav check` still reports such ports as in use.

**Something on port 5000 or 7000 keeps coming back (macOS).** That's the AirPlay Receiver (`ControlCenter`), which macOS restarts when killed. Turn it off in System Settings → General → AirDrop & Handoff → AirPlay Receiver, or use another port.

**Docker ports show `com.docker.backend`, `vpnkit` or `docker-proxy`** (or OrbStack, Lima, Podman's `gvproxy`). Those processes forward ports for every container. When the `docker` CLI is available, kav names the container that publishes each port, and `kav kill` stops that container rather than the runtime.

**The port is taken again right after `kav kill`.** A supervisor (nodemon, a framework's reloader, a process manager) restarted the server. kav names it; stop it with `kav kill <port> --parent`, or press `p` in the TUI's kill dialog.

**"still running" after `kav kill`.** The process ignored SIGTERM. Use `--escalate` to follow up with SIGKILL, or `-f` to send SIGKILL right away.

**macOS says kav "cannot be opened".** Files downloaded with a browser are quarantined; `curl` downloads are not. Run `xattr -d com.apple.quarantine /usr/local/bin/kav`.

**Wrong colours.** Force a theme with `--theme light` (or `KAV_THEME=light`), or turn colour off with `--color never` / `NO_COLOR=1`.

**`man kav` finds nothing after a manual install.** Install `kav.1` as shown above, or read it directly with `kav man > kav.1 && man ./kav.1`.

## Links

- [Website](https://www.appachi.tech/kaval/)
- [FAQ](https://www.appachi.tech/kaval#faq)
- [Privacy Policy](https://www.appachi.tech/kaval/privacy/)
- [Contributing](CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)
- [Security Policy](SECURITY.md)

## License

MIT — [Madhubalan Appachi](https://www.linkedin.com/in/madhuappachi/)
