# Changelog

All notable changes to Kaval will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-02-10

### Added
- Interactive TUI for viewing all listening TCP and UDP ports
- Smart service detection for 30+ dev tools (PostgreSQL, Redis, Next.js, Vite, Docker, Nginx, etc.)
- One-key kill with SIGTERM (x) and SIGKILL (K), with confirmation prompt
- Detail pane showing full process info (command line, PID, CPU, memory, uptime)
- Filter mode to search by port, process name, or service
- Sort cycling: Port, Process Name, CPU, Memory
- TCP/UDP protocol filter toggle
- CLI commands: `kav list`, `kav check <port>`, `kav kill <port>`
- JSON output for scripting (`kav list --json`)
- Color-coded categories (dev servers, databases, caches, containers, infrastructure)
- Cross-platform support: macOS (Intel + Apple Silicon) and Linux (x86_64)
- Homebrew tap installation
