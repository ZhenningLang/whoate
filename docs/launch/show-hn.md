> 草稿，未发布 (DRAFT, not posted). The owner reviews and posts it; nothing here is automated.

## 标题

Show HN: Whoate – which agent session or tmux pane is eating your Mac's memory

## 正文

I run several coding agents (Claude Code, Codex and friends) in tmux panes all
day. When my Mac starts swapping, Activity Monitor shows me a wall of `node`,
`python` and `Helper (Renderer)` processes. It can't tell me which of them
belong to the agent I forgot in pane 3, which ones are MCP servers, or which
ones a terminal I already closed left running.

whoate groups processes by who started them: a coding-agent session (with its
MCP servers and the tmux pane it runs in), an app with all its helpers, a swarm
of identical commands, or an orphan from a closed terminal. It explains each
piece of software in its own terms, for example Chrome's tab and extension
processes or Docker's VM. Advice comes from a small knowledge base where every
entry cites a source and the date it was checked.

How it differs from Activity Monitor:

- Owners instead of process names: session, tmux pane, app.
- It finds orphans that closed terminals left behind.
- A CLI made for coding agents: `whoate top --json`, `whoate why <pid>` and
  `whoate agent-guide` give your agent structured JSON and exit codes. The
  diagnosis is rule-based on your machine, so it spends zero tokens.

There is a menu bar app (SwiftUI) and the CLI (Rust).

```sh
brew install zhenninglang/tap/whoate
```

Privacy: whoate only reads local process and session information. No network,
no telemetry, no model calls, and it never changes another app's settings.

Known limits: Apple Silicon and macOS 14+ only. The app is ad-hoc signed and not
notarized, so the first launch needs System Settings → Privacy & Security → Open
Anyway (the README has the steps).

Code: https://github.com/ZhenningLang/whoate
Page: https://zhenninglang.github.io/whoate/

I would love to hear which software you'd like it to understand next.

## 配图清单

- `site/img/demo.gif`: the 12-second demo (demo data only)
- `site/img/main-agent-en-light.png`: an agent session, its tmux pane and MCP servers
- `assets/social/preview.png`: link preview card
