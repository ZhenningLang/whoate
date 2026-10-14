# whoate 设计文档

> 状态：草案 v1，2026-10-14。owner 已定：项目名 whoate、MIT、Rust 核心 + SwiftUI 界面 + CLI，只做 macOS。

## 1. 要解决的问题

活动监视器、htop 回答「哪个进程大」，不回答「这是谁的」。

起因：2026-10-14 一个 `node` 占了 9.7G。活动监视器里只看得到 `node`。真实身份要查 9 次命令才拼出来：一个前端项目的 wrangler 预览服务器，04:52 由一个已关闭的 tmux pane 启动，父进程已退出，无人管理 12 小时。

whoate 把这条拼图做成一次调用：

- 人打开 GUI，第一屏就看到「谁占得最多、它是谁、要不要管」。
- coding agent 调 CLI，几十毫秒拿到结构化 JSON，不花 token、不用多轮推理。

## 2. 目标与非目标

目标：

- 按「归属」而不是按进程汇总内存和 CPU。归属是人认得出的东西：tmux pane 与其中的 agent 会话、应用、孤儿进程、系统服务。
- 每个归属结论附证据链，按可信度排序，能一眼看懂为什么。
- 规则生成一句人话诊断和建议动作，不调模型。
- CLI 对 agent 友好：版本化 JSON、稳定退出码、非 TTY 无颜色。
- GUI 好看：菜单栏常驻 + 主窗口，SwiftUI 原生质感。

非目标（v1 不做）：

- Linux / Windows。采集层留接口。
- 长期历史库、告警推送。只保留会话内最近 30 分钟内存曲线。
- 需要 root 的深度取证（`lsmp`、`footprint` 他人进程）。
- Codex、Cursor 等其它 agent 的会话识别（留扩展点）。
- 私有 API（如 responsible pid）。XPC 服务按父进程链归到 launchd，标为「系统服务」。

## 3. 核心概念

### 3.1 进程快照 `Snapshot`

一次采样的纯数据：每个进程的 pid、ppid、启动时间、用户、可执行名、命令行（截断）、内存（phys footprint，含压缩部分，与活动监视器「内存」列同口径）、CPU 时间累计、工作目录、`TMUX_PANE` 环境变量（可读时）。另含系统总量：总内存、已用、压缩、wired、swap、内存压力等级。

归因和诊断只吃 `Snapshot`，不碰系统调用。这样能用录下来的真实快照做回放测试。

### 3.2 归属 `Owner`

| kind | 含义 | 例 |
|:---|:---|:---|
| `tmux_pane` | 某个仍存在的 tmux pane 下的进程树 | `main:15.1 [新2]` |
| `agent_session` | pane 内的 coding agent 会话，挂在 pane 下作为细分 | Claude `oss-atlas-51` |
| `app` | 由 launchd 拉起的 .app 及其 Helper | Google Chrome |
| `orphan` | 父进程链断到 launchd，但有证据表明它来自终端（`TMUX_PANE` 指向已关闭 pane，或可执行是 node/python/bun 等开发运行时且 cwd 在用户目录） | wrangler dev :8799 |
| `service` | launchd 拉起的系统或第三方后台服务 | mds、mysqld |
| `system` | root / 系统账号进程且无更好归属 | WindowServer |

### 3.3 归因证据 `Evidence`

每个进程的归属由一串证据决定，按可信度从高到低：

1. **tmux 进程链**：祖先进程是某个 pane 的 shell。确定。
2. **agent 会话文件**：Claude Code 的 `~/.claude/sessions/<pid>.json` 直接写了 pid → sessionId、name、cwd、tmux 位置、status。确定。
3. **`TMUX_PANE` 环境变量**：经 `KERN_PROCARGS2` 读取，父进程退出后仍保留；pane 已关闭时归为孤儿并注明来源 pane。确定（但 node 改写 argv 时可能读不到）。
4. **目录 + 时间配对**：opencode / kilo 的会话库没有 pid，用「同目录、进程启动后 10 秒内创建的根会话」配对。推断。
5. **工作目录**：无其他线索时，cwd 指出项目。弱。
6. **可执行所在 .app 包**：归到应用。确定。

