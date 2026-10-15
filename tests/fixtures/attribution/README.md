# Phase 02 fixtures

`sample-v1.json` 是 P4/P5 接口开发用的**完整手写展示样例**。
全部 pid、会话、资源与 `demo.insight` 都是合成值，不代表本机命中。
`demo.insight` 只用于展示，不是生产知识库条目。
该样例不能充当 JSON Schema、GUI 实机或真实群回放的验收证据。

`published-claude.jsonl` 原样保留公开文章的两段 JSON 示例，
仅把为排版折行的 JSON 合为每记录一行。来源：
https://www.adityabawankule.io/blog/claude-code-session-jsonl-format
（Aditya Bawankule，2026-07-09，2026-10-15 实际读取）。
文章声明检查真实会话后提供 trimmed 示例；这些是已裁剪的公开格式样本，
不是本机完整会话、也不能证明当前 Claude 私有格式未变化。
首问原文是 `fix the failing test`，第二条是工具结果，不能当用户首问。

SQLite 格式依据上游公开源码，不读取 owner HOME：

- OpenCode commit `654b79b27e79da735d6b07d766c8d7431a71d1b7`：
  `packages/core/src/session/sql.ts` 的 session_v2/session_message，
  `packages/schema/src/session-message.ts` 的 user.text/assistant.content。
- Kilo commit `bb6bb187a399340e1a3ff06167115ec442e9924f`：
  `packages/core/src/session/sql.ts` 的 session/message/part 与 session_message。
  两家 GitHub 仓库分别是 https://github.com/anomalyco/opencode 与
  https://github.com/Kilo-Org/kilocode 。
- 临时测试库是依据上述 schema 手写的合成数据，不是实际会话数据库。

历史快照始终只读引用 `../capture/snapshot-20261014-1940.json`。
历史中缺 session/cwd 等上下文，不填当前机器值。
服务群成员与份额根据保存的进程父链独立核对，不能声称已验证历史会话 id。

`swarm-1940-oracle.json` 的 68 pid、字节数与 31 份 owner 分布在接入生产
signature/attribute 前冻结。之后独立复核八步优先级，修正包装根与孤儿：
23 个 Claude 和 7 个 pane 内 OpenCode 祖先各 2 份，`orphan:30634` 有 8 份。
30 个是进程支持的弱 agent 归属；另 1 个是确定的孤儿，不是 31 个已证实的会话。
`restore_npm_exec_pids` 是原始来源逐条核对的 34 个 `npm exec ` 前缀。
P1 fixture 把 `exec` 写成 `<redacted>`；回放只在内存中恢复这个公开词，
既不改旧 fixture，也不让生产规则把 `<redacted>` 当作执行证据。
原始文件 SHA256 在侧车中，2026-10-15 只读核对后仍为 0600。

`restore_node_bin_pids` 同样来自该原始源：34 条 node 的公开入口均为
`node_modules/.bin/chrome-reading-list-mcp`。P1 将 `.bin` 脱敏为 `path-103`。
回放只在内存中恢复 `.bin`；生产不将未知目录段猜成包名或服务名。
2026-10-16 再核 SHA256 不变、0600，精确公开入口匹配 34/34。
