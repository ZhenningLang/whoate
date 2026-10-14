Audience:   P3 实现 agent
Scope:      whoate v1：macOS 上按归属（tmux pane、agent 会话、应用、孤儿、系统）汇总内存与 CPU，Rust 核心 + SwiftUI 界面 + 给 agent 用的 CLI / Non-scope: Linux/Windows、长期历史库、需要 root 的深度取证
State:      in-review
Genre:      G-SPEC
Code-Root:  /Users/zhenninglang/Projects/whoate-docs-design
Prefixes:   U-DEC, U-BEH, U-GATE, U-SPK, U-ACC, U-NG, U-DEP, U-DIAG

# whoate · G-SPEC 机器版 spec

Planned at: 6622904 · 2026-10-14（仓库此时只有 README、LICENSE 与设计草案，没有代码）

## 术语表

- U-DEC: 待决策
- U-BEH: 有意行为变化
- U-GATE: 不可逆动作（单向门）
- U-SPK: 翻车假设与 spike
- U-ACC: 验收承诺
- U-NG: non-goals
- U-DEP: 新增依赖
- U-DIAG: 根因与现状诊断
- 归属（Owner）：人认得出的资源主人，一个进程恰好记在一个归属下。种类见「核心概念」。
- 归因证据（Evidence）：把进程归到某个归属的一条理由，带可信度 certain / inferred / weak。
- 诊断（Verdict）：规则表给一个归属打的等级 ok / watch / act，加一句话和建议动作。
- 孤儿：父进程链断到 launchd、但有证据表明它来自终端的进程。
- 快采 / 慢采：快采用 libproc 读本账号进程（毫秒级）；慢采用 `top` 补齐其他账号进程（秒级，后台跑）。
- footprint：macOS 的 phys footprint，含压缩与换出部分，与活动监视器「内存」列同口径。
- whoate/v1：CLI `--json` 与 GUI 共用的 JSON 合同版本号。

## TL;DR

做一个 Rust 写的进程归因引擎，同时喂给 SwiftUI 菜单栏应用和一个对 agent 友好的 CLI：一次调用返回「谁占得最多、它是谁、证据是什么、要不要管」，几十毫秒、零 token。分五期：采集、归因与诊断、CLI、GUI、发布与介绍页，走 dev-long-run。

## 现状诊断

[U-DIAG-1|a758] 2026-10-14 本机一个 node 进程 footprint 9.7G（top MEM 列 9727M），活动监视器与 htop 只显示「node」。查清它是谁用了 9 次工具调用：它是某前端项目的 `wrangler dev --port 8799`，04:52 由一个已关闭的 tmux pane 启动，父进程 npm exec 已被 launchd 收养，无人管理近 12 小时。现成工具（活动监视器、htop/btop、Mac Performance Monitor、Waterline）都不按 tmux pane 或 agent 会话归组，也不追孤儿出处（README 层面核对，未逐个安装实测）。

[U-DIAG-2|7070] 本机已有的 dotfiles `mem-snapshot report` 也漏掉了它：报告只按「窗口内涨幅」排序，该进程 04:52–06:30 从 3.3G 涨到 9.7G，之后持平，12 小时窗口从 04:57 起算，窗口内涨幅约 0，未进榜。缺「现在谁最大」与「启动初期暴涨」两个视角。

## 已锁定

- 名字 whoate，GitHub 公开仓库 `ZhenningLang/whoate`，MIT（owner 2026-10-14）。
- Rust 核心 + SwiftUI GUI（菜单栏 + 主窗口）+ CLI；去掉 TUI；只做 macOS（owner 2026-10-14）。
- 否决 Tauri（owner 选 SwiftUI）；否决名字 OpenMonitor（同名仓库 46 个，含同类监控）。
- CLI 对 coding agent 友好是核心卖点；GUI 要好看（owner 2026-10-14）。
- agent 会话要定位到会话名、自动标题、第一个用户问题（owner 2026-10-14）。
- 诊断由规则生成，不调模型。
- 开发路线 dev-long-run（owner 2026-10-14）。