GUI 和 CLI 都输出证据链与 `confidence: certain | inferred | weak`。

### 3.4 agent 会话信息

| 字段 | Claude Code | opencode | kilo |
|:---|:---|:---|:---|
| 会话 ID | sessions/<pid>.json | session_v2.id | session.id |
| 会话名 | `name`（derived 或用户设定） | `title` | `title` |
| 自动标题 | transcript 中 `ai-title` 记录 | 同 title | 同 title |
| 第一个用户问题 | transcript 第一条非系统 user 消息 | session_message 首条 user | 同左 |
| 状态 | `status`：idle / waiting / busy / shell | `time_updated` 距今 | 同左 |
| 目录、分支 | `cwd`、transcript `gitBranch` | `directory` | `directory` |
| 模型、花费 | 无 | `model`、`cost` | 同左 |
| 数据位置 | `~/.claude/sessions/`、`~/.claude/projects/<slug>/<id>.jsonl` | `~/.local/share/opencode/opencode.db` | `~/.local/share/kilo/kilo.db` |

读取规则：只读；SQLite 用 `mode=ro` 打开并容忍锁；transcript 只读前 64KB 找首问、倒读末尾找 ai-title；第一个问题截断到 200 字符，去掉 `<system-reminder>` 等包裹块。会话数据缓存，按文件 mtime 失效。

隐私：首问可能含敏感内容。GUI 默认显示；CLI JSON 默认包含，`--redact` 去掉首问与命令行参数。

### 3.5 诊断 `Verdict`

规则表（数据驱动，`verdict/rules.rs` 一张表）。每条规则：条件 → 等级（`ok` / `watch` / `act`）→ 一句中文/英文文案 → 建议动作。

v1 规则：

| 规则 | 条件 | 等级 | 文案示例 |
|:---|:---|:---|:---|
| 孤儿开发服务 | orphan 且监听端口且 0 连接 | act | 无人管理的开发服务器，建议关闭 |
| 孤儿大户 | orphan 且内存 ≥ 1G | act | 启动它的 pane 已关闭 11 小时，占 9.7G |
| 快速上涨 | 最近 10 分钟内存 +≥ 500M 且仍在涨 | watch | 近 10 分钟 +620M，还在涨 |
| 持续上涨 | 30 分钟单调上涨 ≥ 1G | act | 30 分钟只涨不降，疑似泄漏 |
| 空转 agent | agent 会话 idle ≥ 2h 且内存 ≥ 500M | watch | 空闲 3 小时，可关 |
| 系统进程 | system / service 中的 Apple 进程 | ok | 系统进程，勿关 |
| 正常 | 其余 | ok | 正常 |

CLI 只看单次快照时，依赖曲线的规则跳过；`whoate watch` 与 GUI 有曲线。

## 4. 采集

- **自己账号的进程**：libproc。`proc_listallpids` + `proc_pid_rusage(RUSAGE_INFO_V4)` 取 `ri_phys_footprint` 与 CPU 时间，`proc_pidinfo(PROC_PIDTBSDINFO)` 取 ppid/启动时间/uid，`PROC_PIDVNODEPATHINFO` 取 cwd，`sysctl KERN_PROCARGS2` 取命令行与环境。本机实测一遍 702 个进程 4ms。
- **其他账号的进程**（root、`_` 系统账号，本机 904 个里 202 个）：libproc 读不到内存。后台每 6 秒跑一次 `top -l 1 -stats pid,mem,cpu` 补齐（单次 2.5–3.6s），结果带时间戳；ppid、命令名仍用 `sysctl KERN_PROC_ALL`（不受限）。
- **CPU%**：两次采样的 CPU 时间差 / 墙钟差。CLI 单次调用默认采样两次间隔 200ms。
- **系统总量**：`host_statistics64(HOST_VM_INFO64)`、`sysctl vm.swapusage`、`kern.memorystatus_vm_pressure_level`。
- **tmux**：`tmux list-panes -a -F` 一次拿全部 pane（id、session:window.pane、窗口名、pane pid、cwd）。无 tmux 时跳过。
- **端口**：`proc_pidfdinfo` 枚举 socket，取监听端口与已建立连接数（仅孤儿候选，控制开销）。

