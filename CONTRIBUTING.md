# Contributing to Kaval

Thanks for your interest in contributing!

## Development Setup

```bash
# Clone the repo
git clone https://github.com/AppachiTech/kaval.git
cd kaval

# Build
cargo build

# Run
cargo run
```

## Testing & Linting

```bash
# Run tests
make test

# Run lints (format check + clippy)
make lint
```

## Before Submitting a PR

1. Run `make lint && make test` — both must pass
2. Keep commits focused and descriptive
3. Update CHANGELOG.md under `[Unreleased]` if adding user-facing changes

## Project Structure

```
src/
  main.rs      # Entry point, CLI dispatch
  cli.rs       # clap command definitions
  models.rs    # Data structures (PortEntry, Protocol, SortField, etc.)
  scanner.rs   # Port/process scanning logic
  output.rs    # CLI table and JSON rendering
  theme.rs     # TUI color theme
  ui.rs        # Interactive TUI (ratatui)
  util.rs      # Known service detection (PROCESS_RULES / PORT_RULES)
```

## Adding a New Service

See the contributor doc block above `PROCESS_RULES` in `src/util.rs` for how to add
a new service detection rule (match variants, priority, case sensitivity).

## Reporting Issues

Use [GitHub Issues](https://github.com/AppachiTech/kaval/issues) with the provided templates.