## 核心概念

- **归属**：每个进程恰好记在一个归属下，资源只计一次。归属种类：`tmux_pane`（仍存在的 pane 下的进程树）、`agent_session`（pane 内 coding agent 会话，挂在 pane 下做细分）、`app`（launchd 拉起的 .app 及其 Helper）、`orphan`（孤儿）、`service`（launchd 拉起的后台服务）、`system`（其他账号进程且无更好归属）。
- **归因证据**：每个归属判断带一串有序证据，每条有来源与可信度；没有证据的归属不允许出现。
- **诊断**：每个归属恰好一个等级 ok / watch / act 和一句话；只有 act 级带建议动作。

## 有意行为变化

[U-BEH-1|033d] whoate 能结束进程。GUI「关闭」按钮与 CLI `whoate kill` 走同一实现：先列出将被结束的 pid 集合并要求确认（GUI 弹确认框，CLI 需 `--yes` 或交互确认），先 SIGTERM，5 秒未退再询问是否 SIGKILL；拒绝结束 uid 0 进程、Apple 平台签名进程与 whoate 自身。

[U-BEH-2|7953] whoate 只读本机其他 agent 的会话数据：`~/.claude/sessions/<pid>.json`、`~/.claude/projects/<slug>/<sessionId>.jsonl`（只读前 64KB 找第一个用户问题，倒读末尾 64KB 找 `ai-title` 与 `gitBranch`）、`~/.local/share/opencode/opencode.db` 与 `~/.local/share/kilo/kilo.db`（SQLite 只读模式，遇锁跳过本轮）。从不写这些位置。第一个用户问题截断到 200 字符，去掉 `<system-reminder>`、`<pasted_content …>` 等包裹块。

## 单向门

[U-GATE-1|2104] 仓库名与命令名 whoate 已公开（2026-10-14 建库），改名要动 GitHub 仓库、Homebrew 公式、二进制名与 agent skill 文件。

[U-GATE-2|fd28] `whoate/v1` JSON 合同一旦发布，agent 的 skill、脚本会依赖它。规则：字段只增不改；改语义或删字段升 `whoate/v2`，旧版本至少保留一个 minor 版本可用（`--schema v1`）。顶层固定键：`schema`、`taken_at`（RFC 3339，本地时区）、`host`（`mem_total_bytes`、`mem_used_bytes`、`compressed_bytes`、`swap_used_bytes`、`pressure`: normal|warn|critical）、`system_procs`（`fresh`|`stale`|`missing` + `as_of`）、`owners[]`。owner 对象：`id`（稳定：种类前缀 + 键，如 `pane:%266`、`app:com.google.Chrome`、`orphan:16977`）、`kind`、`label`、`mem_bytes`、`cpu_pct`、`pids[]`、`origin`（可选：`tmux_pane`、`pane_alive`、`cwd`、`ports[]`）、`agent`（可选：`tool`、`session_id`、`name`、`title`、`first_prompt`、`status`、`cwd`、`branch`、`model`、`cost_usd`、`confidence`）、`evidence[]`（`kind`、`detail`、`confidence`）、`verdict`（`level`、`rule`、`text`、`action` 可选）。

单向门发散：替代一「每个归属一行 NDJSON」——在 agent 需要流式处理上万归属时反超；本机归属数在 100 量级，单对象更易读，不采用，`whoate watch --json-lines` 另给流式。替代二「MCP 服务器代替 CLI JSON」——在 agent 宿主普遍只认 MCP、不允许跑 shell 时反超；当前三家宿主（Claude Code、kilo、opencode）都能跑 shell，MCP 列为以后可加的薄壳，不作为主合同。版本号机制使这扇门可后退，故不跑 design-divergence-challenger。

