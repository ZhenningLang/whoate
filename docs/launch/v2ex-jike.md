> 草稿，未发布。owner 审稿后自己发，或逐条同意后再发；这里没有任何自动发布。

## 标题

V2EX：做了个 Mac 小工具 whoate：看清是哪个编程助手会话、哪个 tmux 分屏在吃内存

即刻：Mac 卡了，打开活动监视器只看到一排 node？我做了个工具按「谁启动的」来算内存

## 正文

### V2EX（分享创造节点）

我平时在 tmux 里同时开好几个编程助手（Claude Code、Codex 等）。Mac 一开始用 swap，活动监视器里就是一长串 node、python、浏览器的渲染进程，看不出哪些属于我忘在第三个分屏里的那个会话，哪些是它带起来的 MCP 服务，哪些是早就关掉的终端留下的孤儿进程。

于是写了 whoate。它不按进程名列，而是按「谁启动的」归到一起：

- 按会话、tmux 分屏、应用归属，而不是一串进程名；主窗口左栏有「AI & Agent」一类，每行写清从哪启动。
- 能找出终端关掉后留下的孤儿进程。
- 给编程助手用的命令行：让 agent 运行 `whoate top --json`、`whoate why <pid>`、`whoate agent-guide`，拿到结构化 JSON 和退出码。诊断在本机按规则算，不调模型，零 token。

它也认识常见软件自己的进程结构，比如 Chrome 的标签页进程和扩展进程、Docker 的虚拟机、IDE 的语言服务。建议来自内置知识库，每条都附出处和核对日期。

有菜单栏应用（SwiftUI）和命令行（Rust），一行安装：

```sh
brew install zhenninglang/tap/whoate
```

限制：只支持 Apple Silicon、macOS 14 及以上；应用没有做公证，第一次打开要去「系统设置 → 隐私与安全性」点「仍要打开」，README 里有步骤。只读本机进程信息，不联网，没有遥测，不改别的软件的设置。

仓库：https://github.com/ZhenningLang/whoate
介绍页：https://zhenninglang.github.io/whoate/

欢迎说说还希望它认识哪些软件。

### 即刻（短版）

Mac 卡了，活动监视器里一排 node，看不出是哪个编程助手会话、哪个 tmux 分屏启动的。做了个小工具 whoate：按会话、分屏、应用归属算内存，能找出关掉的终端留下的孤儿进程，还能让 agent 自己运行 `whoate top --json` 查，零 token。只读本机，不联网。

https://github.com/ZhenningLang/whoate

## 配图清单

- `site/img/demo.gif`：演示动图（演示数据）
- `site/img/main-agent-en-light.png`：主窗口里的编程助手会话
- `site/img/settings-cli-en-light.png`：设置里的命令行页，给 agent 的用法
- `assets/social/preview.png`：链接预览图
