# Changelog

All notable changes to whoate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-30

The first release. Apple Silicon, macOS 14 (Sonoma) or later.

### Added

- **Ownership, not process names.** Memory and CPU are grouped by who started the
  processes: a coding-agent session, the tmux pane it runs in, an app (with every
  helper it launched), or an orphan that a closed terminal left behind. Many copies
  of one command are grouped as a swarm.
- **Per-software recognition.** Coding agents and their MCP servers, development
  runtimes, Chromium browsers (tab, extension and GPU processes), WebKit, Docker
  and IDE language servers are explained in each tool's own terms.
- **Knowledge base.** Advice comes from built-in entries that each cite a source
  and the date it was checked; `whoate insights --list` prints them all. Your own
  rules go in `~/.config/whoate/rules.toml`.
- **System memory insight.** System services, kernel and wired memory, and an
  estimate of how much physical memory quitting an owner would free.
- **CLI with `whoate/v1` JSON.** `top`, `why`, `orphans`, `agents`, `pane`,
  `swarms`, `watch`, `kill` and `insights`, with a published JSON Schema and
  meaningful exit codes. `whoate agent-guide` prints the instructions for a coding
  agent. Diagnosis is rule-based on the machine: no model calls, no network.
- **Safe kill.** Root processes, Apple platform binaries and whoate itself are
  never killed; `--dry-run` previews, and nothing is killed without `--yes`.
- **Menu bar app.** A panel ranked by memory or CPU, and a main window with
  categories (AI & Agent, apps, orphans and more), details, process trees,
  history curves and a jump to the tmux pane.
- **Settings window.** Launch at login, appearance, interface language
  (Chinese or English), menu bar display and refresh rate, notifications when
  memory gets tight, installing the bundled CLI, and About.

[0.1.0]: https://github.com/ZhenningLang/whoate/releases/tag/v0.1.0