[U-GATE-3|8918] 发布渠道：GitHub Release 附 Apple Silicon 的 CLI 二进制与 `.app` 压缩包；Homebrew 走自建 tap 仓库 `ZhenningLang/homebrew-tap`（新公开仓库）。tap 名与公式名发布后用户的安装命令依赖它。

## 待决策

[U-DEC-1|803c] `.app` 是否做 Apple 公证：不公证则用户首次打开会被系统拦下，要去「系统设置 → 隐私与安全性」点「仍要打开」（macOS 15 起取消了右键打开这条路），或执行 `xattr -d com.apple.quarantine`；公证需要 Apple Developer Program 账号（年费 99 美元）与签名证书。

[U-DEC-2|c796] CLI `--json` 是否默认输出其他 agent 会话的第一个用户问题：默认输出方便 agent 定位，但会让一个 agent 读到另一个会话的提问原文；不默认输出则需要 `--prompts` 才给。

[U-DEC-3|ecb3] agent 能否通过 CLI 结束进程：允许（`whoate kill <pid> --yes`）还是 CLI 只给出命令供人执行（`whoate kill` 只做 dry-run，真正结束只能在 GUI 或交互终端里确认）。

[U-DEC-4|8bac] 界面与 CLI 文案语言：跟随系统语言中英双语，还是只做英文，还是只做中文。

[U-DEC-5|98d4] 产品介绍页放哪：GitHub Pages（`zhenninglang.github.io/whoate`），还是只放仓库里的 `site/` 由 README 链接本地预览。

## 边界

[U-NG-1|c7bd] 不做 Linux / Windows；采集层以 trait 隔离，留移植口。

[U-NG-2|044b] 不做长期历史库与告警推送；只在内存里保留每个归属最近 30 分钟、1 秒一点的曲线（GUI 与 `watch` 用）。

[U-NG-3|1932] 不做需要 root 的取证（`lsmp`、读他人进程 footprint 细项、`sudo` helper）。

[U-NG-4|cb80] v1 不识别 Codex、Cursor、Aider 等其他 agent 的会话；`sources/` 留扩展点，这些进程仍按 tmux pane / cwd 归属。

[U-NG-5|a552] 不用私有 API（如 responsible pid）；WebKit、XPC 这类替应用干活但父进程是 launchd 的服务归为 `service`。

[U-NG-6|43fd] 不做 TUI（owner 2026-10-14 去掉）。

[U-NG-7|5162] 诊断与建议不调大模型，不联网，不收集遥测。

## 新增依赖

[U-DEP-1|b55d] `clap`（Rust，derive 特性）：CLI 九个子命令的参数解析与帮助文本；标准库只有 `std::env::args`，手写解析与帮助生成不值。备选 `argh`（更小，但子命令帮助与补全弱）。

[U-DEP-2|7600] `serde` + `serde_json`：whoate/v1 合同的序列化与 fixture 读写；标准库无 JSON。无合理备选。

[U-DEP-3|e267] `libc`：调用 `proc_listallpids`、`proc_pid_rusage`、`proc_pidinfo`、`proc_pidfdinfo`、`sysctl`、`host_statistics64`；标准库不暴露这些。备选 `libproc` crate（封装更全，但少维护且不含 rusage v4 的 footprint 字段 [未验证：只读过 crate 文档目录，没编译试]），先用 `libc` 手写最小绑定。

[U-DEP-4|a231] `rusqlite`（`bundled` 特性）：只读打开 opencode / kilo 的会话库。备选是每次 shell out 调 `sqlite3`（每秒一个子进程，GUI 刷新时不可接受）。bundled 让二进制自带 SQLite，不依赖系统库版本。

[U-DEP-5|a60c] GitHub Actions macOS runner：CI 跑 `cargo test`、`cargo clippy -D warnings`、`swift build`；公开仓库免费额度。备选只在本机跑（贡献者 PR 无人把关）。

