# Capture fixture

`snapshot-20261014-1940.json` 派生自已批准的 2026-10-14 19:40:08 本机捕获。
原始文件保留在 orchestration workspace，权限 0600，永不提交。

生成入口：`scripts/record-fixture.sh --source <workspace>/raw-snapshot-20261014-1940.json --output tests/fixtures/capture/snapshot-20261014-1940.json`。
生成与验证命令通过 `lr.py heavy` 执行。

来源缺项：host、cwd、responsible pid、socket、TMUX socket 信息未捕获，均为 null。
`start_time` 是 Unix 微秒，但来源只有秒精度。`argv_lossy=true`：历史 command
只是 ps 的文本显示，不是无损 argv；不执行 command，不解释 shell 转义。
历史 tmux 行只有四列，cwd 为 null。top_raw 与 note 从不进入 fixture。
原有 footprint/cpu_time、pid/ppid/uid、tmux pane 关系保持不变。
带空格的应用可执行名按公开名称或bundle同名合并，位置参数独立处理；
其他不明确的历史文本仍按空白有损拆分。原生采集直接使用内核argv，不受此限制。

脱敏按本轮裁决修订：`/System`、`/usr`、`/bin`、`/sbin`、`/Applications`、
`/Library`、`/opt/homebrew`及根目录`/`保留公共路径语义，不能伪装成家目录。
只有`/Users/<用户>`、`/home/<用户>`前缀换成`~`；其下私有项目段一致映射到
`~/redacted/path-N`，其他未知私有绝对路径用`/redacted/path-N`，不假装位于HOME。
保留`.app/Contents/`、`.venv/bin`、`node_modules`、vite/next/wrangler/tsserver/
jupyter等识别器公开信号、全部合法`--`开关名及Chromium类型与参数位置。
网址、token、未知私有参数值换占位符。普通非用户目录进程名保留；用户目录
可执行文件的未知私有名脱敏。公共系统/应用路径与普通进程名按裁决视为公开，
不再用有限应用名白名单重写它们。tmux标签仍替换，数字身份/资源不改。
此文件已在Phase01首轮review修复中从同一19:40源重新生成；P3起不得再改它。
不会读取agent会话正文或软件配置，原始来源与完整top/command/env不进入日志。

复核：834行，阅读清单69份、2901288016 bytes，Chromium标记56/9/25/12。
603条公共根路径仍在原根下；243条应用可执行路径保留`.app/Contents/`。
原command中另1条以相对argv[0]开头，exe_path未捕获；argv[0]仍保留应用标记。
Docker公开信号2条；vite/vite.js脚本3条。私有项目名中的vite子串仍脱敏，
不为匹配子串总数保留私有名称。自动archive检查逐字段比对源资源，并要求
此fixture与当前安全转换完全一致。
