# 分软件内存洞察调研

> 调研日期 2026-10-14。用途：给 whoate 的「为什么这么占 + 怎么省」洞察规则提供依据。
> 约束：洞察只能用本机信号触发（命令行参数、配置文件、偏好键、进程类型），不要 root，不联网。
>
> 标注约定：
> - 「本机已核实」：在本机（macOS 26 / Darwin 25.6，arm64）上用只读命令看到过，括号里写怎么看的。
> - `[未验证：原因]`：没能在本机看到，写明原因。
> - `[推断：依据]`：从源码或文档推出来、没在运行时复现的结论。
>
> 没有找到可靠出处的原因一律不写。

## 总表

| 软件 | 能观察的信号（主） | 最有价值的一条建议 |
|:---|:---|:---|
| Claude Code CLI | 进程 `claude`（Bun 编译的原生 Mach-O）；子进程里的 stdio MCP server；`~/.claude.json` 的 `mcpServers` | 长会话 RSS 持续上涨是已知 bug，重启并 `claude --resume` 能回落；同名 MCP server 每个会话各起一份，不用的在 `/mcp` 关掉 |
| opencode | 进程 `opencode` + `opencode.exe`（Bun 编译）；`~/.config/opencode/opencode.json` 的 `mcp`、`compaction` | 长跑实例内存泄漏是官方承认的问题；用 `OPENCODE_AUTO_HEAP_SNAPSHOT=1` 留证据后重启，`opencode serve` 场景注意 MCP 子进程累积 |
| vite | `node …/node_modules/.bin/vite` | 用 `vite --profile` 找慢插件；别给 `server.watch` 开 `usePolling` |
| next dev | 子进程标题 `next-server (vX.Y.Z)` | `next dev` 默认把子进程堆上限设成本机内存的 50%；想压住就设 `NODE_OPTIONS=--max-old-space-size=4096` |
| webpack | `node …/webpack` / `webpack-dev-server` | `watchOptions.ignored: /node_modules/` |
| wrangler dev | `node …/wrangler … dev` + 子进程 `workerd`、`esbuild --service` | 大 bundle 多 binding 时启动峰值高是已知问题；空闲 esbuild 吃 CPU 也是已知问题 |
| tsserver | `…/typescript/lib/tsserver.js`，参数里有 `--max-old-space-size=<N>` | tsconfig 显式 `exclude` 掉 `node_modules` 与构建产物；大 monorepo 用 project references |
| python | `python … --multiprocessing-fork`；`-m ipykernel_launcher -f …/kernel-*.json`；路径含 `.venv/bin/python` | Jupyter 用 `%reset out` / `%xdel` 释放 `Out` 缓存；DataLoader 多 worker 时把大 list 换成 numpy/pyarrow |
| Chrome / Brave | `--type=renderer`、`--extension-process`、`--type=gpu-process`；`Local State` 的 `performance_tuning.high_efficiency_mode.state` | 打开 Memory Saver（`state` 不为 2 时提示）；扩展进程多时逐个停用 |
| VS Code | `Code Helper (Plugin)`（扩展宿主）、`--type=fileWatcher`、tsserver/Pylance 子进程；`settings.json` | 用 `files.watcherExclude` 排除大目录；Pylance 设 `python.analysis.languageServerMode: "light"` |
| JetBrains（PyCharm） | `~/Library/Application Support/JetBrains/<产品><版本>/pycharm.vmoptions` 的 `-Xmx` | 生成目录 Mark Directory as Excluded；`-Xmx` 按 Help → Change Memory Settings 调 |
| Docker Desktop | `com.docker.virtualization` / `com.docker.krun` 进程；`settings-store.json` 的 `MemoryMiB` | 键缺失＝默认占本机 50% 内存给 VM；按实际需要设上限并开 Resource Saver |

---

## 1. Claude Code CLI

### 为什么占内存 / CPU