[U-DEP-6|b3fe] 新公开仓库 `ZhenningLang/homebrew-tap`：放 whoate 的 Homebrew 公式（CLI）与 cask（GUI）。备选提交到 homebrew-core（要求一定 star 数与稳定发布，v1 达不到）。

SwiftUI、Swift Charts、MenuBarExtra 是系统框架，不算新增依赖。

## 场景化推演

| Scenario | Actor / Context | Step-by-step path | System touchpoints | Exposed issue | Requirement / Contract |
|---|---|---|---|---|---|
| 孤儿大户 | 开发者，内存偏紧，菜单栏图标变黄 | 点菜单栏 → 面板首行「无人管理的开发服务器 9.7G」→ 点「为什么」→ 主窗口看证据 → 点「关闭这 4 个进程」→ 确认 | libproc 快采、KERN_PROCARGS2 读 TMUX_PANE、tmux list-panes、proc_pidfdinfo 端口、kill | node 可能改写 argv 区导致读不到环境变量 | 读不到 TMUX_PANE 时退到 cwd + 开发运行时判定，可信度降为 inferred（[U-SPK-3|0000]） |
| agent 自查 | Claude Code 跑构建前想知道机器还有没有余量 | `whoate top --json -n 5` → 读 `host.pressure` 与 `owners[0]` → 发现自己会话 2.6G 在涨 → `whoate why <pid> --json` | CLI 冷启动、慢采缓存、sessions/<pid>.json | 冷启动若等 top 要 3 秒，agent 体验差 | CLI 不等慢采，用 30 秒内缓存或标 `system_procs: stale`（[U-ACC-3|0000]） |
| 会话库被锁 | opencode 正在写库，GUI 每秒读一次 | 打开 `opencode.db` 只读 → SQLITE_BUSY | rusqlite 只读、busy_timeout | 每秒重试可能放大锁竞争 | busy_timeout 50ms，失败本轮沿用上次缓存并在证据里标 stale，不报错（[U-BEH-2|0000]） |

## 方案

问题拆解（纵向）：采集 → 快照 → 归因 → 诊断 → 两个出口（CLI、GUI）。归因与诊断只吃快照纯数据，便于回放测试。

### 采集

- 本账号进程：`proc_listallpids` + `proc_pid_rusage(RUSAGE_INFO_V4)` 取 `ri_phys_footprint`、`ri_user_time + ri_system_time`；`proc_pidinfo(PROC_PIDTBSDINFO)` 取 ppid、uid、启动时间、comm；`PROC_PIDVNODEPATHINFO` 取 cwd；`sysctl KERN_PROCARGS2` 取 argv 与 envp（只抽 `TMUX_PANE`、`TMUX`，其余丢弃不落盘）。
- 其他账号进程：`sysctl KERN_PROC_ALL` 拿 pid/ppid/uid/comm（不受限）；内存与 CPU 由后台线程每 6 秒跑 `top -l 1 -stats pid,mem,cpu -n 0` 后解析补齐（`-n 0` 不排序列行，[未验证：是否仍输出全部进程行，P1 spike 确认，不行改用 `-n 10000`]），结果写 `~/Library/Caches/whoate/system.json`，≤30 秒视为新鲜。
- CPU%：同一进程两次快采的 CPU 时间差 ÷ 墙钟差；CLI 单次调用快采两次、间隔 200ms。
- 系统总量：`host_statistics64(HOST_VM_INFO64)`、`sysctl vm.swapusage`、`sysctl kern.memorystatus_vm_pressure_level`。
- tmux：`tmux list-panes -a -F '#{pane_id}\t#{session_name}:#{window_index}.#{pane_index}\t#{window_name}\t#{pane_pid}\t#{pane_current_path}'`；无 tmux 或无 server 时为空。
- 端口：只对孤儿候选调 `proc_pidfdinfo` 枚举 socket，取 LISTEN 端口与 ESTABLISHED 数。

### 归因（每个进程一次，按序取第一个成立的）

