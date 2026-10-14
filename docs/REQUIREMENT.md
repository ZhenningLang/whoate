# whoate v1 · REQUIREMENT

已批准的规格是唯一依据，开发时按它实现，不另起需求：

- 机器版规格（全部登记单元、分期、验收）：`docs/specs/overview.md`（State: approved，owner 2026-10-14 两轮评审通过）
- 审批投影：`docs/specs/approval.md`
- 架构投影：`docs/specs/architecture.md`
- 界面原型与走查：`docs/specs/mockups/`、`docs/specs/walkthrough.md`
- 知识库调研（识别器建议的出处）：`docs/research/memory-insights.md`

目标：按 overview.md「实施步骤」的六期（P1 采集、P2 归因核心与编程助手、P3 识别器扩展与知识库、P4 CLI、P5 GUI、P6 发布与门面）实现 whoate v1，每期验收以 overview.md 登记的 U-ACC 为准。

不可牺牲的约束：只做 macOS；Rust 核心 + SwiftUI GUI + CLI，无 TUI；诊断规则生成不调模型；whoate/v1 JSON 字段只增不改；识别器只读本机信号、不改软件设置；不得自行简化或跳过任何登记的验收项，确需简化须 owner 当次批准。