性能预算：GUI 每秒刷新一次快采，CPU 占用 < 1%（单核）；CLI `top --json` 冷启动 < 300ms（不等 top 慢采，用缓存或标记 `system_procs: stale`）。

慢采缓存：CLI 与 GUI 共用 `~/Library/Caches/whoate/system.json`（≤ 30s 视为新鲜）。`--fresh` 强制现跑 top。

## 5. CLI

```
whoate                          # 等同 whoate top
whoate top [--by mem|cpu] [-n 10] [--json] [--fresh] [--redact]
whoate why <pid> [--json]       # 证据链 + 诊断
whoate orphans [--json]
whoate pane <target> [--json]   # target: %12 | main:15.1 | 会话 ID 前缀
whoate agents [--json]          # 所有 agent 会话及其资源
whoate kill <pid|owner-id> [--dry-run] [--yes]
whoate watch [--interval 2s] [--json-lines]
whoate agent-guide              # 打印给 agent 的用法说明
whoate --version
```

约定：

- `--json` 输出单个对象，顶层 `schema: "whoate/v1"`、`taken_at`、`host`、`owners[]`；字段只增不改，破坏性变更升 `v2`。
- 退出码：0 正常；1 存在 `act` 级诊断；2 参数错误；3 目标不存在；4 权限不足。
- 非 TTY 或 `NO_COLOR` 时无颜色、无框线。
- `kill` 默认需要 `--yes` 或交互确认；拒绝杀系统进程与 Apple 签名进程；先 SIGTERM，5 秒后不退再提示是否 SIGKILL。
- `agent-guide` 输出一段 Markdown，`skills/whoate/SKILL.md` 内容相同，可装进 Claude Code / kilo / opencode。

## 6. GUI（SwiftUI）

形态：菜单栏常驻 + 主窗口。

### 6.1 菜单栏

图标是 logo（被咬一口的内存条）单色版，压力偏紧时咬口变黄，告急变红。点击弹出面板：

- 顶部：内存压力条（已用 / 压缩 / swap 三段）和一句结论，如「偏紧：swap 20.4G。最大头是一个无人管理的开发服务器（9.7G）」。
- 前 5 个归属，每行：图标、名称、一句诊断、内存数字、迷你曲线。`act` 级行带「关闭」按钮。
- 底部：「打开 whoate」。

### 6.2 主窗口

三栏：

- **左栏 · 归属分组**：全部 / tmux / agent 会话 / 应用 / 孤儿 / 系统，每组带合计。
- **中栏 · 归属列表**：可按内存 / CPU 排序；每行是一张紧凑卡片：标题（agent 会话名或应用名）、副标题（项目目录、tmux 位置）、诊断胶囊、内存与 CPU 数字、最近 30 分钟曲线。可展开看进程树。
- **右栏 · 详情**：
  - agent 会话：会话名、自动标题、第一个问题（引用样式）、状态、目录与分支、模型与花费、tmux 位置（按钮：在终端里跳到这个 pane）。
  - 证据链：每条证据一行，图标表示可信度。
  - 内存曲线（Swift Charts）。
  - 动作：关闭进程（确认弹窗，列出将被关闭的 pid）、复制 `whoate why <pid>`、在 Finder 显示 cwd。

视觉：遵循 macOS 原生控件与 SF Symbols；配色只用语义色表达状态（act 橙、watch 黄、归属线索青），其余跟随系统明暗模式。数字等宽。

### 6.3 GUI 与 Rust 的接口