1. 祖先链上有 pane shell pid → `tmux_pane`（certain）。该 pane 内若有 agent 进程，其子树再细分为 `agent_session`。
2. Claude Code：`~/.claude/sessions/<pid>.json` 存在 → agent 会话（certain），取 `sessionId`、`name`、`nameSource`、`status`、`cwd`、`tmux`；transcript 补 `ai-title`、第一个用户问题、`gitBranch`。
3. opencode / kilo：进程名或 argv 命中，按「同 cwd、根会话（parent_id 为空）、创建时间在进程启动后 10 秒内、取最近一条」配对（inferred），取 `title`、`directory`、`model`、`cost`、`time_updated`、首条 user 消息。
4. 祖先链断到 launchd 且 envp 有 `TMUX_PANE` → 该 pane 不存在时归 `orphan`（certain，origin 记 pane id 与 `pane_alive:false`）；存在时归回该 pane（certain）。
5. 祖先链断到 launchd、无 TMUX_PANE、可执行属开发运行时（node、bun、deno、python3、ruby、java、go 构建产物不判）且 cwd 在 `$HOME` 下 → `orphan`（weak）。
6. 可执行路径在某个 `.app/Contents/` 下 → `app`，按最外层 .app 的 bundle id 归组（certain）。
7. 其他 launchd 子进程 → `service`；其他账号且以上都不成立 → `system`。

### 诊断规则（表驱动，`crates/whoate-core/src/verdict/rules.rs` 一张表）

| 规则 id | 条件 | 等级 | 文案（中文） | 动作 |
|---|---|---|---|---|
| orphan-dev-server | orphan 且有 LISTEN 端口且 ESTABLISHED=0 | act | 无人管理的开发服务器，建议关闭 | `whoate kill <pids>` |
| orphan-heavy | orphan 且 mem ≥ 1 GiB | act | 启动它的 pane 已关闭 <时长>，占 <内存> | `whoate kill <pids>` |
| leak-suspect | 30 分钟内单调不降且涨幅 ≥ 1 GiB | act | 30 分钟只涨不降，疑似泄漏 | 无 |
| fast-growth | 10 分钟内涨幅 ≥ 500 MiB 且最近一分钟仍在涨 | watch | 近 10 分钟 +<量>，还在涨 | 无 |
| idle-agent | agent 会话空闲 ≥ 2 小时且 mem ≥ 500 MiB | watch | 空闲 <时长>，可关 | 无 |
| system | system 或 Apple 签名 service | ok | 系统进程，勿关 | 无 |
| default | 其余 | ok | 正常 | 无 |

依赖曲线的规则（leak-suspect、fast-growth）在 CLI 单次调用里跳过；`whoate watch` 与 GUI 有曲线。

### CLI

```text
whoate                          # 等同 whoate top
whoate top [--by mem|cpu] [-n 10] [--json] [--fresh] [--prompts|--redact]
whoate why <pid> [--json]
whoate orphans [--json]
whoate pane <%id|session:win.pane> [--json]
whoate agents [--json]
whoate kill <pid|owner-id>... [--dry-run] [--yes]
whoate watch [--interval 2s] [--json-lines]
whoate agent-guide
whoate --version
```

退出码：0 正常；1 存在 act 级诊断（仅 top/orphans/agents）；2 参数错误；3 目标不存在；4 权限不足。非 TTY 或 `NO_COLOR` 时无颜色、无框线。`agent-guide` 输出与 `skills/whoate/SKILL.md` 同内容的 Markdown。

### GUI

菜单栏常驻（`MenuBarExtra`，window 样式），点击弹面板：压力条（已用 / 压缩 / swap）+ 一句结论 + 前 5 个归属（act 级排最前）+「打开 whoate」。主窗口三栏：左栏归属种类与筛选，中栏归属列表（按内存/CPU，带 30 分钟迷你曲线），右栏详情（诊断、证据链、曲线、agent 会话信息、动作按钮）。页面示意见 approval.md「页面示意」小节与 walkthrough.md。

