# Contributing to Kaval

Thanks for your interest in contributing!

## Development Setup

Kaval builds with Rust 1.95 or newer on macOS and Linux.

```bash
# Clone the repo
git clone https://github.com/AppachiTech/kaval.git
cd kaval

# Build
cargo build

# Run
cargo run              # interactive TUI
cargo run -- list      # any subcommand
```

## Testing & Linting

```bash
# Run tests (unit, end-to-end and pseudo-terminal TUI tests)
make test

# Run lints (format check + clippy, tests included)
make lint
```

Some information is only visible to root (other users' processes). Tests don't need it, but `sudo cargo run -- list`
is handy for checking those code paths by hand.

## Before Submitting a PR

1. Run `make lint && make test` — both must pass
2. Keep commits focused and descriptive
3. Update CHANGELOG.md under `[Unreleased]` if adding user-facing changes
4. JSON field names are part of kav's scripting contract: you may add fields, but renaming or removing one
   needs a schema bump (`SCHEMA` in `src/output/json.rs`) and a CHANGELOG note

## Project Structure

```
src/
  main.rs          # Entry point, CLI dispatch, exit codes
  cli.rs           # clap command definitions
  cmd/             # One module per subcommand: list, check, kill, wait, pick
  scanner.rs       # Listening sockets → PortEntry rows (processes, parents, connections, projects, containers)
  models.rs        # Data structures (PortEntry, Protocol, Service, Exposure, sorting and filtering)
  services.rs      # Known-service detection (COMMAND_RULES / PROCESS_RULES / PORT_RULES)
  project.rs       # Git project of a working directory
  docker.rs        # Container attribution (docker ps) and docker stop
  probe.rs         # Loopback connect, bind check and health probe
  killer.rs        # Kill planning: protection rules, parent targeting, signals, waiting for exit
  output/          # Tables (table.rs), JSON schema 2 (json.rs), colour and sanitising (mod.rs)
  theme.rs         # Dark, light and mono themes, background detection
  ui/              # Interactive TUI: event loop (mod.rs), state (app.rs), rendering (draw.rs), clipboard
tests/
  cli.rs           # End-to-end tests running the real binary
  tui.rs           # TUI tests through a pseudo-terminal
  common/mod.rs    # Helper listener processes for the tests
scripts/
  package.sh       # Release tarballs (used by CI and `make package`)
  homebrew-formula.sh
  uninstall.sh     # Shipped in the tarballs
```

## Adding a New Service

See the doc comments at the top of `src/services.rs` and above each rule table. In short:

- If the service runs on a generic runtime (`node`, `python`, `java`, …), add a `COMMAND_RULES` entry that
  matches its command line, e.g. `Cmd::Bin("vite")` or `Cmd::Then("manage", "runserver")`.
- If it names its own process (`postgres`, `redis-server`), add a `PROCESS_RULES` entry.
- Only if neither works, add its default port to `PORT_RULES` — port matches are shown as guesses (`?`).

Add a test next to the existing ones in `src/services.rs` for the new rule.

## Reporting Issues

Use [GitHub Issues](https://github.com/AppachiTech/kaval/issues) with the provided templates.