- **长会话内存持续增长是已知 bug，不是正常的上下文增长。** 官方仓库有多条报告：空闲会话 18 小时涨到约 15 GB 且上下文大小不变（[#18859，OPEN](https://github.com/anthropics/claude-code/issues/18859)）；长会话私有内存到 18 GB（[#33735](https://github.com/anthropics/claude-code/issues/33735)）；极端案例单进程 304 GB（[#17615](https://github.com/anthropics/claude-code/issues/17615)）、93 GB 堆（[#22188](https://github.com/anthropics/claude-code/issues/22188)）。
- **运行时本身的分配器也被报告过泄漏。** Bun/mimalloc 长会话约 1 GB/h 增长、12–24 小时崩溃（[#36132](https://github.com/anthropics/claude-code/issues/36132)）；Bun/WebKit Malloc 几分钟涨到 14 GB+（[#33453](https://github.com/anthropics/claude-code/issues/33453)）。
- **长会话 CPU 也会涨，杀掉后 `--resume` 恢复正常。** 见 [#22968](https://github.com/anthropics/claude-code/issues/22968)。
- **会话文件无限增长被报告为内存来源之一。** 见 [#20200](https://github.com/anthropics/claude-code/issues/20200)。
- **每个会话各自拉起 stdio MCP server。** 官方文档：stdio server 是本机进程，会话结束时 Claude Code 先发 `SIGINT`，100 ms 后 `SIGTERM`，再 400 ms 后 `SIGKILL`（[MCP 文档](https://code.claude.com/docs/en/mcp)）。所以开 N 个会话，就有 N 份同名 MCP server。
- **子 agent 并发。** 一个会话最多同时跑 20 个子 agent（`CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS`，默认 20），只读工具和子 agent 并行上限 10（`CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY`），官方说明提高它「会消耗更多资源」（[env-vars 文档](https://code.claude.com/docs/en/env-vars)）。

### 本机能观察的信号

- 可执行文件：`~/.local/bin/claude` 是软链，指向 `~/.local/share/claude/versions/<版本>`，类型是 `Mach-O 64-bit executable arm64`。本机已核实（`file -L`，版本 2.1.299）。进程 `comm` 就叫 `claude`，`argv` 只有用户给的 flag（本机常见 `claude --dangerously-skip-permissions`）。本机已核实（`ps -o comm,args`）。
  - 推论：原生安装版不是 `node` 进程，`NODE_OPTIONS=--max-old-space-size` 对它无效。`[推断：依据是可执行文件为单一 Mach-O，Bun 运行时报告见 #36132；没做对照实验]`
- MCP 子进程：本机 20 个 `claude` 进程的直接子进程里有 19 个 `chrome-reading-list-mcp`、20 个 `mcp-server-darwin-arm64`、21 个 `python`。本机已核实（`pgrep -P`）。whoate 可以按「父进程是 claude + 子进程同名」聚合，算出「同一个 MCP server 被复制了 N 份，合计 X MB」。
- MCP 配置：用户级与 local 级在 `~/.claude.json`（顶层 `mcpServers`，以及 `projects.<路径>.mcpServers`），项目级在仓库根 `.mcp.json`（[MCP 文档](https://code.claude.com/docs/en/mcp)）。本机已核实顶层 `mcpServers` 有 4 项、类型含 `stdio` 和 `http`（只数了条目，没读内容）。
- 会话文件：`~/.claude/projects/` 下按项目存会话记录。本机已核实（`du -sh`，5.8 GB，853 个项目目录）。可以作为「会话记录很大」的信号，但不等于进程内存。
- 子 agent：本机 `claude` 进程下没有 `claude` 子进程。`[推断：子 agent 跑在同一进程内，所以子 agent 的内存算在父 claude 进程里；只看了进程树，没读源码]`。agent teams 会另起 Claude 实例（[costs 文档](https://code.claude.com/docs/en/costs)）。

### 建议

- 单个 `claude` 进程 RSS 过大（比如超过 4 GB）且会话已开很久：退出后 `claude --resume` 继续，内存会回落（[#22968](https://github.com/anthropics/claude-code/issues/22968)、[#18859](https://github.com/anthropics/claude-code/issues/18859)）。
- 同名 MCP server 被多个会话各起一份：在不用它的会话里用 `/mcp` 把它关掉，或只在需要的项目的 `.mcp.json` 里配置它（[MCP 文档](https://code.claude.com/docs/en/mcp)）。
- 换任务时用 `/clear` 开新会话，不要一个会话开一整天（[costs 文档](https://code.claude.com/docs/en/costs)）。这条官方是为了省 token 写的，对内存的作用 `[推断：依据是 #33735 把增长关联到长会话的大文件读取和压缩]`。
- 子 agent 并发太高时，用 `CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY` 调低并行上限（[env-vars 文档](https://code.claude.com/docs/en/env-vars)）。

---

## 2. opencode

### 为什么占内存 / CPU

- **官方承认内存问题分散且多发，开了 Memory Megathread 收集 heap snapshot。** 维护者说明：用 `OPENCODE_AUTO_HEAP_SNAPSHOT=1 opencode` 启动后，每分钟检查一次，RSS 超过 2 GB 自动写 `.heapsnapshot` 到日志目录（[#20695](https://github.com/anomalyco/opencode/issues/20695)）。注：仓库已从 `sst/opencode` 迁到 `anomalyco/opencode`。
- **长跑实例泄漏。** 64 GB 机器连续跑十几小时后只剩 60 MB 可用（[#37799](https://github.com/anomalyco/opencode/issues/37799)）；macOS 上挂一天左右触发内核 panic，`zone map exhausted`（[#32464](https://github.com/anomalyco/opencode/issues/32464)）。
- **每次 LLM 调用都往共享 `EventTarget` 挂监听器且不清理。** `opencode web` 20 分钟从 127 MB 涨到 4.9 GB（[#34574](https://github.com/anomalyco/opencode/issues/34574)）。
- **`opencode serve` 下 MCP 子进程累积。** 每个客户端连接或重连都起一整套 MCP server，断开不回收；5 个 MCP 约 300 MB 一套，65 分钟涨了约 20 GB（[#46035，OPEN](https://github.com/anomalyco/opencode/issues/46035)）。
- **edit 工具的 per-file 锁 Map 不回收**，长会话编辑很多文件会慢慢涨（[#47604，OPEN](https://github.com/anomalyco/opencode/issues/47604)）。
- **把 PDF 附件以 base64 存进 prompt 历史 JSON**，启动时整份加载，179 MB 的 JSON 导致内存膨胀（[#34215](https://github.com/anomalyco/opencode/issues/34215)）。

本机曾见单进程峰值 92.5 GB 的具体原因 `[未验证：没有当时的 heap snapshot 或进程树]`。上面几条都能解释到几十 GB 量级，但对不上是哪一条。

### 本机能观察的信号

- 进程形态：`opencode --standalone --auto`（父，Mach-O）→ 子进程 `/opt/homebrew/lib/node_modules/@opencode/cli/bin/opencode.exe`（Mach-O arm64）。本机已核实（`ps -o pid,ppid,args` + `file -L`）。注意本机装的是 `@opencode/cli` 的 dev 构建（`opencode v0.0.0-dev-20935`），官方发行版的进程形态 `[未验证：本机没装官方发行版]`。
- MCP 子进程：本机 opencode 子进程里有 8 个 `mcp-server-darwin-arm64`、8 个 `chrome-reading-list-mcp`。本机已核实（`pgrep -P`）。
- 配置：全局 `~/.config/opencode/opencode.json`，项目级在仓库根 `opencode.json`（[config 文档](https://opencode.ai/docs/config/)）。MCP 在 `mcp.<名字>`，`type: "local"` 是本地子进程，`enabled: false` 可停用（[MCP 文档](https://opencode.ai/docs/mcp-servers/)）。压缩在 `compaction.auto`（默认 `true`）、`compaction.prune`（默认 `false`）、`compaction.reserved`（[config 文档](https://opencode.ai/docs/config/)）。本机已核实键存在：`mcp` 2 项（`local`，都启用），`compaction = {auto: true, reserved: 20000, preserve_recent_tokens: 20000}`。
- 数据目录：`~/.local/share/opencode/`，含 `opencode.db`（本机 1.3 GB）、`log/`、`tool-output/`。本机已核实（`ls`、`du`）。Megathread 说的 heap snapshot 就写在 `log/` 下 `[推断：依据是 #20695 写的「log directory」，没实际生成过]`。

### 建议

- 单个 `opencode` 进程超过 2 GB 且在涨：先用 `OPENCODE_AUTO_HEAP_SNAPSHOT=1 opencode` 重开，留下 snapshot 交给上游，再重启进程回收内存（[#20695](https://github.com/anomalyco/opencode/issues/20695)）。
- 用 `opencode serve` 时，如果 `npx`/`uv` 起的 MCP 子进程数量随时间增长，就是 #46035 的形态：重启 serve，把不必要的 MCP 设成 `enabled: false`（[#46035](https://github.com/anomalyco/opencode/issues/46035)、[MCP 文档](https://opencode.ai/docs/mcp-servers/)）。
- 不要把大 PDF 直接贴进对话，它会进 prompt 历史 JSON（[#34215](https://github.com/anomalyco/opencode/issues/34215)）。

---

## 3. Node 开发服务器

### 3.1 vite

**原因**
- 插件的 `buildStart`、`config`、`configResolved` 钩子在 dev server 启动时会被 await，钩子做重活会拖慢启动（[Vite Performance](https://vite.dev/guide/performance)）。
- barrel file 和大依赖会增加转换量；大依赖建议动态 import（[Vite Performance](https://vite.dev/guide/performance)）。
- `server.watch.usePolling` 会导致高 CPU；默认 watcher 跳过 `.git/`、`node_modules/`、`test-results/`、`cacheDir`、`build.outDir`（[server options](https://vite.dev/config/server-options)）。

**信号**
- 进程：`node <项目>/node_modules/.bin/vite` 或 `node …/vite/bin/vite.js`。本机已核实（`ps -o args`，当时有 3 个）。项目根可从 `argv` 路径推出，再读 `vite.config.*` 里的 `server.watch.usePolling` `[推断：需解析 JS 配置，静态读取可能不准]`。

**建议**
- 启动慢或 CPU 高：跑 `vite --profile`，按 `p + enter` 导出 `.cpuprofile` 找慢插件（[Vite Performance](https://vite.dev/guide/performance)）。
- 配了 `server.watch.usePolling: true` 且不在 WSL2/网络盘上：去掉它（[server options](https://vite.dev/config/server-options)）。

### 3.2 next dev

**原因**
- **`next dev` 默认把子进程的 V8 老生代上限设成本机总内存的 50%。** 源码 `packages/next/src/cli/next-dev.ts`：没设 `max-old-space-size` 且没设 `NEXT_DISABLE_MEM_OVERRIDE` 时，`maxOldSpaceSize = floor(totalMemInMB * 0.5)`，通过 `NODE_OPTIONS` 传给 fork 出来的子进程（[next-dev.ts](https://github.com/vercel/next.js/blob/canary/packages/next/src/cli/next-dev.ts)）。在 64 GB 的 Mac 上，dev server 可以长到 32 GB 才开始被 GC 卡住。
- 依赖多会更占内存；webpack 缓存会把模块放内存；启动时预加载所有页面模块且不卸载（[Next.js Memory Usage](https://nextjs.org/docs/app/guides/memory-usage)）。

**信号**
- 子进程把 `process.title` 改成 `next-server (v<版本>)`（[start-server.ts](https://github.com/vercel/next.js/blob/canary/packages/next/src/server/lib/start-server.ts)，第 207 行）。`ps -o args` 看到的就是这个标题，不是原始命令行。父进程是 `node …/next dev`。`[未验证：本机当时没有运行 next dev，结论来自源码]`
- 堆上限走 `NODE_OPTIONS` 环境变量，不在 `argv` 里，`ps` 看不到 `[推断：依据是源码用 env 传参]`。whoate 只能从 `next-server` 标题推断「默认上限＝50% 物理内存」。

**建议**
- 想让 dev server 早点被 GC 压住：`NODE_OPTIONS=--max-old-space-size=4096 next dev`（显式设置后 Next 不再覆盖，见 [next-dev.ts](https://github.com/vercel/next.js/blob/canary/packages/next/src/cli/next-dev.ts)）。
- webpack 模式下在 `next.config.js` 加 `experimental.webpackMemoryOptimizations: true`；页面很多时设 `experimental.preloadEntriesOnStart: false`（[Next.js Memory Usage](https://nextjs.org/docs/app/guides/memory-usage)）。

### 3.3 webpack

**原因**
- 监视大量文件会导致高 CPU 或高内存，典型是 `node_modules`（[watchOptions](https://webpack.js.org/configuration/watch/)）。
- watcher 退化成轮询时，文件多会导致高 CPU（[Build Performance](https://webpack.js.org/guides/build-performance/)）。
- `output.pathinfo` 在数千模块的项目里给 GC 带来压力（[Build Performance](https://webpack.js.org/guides/build-performance/)）。

**信号**
- 进程：`node …/webpack` 或 `node …/webpack-dev-server` `[未验证：本机当时没有运行 webpack]`。

**建议**
- 在 webpack 配置里加 `watchOptions: { ignored: /node_modules/ }`（[watchOptions](https://webpack.js.org/configuration/watch/)）。
- 开发环境用 `devtool: 'eval-cheap-module-source-map'`，设 `output.pathinfo: false`（[Build Performance](https://webpack.js.org/guides/build-performance/)）。

### 3.4 wrangler dev

**原因**
- Miniflare 的 `serializeConfig` 峰值 RSS 随「bundle 大小 × binding 数量」增长：8 个 Worker、66 MB 模块时 supervisor 峰值约 3.7 GB，之后回落到 0.3–0.5 GB（[workers-sdk #15271，OPEN](https://github.com/cloudflare/workers-sdk/issues/15271)）。
- 空闲时 esbuild 仍占不可忽略的 CPU（[workers-sdk #11469，OPEN](https://github.com/cloudflare/workers-sdk/issues/11469)）。

**信号**
- 进程：`node …/wrangler … dev`，子进程 `workerd` 和 `esbuild --service=…` `[未验证：本机没有 wrangler 项目在运行，进程形态来自对工具链的了解，未在本机 ps 中看到]`。

**建议**
- 启动时 RSS 冲到几个 GB 又回落：这是 #15271 的已知形态，不是泄漏，等它回落即可；多 Worker 项目可拆分启动（[#15271](https://github.com/cloudflare/workers-sdk/issues/15271)）。
- 内存持续涨：按 `d` 打开 DevTools 的 Memory 面板抓 heap snapshot（[Cloudflare 文档](https://developers.cloudflare.com/workers/observability/dev-tools/memory-usage/)）。

### 3.5 tsserver

**原因**
- TypeScript 读到了不该读的文件（`node_modules`、构建产物）；单个大工程时语言服务会内存不足（[TypeScript Performance wiki](https://github.com/microsoft/TypeScript/wiki/Performance)）。
- 自动引入所有 `@types` 包会拖慢工程构建（[TypeScript Performance wiki](https://github.com/microsoft/TypeScript/wiki/Performance)）。

**信号**
- VS Code 里的 tsserver 由 TS 扩展启动，参数里带 `--max-old-space-size=${maxTsServerMemory}`，还会有 `--serverMode partialSemantic`（语法服务器）、`--useInferredProjectPerProjectRoot`、`--cancellationPipeName`。本机已核实（读 VS Code 1.138.0 安装包里 `extensions/typescript-language-features/dist/extension.js` 的字符串，不是运行时 `ps`）。
- 内置 tsserver 路径：`/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/node_modules/typescript/lib/tsserver.js`。本机已核实（`ls`）。用了工作区 TypeScript 时路径是 `<项目>/node_modules/typescript/lib/tsserver.js` `[推断：依据是 typescript 包的标准目录结构，本机没有运行中的工作区 tsserver]`。
- `--max-old-space-size` 的数值就是堆上限，whoate 可以直接拿 RSS 和它比。

**建议**
- tsconfig 里一旦写了 `exclude`，就要显式加 `node_modules`；想看读了哪些文件用 `tsc --listFilesOnly`（[TypeScript Performance wiki](https://github.com/microsoft/TypeScript/wiki/Performance)）。
- 大 monorepo 拆成 project references，并在编辑器侧设 `disableReferencedProjectLoad: true`、`disableSolutionSearching: true`（[TypeScript Performance wiki](https://github.com/microsoft/TypeScript/wiki/Performance)）。

---

## 4. Python

### 为什么占内存 / CPU

- **Jupyter/IPython 的输出缓存会让旧结果无法回收。** `_`、`__`、`_<n>`、`Out` 都持有引用，「可能给系统带来很大的内存压力」；用 `InteractiveShell.cache_size` 控制，设 0 关闭（[IPython reference](https://ipython.readthedocs.io/en/stable/interactive/reference.html)）。
- **macOS 上 multiprocessing 默认用 spawn**（3.8 起），每个子进程都是全新解释器，要重新 import，比 fork/forkserver 慢（[multiprocessing 文档](https://docs.python.org/3/library/multiprocessing.html)）。每个 worker 各自持有一份 import 后的模块 `[推断：spawn 不共享父进程内存页]`。
- **PyTorch DataLoader 多 worker 时，worker 访问父进程的 Python 对象后，会占到和父进程一样多的内存**，总量约「worker 数 × 父进程大小」；解法是换成 Pandas/Numpy/PyArrow 这类非引用计数的表示（[PyTorch DataLoader 文档](https://docs.pytorch.org/docs/2.14/data.html)）。
- **pymalloc 以 1 MiB arena 为单位向系统要内存**，只有整个 arena 空了才能还（[Python Memory Management](https://docs.python.org/3/c-api/memory.html)）。这会让 RSS 在对象释放后不回落 `[推断：文档只写了 arena 用 mmap/munmap 分配释放，碎片导致不回落是推论]`。

### 本机能观察的信号

- multiprocessing spawn 子进程：命令行形如 `python -c 'from multiprocessing.spawn import spawn_main; spawn_main(…)' --multiprocessing-fork`。本机已核实（Python 3.13.12 调用 `spawn.get_command_line()` 的输出；`get_start_method()` 为 `spawn`）。whoate 可以把这类进程按父进程聚合成「某脚本的 N 个 worker」。
- Jupyter kernel：命令行 `python -m ipykernel_launcher -f <连接文件>`（[ipykernel kernelspec.py](https://github.com/ipython/ipykernel/blob/main/ipykernel/kernelspec.py)）。连接文件在 `~/Library/Jupyter/runtime/kernel-<id>.json`。本机已核实 runtime 目录和 3 个 `kernel-*.json`（`ls`，只看了文件名形态）；本机没装 ipykernel，运行时命令行 `[未验证：本机没有运行中的 kernel]`。
- 虚拟环境：解释器路径里出现 `/.venv/bin/python` 或 `/envs/<名字>/bin/python`，其上一级目录有 `pyvenv.cfg`（[venv 文档](https://docs.python.org/3/library/venv.html)）`[未验证：本机当时运行的 python 进程没逐个查路径]`。
- PyTorch DataLoader worker 在 macOS 上同样走 spawn，命令行也是 `--multiprocessing-fork` `[推断：DataLoader 用 multiprocessing，macOS 默认 spawn]`。

### 建议

- Jupyter kernel 占用大：在 notebook 里跑 `%reset out` 清输出缓存、`%xdel 变量名` 删大对象；或在 IPython 配置里设 `c.InteractiveShell.cache_size = 0`（[IPython reference](https://ipython.readthedocs.io/en/stable/interactive/reference.html)）。
- 一个脚本带出很多 `--multiprocessing-fork` 子进程且每个都大：减少 worker 数，或把数据换成 numpy/pyarrow 让 worker 不复制（[PyTorch DataLoader 文档](https://docs.pytorch.org/docs/2.14/data.html)）。
- 定位 Python 侧谁在分配：`python -X tracemalloc=25 script.py`，或在代码里 `tracemalloc.start()` 后比较快照（[tracemalloc 文档](https://docs.python.org/3/library/tracemalloc.html)）。

---

## 5. Google Chrome 与 Brave

### 为什么占内存 / CPU

- **Site Isolation：每个站点一个渲染进程**，每个进程都有固定开销；桌面端有按可用内存设定的「软」进程上限，超过后才开始复用同站进程（[Chromium 进程模型](https://chromium.googlesource.com/chromium/src/+/main/docs/process_model_and_site_isolation.md)）。
- **扩展各自独占渲染进程**，扩展之间、扩展和网页之间都不共享进程；content script 则跑在它注入的网页进程里（[Chromium 进程模型](https://chromium.googlesource.com/chromium/src/+/main/docs/process_model_and_site_isolation.md)）。
- **Memory Saver 默认关闭。** 源码里 `kMemorySaverModeState` 的默认值是 `kDisabled`（[prefs.cc](https://github.com/chromium/chromium/blob/main/components/performance_manager/user_tuning/prefs.cc)）。打开后 Chrome 会停用不在用的标签页，回到标签页时重新加载（[Chrome 帮助](https://support.google.com/chrome/answer/12929150?hl=en)）。
- GPU 进程的内存构成 `[未验证：没找到 Chromium 官方对 GPU 进程内存构成的说明，不写原因]`。

### 本机能观察的信号

**进程参数**（本机已核实，`ps -axww -o args` 统计 Chrome 与 Brave 的 helper，只统计 flag 名）
- 渲染进程：`Google Chrome Helper (Renderer)` / `Brave Browser Helper (Renderer)`，带 `--type=renderer`（本机 73 个）。
- 扩展进程：`--type=renderer --extension-process`（本机 12 个）。
- GPU 进程：`--type=gpu-process`（本机 3 个；本机同时开着 Chrome、Brave 和 headless Chrome，每个浏览器实例各一个 `[推断：没逐个对应父进程]`）。
- 工具进程：`--type=utility --utility-sub-type=network.mojom.NetworkService` / `storage.mojom.StorageService` / `audio.mojom.AudioService` 等；Brave 另有 `brave_rewards.mojom.RewardsEngineFactory`。
- 浏览器版本号在 helper 路径里：`…/Google Chrome Framework.framework/Versions/<版本>/Helpers/…`。本机同时看到 154 和 155 两个版本的 helper，说明旧版本进程在升级后还活着 `[推断：依据是同时存在两个版本目录下的 helper，没确认是哪个窗口]`。

**偏好文件**（Chrome 与 Brave 用同一套键，Brave 是 Chromium 衍生）

| 设置 | 文件 | 键 | 取值 |
|:---|:---|:---|:---|
| Memory Saver 开关 | `Local State` | `performance_tuning.high_efficiency_mode.state` | `0` 关、`1` 已废弃、`2` 开；键缺失＝默认关（[prefs.h](https://github.com/chromium/chromium/blob/main/components/performance_manager/public/user_tuning/prefs.h)、[prefs.cc](https://github.com/chromium/chromium/blob/main/components/performance_manager/user_tuning/prefs.cc)） |
| Memory Saver 力度 | `Local State` | `performance_tuning.high_efficiency_mode.aggressiveness` | `0` conservative、`1` medium、`2` aggressive；缺失＝`1`（同上） |
| Energy Saver | `Local State` | `performance_tuning.battery_saver_mode.state` | `0` 关、`1` 低电量时开、`2` 用电池时开、`3` 常开；缺失＝`1`（同上） |
| 标签页冻结 | `Local State` | `performance_tuning.tab_freezing.enabled` | 缺失＝`true`（同上） |
| 不休眠的站点 | `Default/Preferences` | `performance_tuning.tab_discarding.exceptions_with_time` 等 | 按 profile 存（同上） |
| 硬件加速 | `Local State` | `hardware_acceleration_mode.enabled` | 缺失＝默认开（[pref_names.h](https://github.com/chromium/chromium/blob/main/chrome/common/pref_names.h)） |
| 后台运行 | `Local State` | `background_mode.enabled` | 同上 |
| 已装扩展 | `Default/Secure Preferences` | `extensions.settings`（每个扩展一项） | 判断是否启用要看各项的状态字段 `[推断：本机按 disable_reasons/state 粗算，字段语义未查源码]` |

路径：Chrome `~/Library/Application Support/Google/Chrome/`，Brave `~/Library/Application Support/BraveSoftware/Brave-Browser/`。

本机已核实（Python 只读解析 JSON，只输出了下列键值）：
- Chrome 与 Brave 的 `Local State` 里 `performance_tuning.high_efficiency_mode.state = 2`，即两者 Memory Saver 都已开启。
- 两者都没有 `aggressiveness`、`battery_saver_mode.state`、`hardware_acceleration_mode.enabled` 键，按上表是默认值。
- `tab_stats.discards_proactive` 在 Chrome 是 6、Brave 是 4，可作为「Memory Saver 确实在休眠标签」的旁证（这个键名的含义 `[推断：从键名推出，未查源码]`）。
- Brave 有 `performance_tuning.discard_ring_treatment.enabled = true`。
- 扩展：Chrome `Secure Preferences` 的 `extensions.settings` 有 37 项，Brave 21 项（含组件扩展）。

**注意**：企业策略（`HighEfficiencyModeEnabled`、`MemorySaverModeSavings`）可以覆盖用户偏好，策略值不在 `Local State` 里 `[未验证：本机没有托管策略，策略存放位置没核实]`。所以 `state` 只代表用户自己的设置。

### 建议

- `state` 不是 `2`：到 Chrome「设置 → 性能 → Memory Saver」打开，常用站点加到「Always keep these sites active」（[Chrome 帮助](https://support.google.com/chrome/answer/12929150?hl=en)）。Brave 在「设置 → 系统 → 性能」`[来源：Brave 官方帮助文章，原页 403 无法直接抓取，内容来自搜索摘要]`（[Brave 帮助](https://support.brave.app/hc/en-us/articles/13383683902733-How-do-I-use-the-Memory-and-Energy-Saver-features-in-Brave)）。
- 已开但内存仍高：把力度调到 Maximum（对应 `aggressiveness = 2`），标签页会更早被停用（[Chrome 帮助](https://support.google.com/chrome/answer/12929150?hl=en)）。
- `--extension-process` 进程多且大：在 Chrome 任务管理器（Window → Task Manager）里看哪个扩展占得多，停用它；每个扩展都独占一个进程（[Chromium 进程模型](https://chromium.googlesource.com/chromium/src/+/main/docs/process_model_and_site_isolation.md)）。

---

## 6. VS Code

### 为什么占内存 / CPU

- **扩展宿主 CPU 高，多半是某个扩展导致的。** 官方建议：Help → Open Process Explorer 看哪个进程；用 `code --disable-extensions` 对照；用 F1 → Help: Start Extension Bisect 二分定位（[Performance Issues wiki](https://github.com/microsoft/vscode/wiki/Performance-Issues)）。
- **文件监视进程吃 CPU，要排除大目录。** `files.watcherExclude` 的官方描述：文件监视进程占用大量 CPU 时，排除构建输出这类不关心的大目录（本机已核实，读 VS Code 1.138.0 `out/nls.messages.json`）。默认值只排除 `.git/objects/**`、`.git/subtree-cache/**`、`.hg/store/**`（及 `*/` 前缀版本），**不包括 `node_modules`**（本机已核实，读 `workbench.desktop.main.js` 的默认值）。
- **tsserver 堆上限默认 3072 MB**（`typescript.tsserver.maxTsServerMemory`，本机已核实，读内置 TS 扩展 `package.json`），作为 `--max-old-space-size` 传给 tsserver（本机已核实，见 3.5）。
- **Pylance 的 Node 堆上限默认 8192 MB**：`python.analysis.nodeArguments` 默认 `["--max-old-space-size=8192"]`（本机已核实，读 `ms-python.vscode-pylance-2026.2.1/package.json`）。其他相关默认值：`python.analysis.indexing = true`、`userFileIndexingLimit = 2000`、`languageServerMode = "default"`（可选 `light`/`default`/`full`）、`diagnosticMode = "openFilesOnly"`（同上）。`light` 模式会默认关闭 indexing、`useLibraryCodeForTypes`，并把 `exclude` 设成 `["**"]`（[Pylance 文档，经搜索摘要](https://github.com/microsoft/pylance-release)）。
- **rust-analyzer 的默认行为都偏「全量」**：`cachePriming.enable = true`（加载时预热缓存）、`procMacro.enable = true`、`cargo.buildScripts.enable = true`（运行 build.rs）、`checkOnSave = true`、`lru.capacity` 缺省保留 128 棵语法树（[rust-analyzer 配置](https://rust-analyzer.github.io/book/configuration.html)）。

### 本机能观察的信号

- helper 进程名：`Code Helper.app`、`Code Helper (GPU).app`、`Code Helper (Plugin).app`、`Code Helper (Renderer).app`。本机已核实（`ls` 应用包的 `Contents/Frameworks/`）。
- 扩展宿主：VS Code 用 Electron utility process 启动它，`name: "extension-host"`，参数含 `--skipWorkspaceStorageLock`。本机已核实（读 `out/main.js` 源码字符串）。Electron 的 `allowLoadingUnsignedLibraries` 选项会让 utility process 用 `Helper (Plugin).app` 启动（[Electron utilityProcess](https://www.electronjs.org/docs/latest/api/utility-process)）。所以运行时形态应是 `Code Helper (Plugin) --type=utility --utility-sub-type=node.mojom.NodeService … --skipWorkspaceStorageLock` `[未验证：本机 VS Code 当时没在运行，没拿到 ps 输出]`。
- 文件监视：参数 `--type=fileWatcher`（本机已核实，源码字符串）。
- Process Explorer 对进程的分类规则：`--type=renderer` → window，`--type=utility` → utility-process，`--type=extensionHost` → extension-host（本机已核实，`out/main.js` 源码）。whoate 可以复用同一套规则。
- 语言服务器：
  - Pylance：`node … ~/.vscode/extensions/ms-python.vscode-pylance-<版本>/dist/server.bundle.js`，带 `--max-old-space-size=8192`。本机已核实 `server.bundle.js` 存在、扩展里有 `--max-old-space-size=8192` 字符串；运行时 `[未验证：VS Code 没在运行]`。
  - tsserver：见 3.5。
  - rust-analyzer：`~/.vscode/extensions/rust-lang.rust-analyzer-<版本>/server/rust-analyzer` `[未验证：本机没装该扩展]`。
- 设置文件：`~/Library/Application Support/Code/User/settings.json`（本机已核实存在，有 `profiles/` 子目录存其它 profile）；工作区设置在 `<项目>/.vscode/settings.json`。本机已核实用户设置里 `python.analysis.languageServerMode = "light"`、`python.analysis.typeCheckingMode = "basic"`。注意 settings.json 是 JSONC（允许注释和尾逗号），解析要用宽松解析器。

### 建议

- 扩展宿主 CPU/内存高：F1 → Help: Start Extension Bisect 找出问题扩展（[Performance Issues wiki](https://github.com/microsoft/vscode/wiki/Performance-Issues)）。
- 文件监视进程 CPU 高：在 `files.watcherExclude` 里加 `"**/node_modules/**": true`、`"**/dist/**": true` 等构建目录（设置描述见上，本机已核实）。
- Pylance 占内存大：设 `"python.analysis.languageServerMode": "light"`，或单独设 `"python.analysis.indexing": false`、调低 `python.analysis.userFileIndexingLimit`（默认值本机已核实；效果说明见 [Pylance 文档](https://github.com/microsoft/pylance-release)）。
- tsserver 频繁崩溃重启：大工程才调高 `typescript.tsserver.maxTsServerMemory`；工程本身先按 3.5 排除 `node_modules`。上限太低会频繁 GC 甚至崩溃（[vtsls 说明](https://github.com/yioneko/vtsls)）。
- rust-analyzer 占用大：关 `rust-analyzer.cachePriming.enable`，必要时关 `rust-analyzer.procMacro.enable`（会损失宏展开）（[rust-analyzer 配置](https://rust-analyzer.github.io/book/configuration.html)）。

---

## 7. JetBrains IDE（以 PyCharm 为例）

### 为什么占内存 / CPU

- **堆上限 `-Xmx` 决定 IDE 最多能用多少 Java 堆**；空闲堆在 GC 后低于最大堆的 5% 时 IDE 会弹低内存警告（[Increase memory heap](https://www.jetbrains.com/help/pycharm/increasing-memory-heap.html)）。
- **索引**：项目里动态生成的文件和目录会被分析和索引，排除它们能减少分析时间并提升整体性能（[Indexing](https://www.jetbrains.com/help/pycharm/indexing.html)）。
- **打开项目时本地索引和共享索引同时处理，会推高 CPU**，可以开 Wait for shared indexes（[Shared indexes 2022.3](https://www.jetbrains.com/help/pycharm/2022.3/shared-indexes.html)）。
- **Power Save Mode 会关闭检查和其它重型后台任务**（[RustRover 性能调优](https://www.jetbrains.com/help/rust/2025.1/performance-tuning-tips.html)；PyCharm 同平台 `[推断：IntelliJ 平台共用此功能，没找到 PyCharm 专页]`）。

### 本机能观察的信号

- 用户 vmoptions：`~/Library/Application Support/JetBrains/<产品><版本>/pycharm.vmoptions`（IntelliJ IDEA 是 `idea.vmoptions`）。目录规则见 [IDE 目录文档](https://www.jetbrains.com/help/pycharm/directories-used-by-the-ide-to-store-settings-caches-plugins-and-logs.html)。本机已核实：存在 `PyCharmCE2020.1` 到 `PyCharmCE2024.3` 共 15 个目录，各有 `pycharm.vmoptions`；`PyCharmCE2024.3/pycharm.vmoptions` 里是 `-Xmx2048m`。
- 应用内默认 vmoptions：`PyCharm.app/Contents/bin/pycharm.vmoptions`；也可以用环境变量 `PYCHARM_VM_OPTIONS` 指定（[Tuning the IDE](https://www.jetbrains.com/help/pycharm/tuning-the-ide.html)）。`[未验证：本机没装 PyCharm 应用]`
- 缓存/索引目录：`~/Library/Caches/JetBrains/<产品><版本>`；插件：配置目录下 `plugins/`；日志：`~/Library/Logs/JetBrains/<产品><版本>`（[IDE 目录文档](https://www.jetbrains.com/help/pycharm/directories-used-by-the-ide-to-store-settings-caches-plugins-and-logs.html)）。本机已核实 2024.3 配置目录下有 `plugins/` 和 `disabled_plugins.txt`。
- 进程命令行里能不能看到 `-Xmx` `[未验证：本机没在运行 JetBrains IDE]`。JetBrains 的原生启动器从 vmoptions 读 JVM 参数，`ps` 里可能只看到 `…/MacOS/pycharm`，所以应以读 vmoptions 文件为准 `[推断：依据是 Tuning the IDE 文档说 JVM 参数来自 vmoptions 文件，没看到运行时进程]`。版本号可从进程路径对应的 `Info.plist` 拿到，再拼出配置目录名。
- 选哪个 vmoptions：同一产品有多个版本目录时，要按运行中进程的版本去选，不能取最新目录 `[推断：依据是本机 15 个版本目录各有独立 vmoptions，且 -Xmx 可能不同]`。

### 建议

- RSS 接近 `-Xmx` 且常弹低内存警告：Help → Change Memory Settings 调高（[Increase memory heap](https://www.jetbrains.com/help/pycharm/increasing-memory-heap.html)）；反之 IDE 闲着也占很多，可以调低。
- 索引久、CPU 高：对构建产物、数据目录右键 Mark Directory as → Excluded（[Indexing](https://www.jetbrains.com/help/pycharm/indexing.html)）。
- 临时省电：File → Power Save Mode（[RustRover 性能调优](https://www.jetbrains.com/help/rust/2025.1/performance-tuning-tips.html)）。

---

## 8. Docker Desktop

### 为什么占内存 / CPU

- **内存给的是 Linux VM，默认是本机内存的 50%**；CPU 上限、swap（默认 1 GB）也在 Resources → Advanced 里设（[Docker Desktop settings](https://docs.docker.com/desktop/settings-and-maintenance/settings/)）。
- **VM 不会自动把内存还给 macOS，除非用 Docker VMM 或开 Resource Saver。** Docker VMM 文档写明它在容器不活跃时把空闲内存还给主机（[VMM 文档](https://docs.docker.com/desktop/features/vmm/)）。Resource Saver 在没有容器运行一段时间后（默认 5 分钟）停掉 VM，可省 2 GB 以上（[Resource Saver](https://docs.docker.com/desktop/use-desktop/resource-saver/)）。
- 老版本里，Docker 进程本身（不是 VM）在 `docker cp` 等主机到容器的传输后不释放内存（[docker/for-mac #6120](https://github.com/docker/for-mac/issues/6120)，Docker Desktop 4.3.2 时代，可能已过时）。

### 本机能观察的信号

- 设置文件：`~/Library/Group Containers/group.com.docker/settings-store.json`（4.34 及更早是同目录 `settings.json`）（[Docker Desktop settings](https://docs.docker.com/desktop/settings-and-maintenance/settings/)）。本机已核实文件存在。
- 键名（PascalCase）：`MemoryMiB`、`Cpus`、`SwapMiB`、`DiskSizeMiB`、`UseResourceSaver`、`AutoPauseTimeoutSeconds`、`UseVirtualizationFramework`、`UseLibkrun`（Docker VMM）。本机已核实（`strings` 读 `/Applications/Docker.app/Contents/MacOS/com.docker.backend`，Docker Desktop 4.94.0；本机 `settings-store.json` 里有 `DiskSizeMiB = 98304`，其它资源键都不在）。
  - 键缺失＝用默认值：内存 50% 本机内存（[Docker Desktop settings](https://docs.docker.com/desktop/settings-and-maintenance/settings/)）。`AutoPauseTimeoutSeconds` 对应 Resource Saver 超时 `[推断：从键名与 Resource Saver 默认 5 分钟对应，未查源码]`。
  - 第三方教程写成小写 `memoryMiB`/`cpus`（[oneuptime](https://oneuptime.com/blog/post/2026-02-08-how-to-configure-docker-desktop-memory-and-cpu-limits-on-macos/view)），与本机二进制里的 PascalCase 不一致，以本机为准。
- 进程：`/Applications/Docker.app/Contents/MacOS/` 下有 `com.docker.backend`、`com.docker.virtualization`（Apple Virtualization framework）、`com.docker.krun`（Docker VMM）等。本机已核实（`ls`）。用 Apple Virtualization framework 时，VM 内存记在系统进程 `com.apple.Virtualization.VirtualMachine` 名下 `[未验证：本机 Docker 当时没在运行；依据是第三方文章 extrememanual.net/60127]`。whoate 要把这个系统进程归到 Docker（看它的父进程或负责进程）。
- `docker stats`：实时显示容器的 `MEM USAGE / LIMIT`；Linux 上 CLI 报告的内存已减去 cache（[docker stats](https://docs.docker.com/reference/cli/docker/container/stats/)）。用 `docker stats --no-stream --format json` 取一次快照。它依赖 daemon：本机 Docker 没运行时报错 `failed to connect to the docker API at unix:///Users/<用户>/.docker/run/docker.sock`。本机已核实。

### 建议

- `MemoryMiB` 缺失（＝50% 本机内存）且实际容器用不了那么多：在 Settings → Resources → Advanced 把 Memory limit 调到 `docker stats` 峰值之和再加余量（[Docker Desktop settings](https://docs.docker.com/desktop/settings-and-maintenance/settings/)）。
- 经常开着 Docker 但很少跑容器：确认 `UseResourceSaver` 没被关掉，让 VM 空闲 5 分钟后自动停（[Resource Saver](https://docs.docker.com/desktop/use-desktop/resource-saver/)）。
- 希望空闲内存还给 macOS：Settings → General → Virtual Machine Manager 选 Docker VMM（需至少给 VM 4 GB，且不支持 Rosetta，amd64 镜像会慢）（[VMM 文档](https://docs.docker.com/desktop/features/vmm/)）。

---

## 附：本机核实清单

| 信号 | 结果 | 方法 |
|:---|:---|:---|
| Chrome/Brave `Local State` 的 `performance_tuning.high_efficiency_mode.state` | 已核实，两者都是 2（开） | Python 只读解析 JSON，只打印目标键 |
| Chrome/Brave 进程参数 `--type=renderer`、`--extension-process`、`--type=gpu-process`、`--utility-sub-type=…` | 已核实 | `ps -axww -o args`，只统计 flag 名 |
| Memory Saver 等键的取值含义与默认值 | 已核实（源码） | Chromium `prefs.h`、`prefs.cc` |
| Claude Code 是 Mach-O 原生二进制、子进程里的 MCP server | 已核实 | `file -L`、`pgrep -P` |
| `~/.claude.json` 有 `mcpServers` | 已核实（只数了条目） | Python 读 JSON |
| opencode 进程树与 `opencode.json` 的 `mcp`、`compaction` | 已核实（dev 构建） | `ps`、Python 读 JSON |
| opencode 92.5 GB 峰值原因 | 未验证 | 没有当时的现场数据 |
| vite 进程形态 | 已核实 | `ps` |
| next dev 进程标题、50% 堆上限 | 已核实（源码），运行时未验证 | `gh api` 读 Next.js 源码 |
| webpack、wrangler 进程形态 | 未验证 | 本机没在运行 |
| tsserver 启动参数、`maxTsServerMemory` 默认 3072 | 已核实（安装包） | 读 VS Code 1.138.0 内置扩展 |
| Pylance 默认 `--max-old-space-size=8192`、indexing 默认值 | 已核实（安装包） | 读 Pylance 2026.2.1 `package.json` |
| `files.watcherExclude` 默认值与描述 | 已核实（安装包） | 读 workbench JS 与 nls 文件 |
| VS Code 扩展宿主运行时 ps 形态 | 未验证 | VS Code 没在运行；只核实了源码字符串 |
| rust-analyzer 路径 | 未验证 | 本机没装 |
| Python spawn 子进程命令行 | 已核实 | 调 `multiprocessing.spawn.get_command_line()` |
| Jupyter runtime 目录与 `kernel-*.json` | 已核实（只看文件名） | `ls` |
| ipykernel 运行时命令行 | 未验证 | 本机没装 ipykernel；依据源码 |
| PyCharm 用户 vmoptions 路径与 `-Xmx2048m` | 已核实 | `ls`、只取 `-Xm[sx]` 行 |
| PyCharm 应用内 vmoptions、进程参数 | 未验证 | 本机没装 PyCharm 应用 |
| Docker `settings-store.json` 路径与键名 | 已核实 | `ls`、`strings com.docker.backend` |
| Docker VM 内存记在 `com.apple.Virtualization.VirtualMachine` | 未验证 | Docker 没在运行；仅第三方来源 |
| `docker stats` 在 daemon 未运行时的报错 | 已核实 | 运行 `docker version` |