Rust 编译为静态库 `libwhoate_ffi.a`，导出少量 C 函数：

```c
WhoateEngine* whoate_engine_new(void);
char* whoate_engine_tick(WhoateEngine*);        // 返回 whoate/v1 JSON（与 CLI 同一结构）
char* whoate_why(WhoateEngine*, int32_t pid);
int32_t whoate_kill(int32_t pid, int32_t force, char** err);
void whoate_string_free(char*);
void whoate_engine_free(WhoateEngine*);
```

Swift 侧用 `Codable` 解码。CLI 与 GUI 共用一个 JSON 合同，Rust 端有 JSON schema 快照测试。曲线由引擎内部的环形缓冲维护（30 分钟 × 1 秒，按 owner 聚合）。

构建只依赖 Command Line Tools（已验证 `swiftc` 能编 SwiftUI + Charts + MenuBarExtra）：SwiftPM 可执行目标 → 脚本打成 `.app`（Info.plist、图标、ad-hoc 签名）。

## 7. 仓库结构

```
whoate/
├ Cargo.toml                    workspace
├ crates/
│ ├ whoate-core/                采集 + 归因 + 诊断，纯库
│ │ ├ src/sample/               libproc、sysctl、top 慢采、tmux、端口
│ │ ├ src/sources/              claude_code、opencode、kilo
│ │ ├ src/attribute/            Snapshot → Owners + Evidence
│ │ ├ src/verdict/              规则表
│ │ └ src/model.rs              whoate/v1 数据结构（serde）
│ ├ whoate-cli/                 clap 子命令、渲染、退出码
│ └ whoate-ffi/                 C ABI 静态库
├ app/                          SwiftPM：WhoateApp（SwiftUI）
├ skills/whoate/SKILL.md
├ site/                         产品介绍页（静态 HTML）
├ assets/                       logo（SVG、icns、菜单栏模板图）
├ scripts/                      bundle-app.sh、release.sh、record-fixture.sh
├ tests/fixtures/               录制的真实快照（脱敏）
└ docs/design.md
```

## 8. 测试与验收

- **回放测试**：`scripts/record-fixture.sh` 把本机快照录成 JSON（命令行与路径脱敏）。归因与诊断用 fixture 跑，断言具体归属和证据。必须包含：孤儿 wrangler（TMUX_PANE 指向已关 pane）、tmux 内 Claude 会话、opencode 目录时间配对、Chrome Helper 归应用、root 进程归系统。
- **合同测试**：`whoate/v1` JSON 快照测试；Swift 端对同一 fixture 解码测试。
- **真机冒烟**：`whoate top --json` 在本机 < 300ms；`whoate why <本会话 claude pid>` 返回正确会话名；起一个孤儿 `python3 -m http.server` 关掉其 pane，`whoate orphans` 能抓到并给出来源 pane。
- **GUI**：截图验收菜单栏面板与主窗口（明暗两种），检查文字不溢出、1 秒刷新不闪。
- **CI**：GitHub Actions macOS runner 跑 `cargo test`、`cargo clippy -D warnings`、`swift build`。

## 9. 分阶段

| 阶段 | 内容 | 验收 |
|:---|:---|:---|
| P1 采集 | workspace、model、libproc/sysctl/top/tmux 采集、fixture 录制 | 本机快照与 `top` MEM 列一致（抽 10 个误差 < 1%） |
| P2 归因与诊断 | 证据链、三种 agent 来源、规则表 | fixture 回放全绿，含第 8 节五个场景 |
| P3 CLI | 全部子命令、JSON 合同、退出码、kill 安全、agent-guide 与 SKILL.md | 真机冒烟三项 |
| P4 GUI | FFI、SwiftUI 菜单栏 + 主窗口、曲线、打包 .app | 截图验收、1s 刷新 CPU < 1% |
| P5 发布与门面 | logo 定稿、README、Release（CLI 二进制 + .app zip）、Homebrew tap、产品介绍页 | 新机器按 README 一行装好；介绍页可访问 |