### GUI 与 Rust 的接口

`crates/whoate-ffi` 编为 staticlib，导出：

```c
typedef struct WhoateEngine WhoateEngine;
WhoateEngine* whoate_engine_new(void);
char* whoate_engine_tick(WhoateEngine*);          /* whoate/v1 JSON，与 CLI 同结构 */
char* whoate_why(WhoateEngine*, int32_t pid);
int32_t whoate_kill(const int32_t* pids, int32_t n, int32_t force, char** err);
void whoate_string_free(char*);
void whoate_engine_free(WhoateEngine*);
```

Swift 侧 SwiftPM C target 引头文件，`Codable` 解码。曲线环形缓冲在引擎内（每归属 1800 点）。

## 风险与验证

### Premise Collapse

If 本账号进程的 footprint 不能用 libproc 在毫秒级读到，「实时、零 token」的卖点塌掉，GUI 只能降到数秒一刷。（已验证，见 [U-SPK-2|0000]）

If Claude Code 改了 sessions 文件或 transcript 格式，agent 会话信息退化成「Claude · cwd」，归属本身仍按 tmux 链成立。（接受风险，见 [U-SPK-5|0000]）

### 翻车假设与 spike

[U-SPK-1|07fa] 只有 Command Line Tools、没有 Xcode 时，SwiftPM 能链接 Rust staticlib 并构建 SwiftUI 程序。已验证 2026-10-14：`cargo build --release` 产出 staticlib，SwiftPM C target + `linkerSettings: .unsafeFlags(["-L…","-lcore"])`，`swift build -c release` 输出 `Build complete!`，可执行打印 `{"schema":"whoate/v1","pids":54192}`；`swiftc` 单独编 SwiftUI + Charts + MenuBarExtra 也通过。

[U-SPK-2|4f56] libproc 快采速度与覆盖面。已验证 2026-10-14：Python ctypes 调 `proc_pid_rusage` v4，一遍 904 个 pid 中读到 702 个，耗时 4ms；抽 6 个 pid 与 `top` MEM 列对比 5 个完全一致、1 个差 2M（两次采样时间差）；读不到的 202 个是 root 与 `_` 系统账号（含 fseventsd、mds、WindowServer）。`top -l 1` 全量 3.6 秒、前 30 行 2.5 秒。

[U-SPK-3|4f53] 孤儿进程的环境变量里保留 `TMUX_PANE`。已验证 2026-10-14：父进程为 launchd 的 `opencode serve --service`（pid 30634）读到 `TMUX_PANE=%3647`；抽查 10 个 node/claude/opencode 进程 9 个可读，1 个（lr 拉起的 claude）读不到。读不到时走归因第 5 步，可信度 weak。

[U-SPK-4|2147] opencode / kilo 会话按目录 + 时间配对。抽查 1 例成立：pid 65250 于 17:19:13 启动，同目录根会话 17:19:18 创建。同目录同时开多个会话会配错，处置：接受风险，可信度标 inferred，界面显示「推断」。

[U-SPK-5|c4ec] Claude Code 的 `sessions/<pid>.json` 与 transcript 记录类型（`ai-title`、`last-prompt` 等）是未公开的内部格式。处置：接受风险；解析失败字段置空、证据标 weak，不让单个会话拖垮整轮；fixture 里保留一份真实样本做回归。

[U-SPK-6|8f90] GUI 每秒刷新时自身 CPU < 1%（单核）。处置：需 spike，P4 第一步，时间盒 1 小时：引擎 tick 循环 + 空 SwiftUI 列表跑 60 秒，用 `top -pid` 取平均；不达标改 2 秒刷新、列表 diff 更新。

## 验收承诺

