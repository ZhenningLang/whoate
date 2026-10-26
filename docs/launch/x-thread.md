> 草稿，未发布 (DRAFT, not posted). The owner reviews and posts it; nothing here is automated.

## 标题

whoate launch thread (X)

## 正文

### 1/

My Mac was swapping and Activity Monitor showed a wall of `node` processes. Which agent
session started them? Which tmux pane? Which were left by a terminal I closed?

So I built whoate: memory grouped by who started it. 🧵

[attach site/img/demo.gif]

### 2/

whoate groups processes by owner: a coding-agent session with its MCP servers,
the tmux pane it runs in, an app with all its helpers, or an orphan from a closed
terminal.

### 3/

It knows how software splits into processes: Chrome's tab and extension
processes, Docker's VM, an IDE's language servers. Advice comes from a knowledge
base where every entry cites its source.

### 4/

The part I use most: your coding agent can ask too.

whoate top --json
whoate why <pid>
whoate agent-guide

Structured JSON and exit codes. The diagnosis runs locally by rules: zero tokens.

### 5/

Menu bar app + CLI, Apple Silicon, macOS 14+. No network, no telemetry.

brew install zhenninglang/tap/whoate

https://github.com/ZhenningLang/whoate

## 配图清单

- `site/img/demo.gif`: post 1
- `site/img/main-agent-en-light.png`: post 2
- `site/img/main-browser-en-light.png`: post 3
- `site/img/settings-cli-en-light.png`: post 4