[U-ACC-1|a19b] P1 采集：本机快照抽 10 个本账号 pid，footprint 与同时刻 `top -l 1 -stats pid,mem` 误差 < 1%；一次快采（不含慢采）< 50ms；`record-fixture` 能把本机快照存成脱敏 JSON（argv 只留可执行名与前两个参数，路径把 `$HOME` 换成 `~`）。

[U-ACC-2|fe1e] P2 归因与诊断：fixture 回放测试覆盖五个场景且全部断言具体归属与证据——孤儿 wrangler（TMUX_PANE 指向已关闭 pane → orphan certain，规则 orphan-dev-server 命中）、tmux 内 Claude 会话（sessions 文件 → agent_session certain，name / title / first_prompt 非空）、opencode 目录时间配对（inferred）、Chrome Helper 归 app、root 进程归 system；每个进程恰好一个归属，归属内存之和等于快照总和。

[U-ACC-3|41ff] P3 CLI：本机 `whoate top --json` 冷启动（慢采缓存存在）< 300ms，缓存缺失时 < 500ms 且 `system_procs` 为 `missing` 或 `stale`；输出过 whoate/v1 JSON Schema 校验；退出码五种各有测试；`whoate kill` 对 uid 0 进程返回 4 且不发信号。

[U-ACC-4|ba7a] 孤儿端到端：在新 tmux pane 里起 `python3 -m http.server 8797`，`tmux kill-pane` 关掉该 pane 后，`whoate orphans --json` 一次调用列出该进程，`origin.tmux_pane` 等于原 pane id，`pane_alive=false`，`ports` 含 8797。

[U-ACC-5|d18b] agent 定位端到端：对本会话的 claude 进程跑 `whoate why <pid> --json`，返回 `agent.name`、`agent.title`、`agent.first_prompt` 与 `~/.claude` 里的原始值一致。

[U-ACC-6|0f94] GUI：菜单栏面板与主窗口在浅色、深色两种外观各截一张图，文字无溢出、无截断；1 秒刷新 60 秒内 whoate 进程平均 CPU < 1%；点「关闭」弹出确认框且列出 pid。

[U-ACC-7|8627] 发布：在一个没装过 whoate 的用户目录下，按 README 的一行命令装好 CLI 并跑通 `whoate top`；Release 页有 CLI 二进制与 `.app` 压缩包；产品介绍页可访问（落点按 [U-DEC-5|0000] 裁决）。

## 现成方案

仓库只有 README、LICENSE 与本 spec，`/think-map` 不适用（没有代码可画）。逐能力按阶梯：

- 进程枚举与内存：停在第 3 级（系统 API，经 `libc`），不自写解析 `ps` 输出。
- 系统进程内存：停在第 4 级（平台自带 `top`），不做 root helper。
- JSON：第 5 级已有生态（serde）。
- 会话库：第 5 级（rusqlite），不自写 SQLite 解析。
- 曲线图：第 4 级（Swift Charts 系统框架）。
- 菜单栏：第 4 级（SwiftUI MenuBarExtra）。
- `.app` 打包：第 6 级，一个 shell 脚本写 Info.plist + `codesign -s -`，不引入 Xcode 工程或打包工具。

## 实施步骤

仓库形状（全部新建）：

```text
whoate/
├── Cargo.toml                       # workspace: crates/*
├── crates/
│   ├── whoate-core/src/
│   │   ├── model.rs                 # whoate/v1 结构体（serde）
│   │   ├── snapshot.rs              # Snapshot 纯数据
│   │   ├── sample/{libproc,sysctl,top,tmux,ports}.rs
│   │   ├── sources/{claude_code,opencode,kilo}.rs
│   │   ├── attribute/mod.rs         # Snapshot → Vec<Owner>
│   │   ├── verdict/rules.rs         # 诊断规则表
│   │   └── history.rs               # 30 分钟环形缓冲
│   ├── whoate-cli/src/main.rs       # clap 子命令 + 渲染 + 退出码
│   └── whoate-ffi/{src/lib.rs,include/whoate.h}
├── app/                             # SwiftPM：Package.swift + Sources/{CWhoate,Whoate}
├── schema/whoate-v1.schema.json
├── skills/whoate/SKILL.md
├── tests/fixtures/*.json            # 脱敏真实快照
├── scripts/{record-fixture.sh,bundle-app.sh,release.sh}
├── site/index.html                  # 产品介绍页
└── assets/logo/{logo.svg,AppIcon.icns,menubar-template.pdf}
```

| 期 | 内容 | 验证 |
|---|---|---|
| P1 采集 [be] | workspace、model、snapshot、sample/*、record-fixture | `cargo test -p whoate-core`；[U-ACC-1|0000] |
| P2 归因与诊断 [be] | sources/*、attribute、verdict、history | fixture 回放；[U-ACC-2|0000] |
| P3 CLI [be] | whoate-cli、JSON Schema、kill 安全、agent-guide、SKILL.md | 合同测试 + [U-ACC-3|0000] [U-ACC-4|0000] [U-ACC-5|0000] |
| P4 GUI [fe] | whoate-ffi、app/、bundle-app.sh、[U-SPK-6|0000] spike | 截图验收 + [U-ACC-6|0000] |
| P5 发布与门面 [int] | logo 定稿、README、CI、release.sh、homebrew-tap、site/ | [U-ACC-7|0000] |

依赖：P2←P1，P3←P2，P4←P2，P5←P3、P4。P3 与 P4 可并行。

```yaml
# spec-contract
checks:
  - "抽 10 个本账号 pid，footprint 与 top MEM 误差 < 1%，一次快采 < 50ms"
  - "fixture 回放五场景断言具体归属与证据，归属内存之和等于快照总和"
  - "whoate top --json 冷启动 < 300ms（有缓存），输出过 whoate/v1 JSON Schema"
  - "关掉 tmux pane 后 whoate orphans 一次调用列出其 http.server，pane_alive=false"
  - "whoate why <本会话 claude pid> 返回 name/title/first_prompt 与 ~/.claude 原值一致"
  - "GUI 浅/深两套截图无溢出，1 秒刷新 60 秒平均 CPU < 1%"
  - "按 README 一行命令在新用户目录装好 CLI 并跑通 whoate top"
non_goals:
  - "Linux / Windows"
  - "长期历史库与告警推送"
  - "需要 root 的取证"
  - "Codex / Cursor 等其他 agent 会话识别"
  - "私有 API"
  - "TUI"
  - "调大模型生成建议"
validation_commands:
  - "cargo test --workspace"
  - "cargo clippy --workspace -- -D warnings"
  - "swift build -c release --package-path app"
locked_decisions:
  - "名字 whoate，MIT，公开仓库 ZhenningLang/whoate"
  - "Rust 核心 + SwiftUI GUI + CLI，无 TUI，只做 macOS"
  - "诊断规则生成，不调模型"
  - "whoate/v1 字段只增不改"
derisk_spikes:
  - type: "#4 未知技术 / #8 环境差异"
    question: "无 Xcode 时 SwiftPM 能否链接 Rust staticlib 并构建 SwiftUI"
    method: "最小 staticlib + SwiftPM C target + swift build"
    status: "verified"
  - type: "#11 性能"
    question: "libproc 一遍全部本账号进程要多久，覆盖多少"
    method: "ctypes 调 proc_pid_rusage v4 计时并与 top 对比"
    status: "verified"
  - type: "#2 第三方契约 / #9 隐含契约"
    question: "Claude Code sessions 与 transcript 格式能否稳定解析"
    method: "读本机 23 个 sessions 文件与 transcript 记录类型统计"
    status: "deferred(接受风险：内部格式，解析失败降级)"
  - type: "#11 性能"
    question: "GUI 1 秒刷新自身 CPU 能否 < 1%"
    method: "tick 循环 + 空列表跑 60 秒取平均"
    status: "spike-before-implement"
```
