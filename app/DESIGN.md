# whoate GUI 设计契约

本文件规定菜单栏项、面板、三栏主窗口用什么字号、间距、颜色、材质和控件。每一节写「用什么」和「依据哪条 HIG」。HIG 原文摘录见 Phase 10 `research.md` F7，读取日期 2026-10-20。

带引号的依据是 F7 读过的原文（Typography、Color、Materials、Buttons、Sidebars、The menu bar）。只写页面名、不带引号的（Layout、Charts、Writing、Dark Mode、Accessibility、Localization）是按页面主题引用，本期没有逐字核对原文。

适用范围：`app/Sources/Whoate/`。代码里出现的写死字号或十六进制颜色，必须登记在文末「例外」表里，否则 verify 的静态扫描判失败。

设计稿：Phase 10 目录 `design/round-<N>/`，由 `tests/gui/design_shots.py` 用真实 SwiftUI 渲染并截图。

## 1. 字号层级

只用 macOS 内置文本样式，不写 `.font(.system(size:))`。需要加粗时用 `.weight(...)`，数字一律 `.monospacedDigit()`。

| 位置 | 文本样式 | macOS 字号（pt） |
|---|---|---|
| 面板头部结论（「内存偏紧」「整机 CPU 37%」） | `.title3.weight(.semibold)` | 15 |
| 面板头部数字、CPU 负载行、底栏状态文字 | `.callout` | 12 |
| 面板图例、胶囊 | `.subheadline` | 11 |
| 行标题、行内数值 | `.headline` | 13 粗 |
| 行副标题 | `.callout`，次要色 | 12 |
| 「采样不足」 | `.caption` | 10 |
| 主窗口侧栏条目 | 系统侧栏默认（`.body`） | 13 |
| 主窗口中栏表头 | `.subheadline`，次要色 | 11 |
| 详情标题 | `.title2.weight(.semibold)` | 17 |
| 详情卡标题 | `.headline` | 13 粗 |
| 详情卡正文、键值 | `.callout` | 12 |
| 菜单栏项文字 | 系统菜单栏字体，不设字号 | 系统决定 |

依据：HIG Typography（macOS 内置文本样式表：Title 2 17、Title 3 15、Headline 13 粗、Body 13、Callout 12、Subheadline 11、Footnote/Caption 10；macOS 默认 13 pt，最小 10 pt）。

## 2. 间距

全部取 4 pt 的倍数。

| 位置 | 值（pt） |
|---|---|
| 面板宽度 | 420 |
| 面板头部、底栏内边距 | 左右 16，上下 12 |
| 面板行内边距 | 左右 16，上下 12 |
| 面板行内：标题与副标题 | 4 |
| 面板行内：第一行与按钮行、控件之间 | 8 |
| 面板行之间的分隔线 | 左缩进 16 |
| 详情「进程关系」树每一层缩进 | 16 |
| 主窗口侧栏宽 | 180–260，默认 200 |
| 主窗口中栏宽 | 320–480，默认 360 |
| 中栏表头 | 左右 16，上下 8 |
| 中栏行：三行之间 | 4，行上下各 4（另加系统列表行距） |
| 详情区内边距 | 20 |
| 详情卡之间 | 16 |
| 详情卡内边距 | 8（GroupBox 自带的内边距之外） |
| 详情卡键值：列间 / 行间 | 16 / 8 |
| 底部按钮之间 | 8；破坏性按钮与其他按钮之间至少 16 |

依据：HIG Layout（用一致的间距和对齐建立层次）；HIG Buttons「It's essential to include enough space around a button so that people can visually distinguish it from surrounding components」。

## 3. 颜色

只用系统颜色，不自造色板，不写颜色值。`Palette.swift` 的十六进制色板已退役，现在只把含义映射到系统色。

| 含义 | 颜色 | 第二信号（不只靠颜色） |
|---|---|---|
| act（建议关闭） | 系统橙 `.orange` | 胶囊里的 `exclamationmark.triangle.fill` 符号 + 文字 |
| watch（留意、还在涨、空闲） | 系统黄 `.yellow` | `exclamationmark.circle.fill` 符号 + 文字 |
| 只有建议、本身正常 | 系统黄 | `lightbulb.fill` 符号 + 「有 N 条建议」 |
| ok | 次要标签色 `.secondary` | 胶囊文字「正常」等 |
| 破坏性按钮（关闭、强制结束、关闭 N 个进程） | 系统红（`role: .destructive`） | 按钮文字 |
| 正文、标题 | `.primary` / `.secondary` / `.tertiary` | — |
| 分隔线、卡片描边 | `Divider()`、`.separator` | — |
| 内存条：已用 / 压缩 / swap / 底槽 | `.systemGray` / `.systemGray` 45% / 系统黄 / `.quaternary` | 图例文字 |

胶囊文字用 `.primary`（act、watch）或 `.secondary`（ok），不用黄色文字：黄色字在浅色背景上看不清。颜色只上符号和 15% 的底色。

依据：HIG Color「Avoid hard-coding system color values in your app」「Avoid redefining the semantic meanings of dynamic system colors」「Avoid relying solely on color to differentiate between objects」。

## 4. 材质

| 位置 | 材质或背景 |
|---|---|
| 面板 | `MenuBarExtra(.window)` 的系统窗口材质，视图本身不铺背景 |
| 主窗口侧栏 | `List` + `.listStyle(.sidebar)`，系统侧栏材质 |
| 中栏 | `List` + `.listStyle(.inset)`，系统内容背景 |
| 详情区 | 窗口背景 |
| 详情卡 | `GroupBox`（系统分组背景），不用自绘圆角卡片 |
| 关闭确认（R16，owner 2026-10-25） | 主窗口：系统 sheet，macOS 负责居中与键盘焦点，宽 440；后面整窗（三栏）盖一层 40% 黑。面板：同一张卡片（`CloseCard`）放在面板正中，`.regularMaterial` 圆角 12，描边 `.separator`，阴影，后面同样 40% 黑。两处各管自己发起的关闭，不互相弹出 |
| 内容层 | 不用 Liquid Glass |

依据：HIG Materials「Don't use Liquid Glass in the content layer」；侧栏、弹出面板这类组件交给系统材质。HIG Sidebars（侧栏用系统样式）。

## 5. 按钮

| 按钮 | 样式 | 位置 |
|---|---|---|
| 完整面板（Full Panel） | 标准按钮（`.bordered`，常规尺寸）；主窗口有没有都叫这个词 | 面板底栏最右 |
| 退出（Quit） | 独立按钮，无边框（`.borderless`）、次要色文字，保留 ⌘Q | 面板底栏最左，与「完整面板」隔着整条状态文字 |
| 行内「详情」 | 标准按钮，小尺寸 | 面板行第二行右侧 |
| 行内「关闭」 | 标准按钮 + `role: .destructive`，文字系统红（有边框的标准按钮不吃 tint，所以直接给文字上色），小尺寸，不当主按钮 | 紧跟「详情」 |
| 确认框「结束进程」「强制结束」 | `role: .destructive`，红色填充（`.borderedProminent` + `.tint(.red)`），大尺寸；不设为默认按钮，回车不触发；「取消」「完成」绑 Esc | 确认框右下，在「取消」右边 |
| 详情「跳到这个 pane」 | 标准按钮；不在 tmux 的会话按钮置灰，旁边写「不在 tmux 里」 | 详情底部最左 |
| 详情「复制相关信息」（Copy Details） | 标准按钮；写剪贴板后旁边显示「已复制」2 秒 | 跳转按钮右侧 |
| 详情「关闭 N 个进程…」 | `role: .destructive` + `.borderedProminent` + `.tint(.red)`（红底；窗口不在前台时系统把它画成灰底，和其他突出按钮一样），不设为默认按钮，只打开两段确认；系统进程置灰并提示「系统进程，勿关」 | 详情底部最右，和其他按钮之间用弹性空白隔开 |

详情底部只有这三个按钮（owner 2026-10-21 点验 R6）：「复制命令」「在 Finder 中显示目录」都已删除；系统服务卡里的重启命令只是可选中的文字，不带按钮；「复制相关信息」的文本不含它。

「复制相关信息」的文本（R4）给用户自己的 AI 判断保留还是关闭：第一行说明来意并声明「不含对话内容」，之后是软件/会话、种类、内存（读不全时带「≥」）、CPU、工作目录、tmux 位置、归属理由（白话句与进程关系树，R10）、原始证据（kind、说明、可信度，只在这里出现）、建议、进程列表（pid、名字、命令行，按内存降序最多 30 个，其余写「另有 N 个未列出」）。命令行只用核心 `whoate_why` 给的、按 fixture 规则脱敏过的值；没有时写「命令行不可用」。不放会话首句和回复。

退出入口（反馈 3，owner 2026-10-21 设计关卡裁决「保留独立按钮」）：退出仍是按钮，放在底栏最左，无边框、次要色；「完整面板」是有边框的标准按钮，放在最右；中间是状态文字。两个按钮中心距远大于 60 pt（实测约 310 pt），样式一眼可分，点「完整面板」不会碰到退出。⌘Q 仍走同一个 `QuitAction`。点「完整面板」或行内「详情」后面板自动收起（反馈 2）。按钮文字按 owner 2026-10-21 点验 R1 定为「退出」「完整面板」（Quit、Full Panel）。

面板行双击（owner 2026-10-25 点验 R14）：双击行的任意空白或文字处，和行内「详情」走同一条路（打开或提前主窗口、左栏切到所属分类、中栏选中并滚到中间、面板收起）；同类进程群行没有按钮，双击是它打开主窗口的入口，主窗口里没有这一行时选它占内存最大的成员归属，都没有就只开窗口。「详情」「关闭」、建议胶囊保留自己的点击，双击它们不触发；单击行不做任何事。

主窗口标题（owner 2026-10-25 复查 R15，推翻 R13 的「图标＋whoate」）：工具栏最左只有大写字标「WHO-ATE」，就是系统画的窗口标题，中英文界面相同；前面不放图标，不另画字标，不加计数或分类名。Window 菜单、调度中心里的窗口名也是「WHO-ATE」；app 名、Dock 名、菜单栏和小面板仍是 whoate。不隐藏系统标题：实测隐藏后工具栏变成紧凑高度，右侧的压力胶囊和「内存 / CPU」被拉到左边。

关闭确认的内容（owner 2026-10-25 R16）：标题「关闭「名字」？」；一句白话「会结束 N 个进程，预计腾出约 X 内存。」（测不到时说「腾出多少内存无法预估。」）；进程清单每行「程序名　内存　进程号 N」，进程号是行尾的次要文字，超过 6 行变成可滚动区域；有估算时下面一行次要色说明「按常驻内存估算，含共享部分，实际腾出的可能更少。」。说明文字不低于 `.secondary`。第二步（5 秒后仍未退出）、等待中、失败都用同一张卡片和同一位置；发信号期间 sheet 不能被 Esc 关掉。

依据：HIG Buttons「Don't assign the primary role to a button that performs a destructive action」「a destructive button uses the system red color」「It's essential to include enough space around a button…」。HIG 没有退出入口的专门条款；收进「⋯」菜单是 macOS 菜单栏工具的常见做法（推断，不是 HIG 原文）。

## 6. 菜单栏项

- 图标（owner 2026-10-25 R20 定稿）：单色「被咬掉一口的饼图」，`assets/logo/menubar-template.pdf`，作为模板图（`isTemplate = true`），18×18 pt；系统按菜单栏明暗给它着色。PDF 由 `scripts/bundle-app.sh icons` 从 `logo-template.svg` 生成：先渲染成 1024 像素位图再用 potrace 描成纯矢量路径，因为 rsvg 直接转出的 PDF 把 SVG 遮罩写成软遮罩，AppKit 画出来是空的（实测）。
- 文字：已用内存（如「41.2G」），系统菜单栏字体，菜单栏前景色，等宽数字。不加颜色、不画位图。
- 删除 08a 的彩色位图路径（`MenuBarLabel.render(text:color:)`）。
- 设计稿里发现：旧版 `menubar-template.pdf` 缩到 18×14 pt 后看起来就是一串点「‥‥」。owner 反馈 5 说的「….」很可能是这个图标本身，不是系统截断（推断：依据是设计稿画廊截图，没在真菜单栏对照）。R20 换成饼图后不再有这个问题。
- 告警态（内存偏紧、内存紧张）四个候选，都只用形状或文字表达，不用颜色：

| 方案 | 平时 | 偏紧 | 紧张 | 优点 | 缺点 |
|---|---|---|---|---|---|
| A 符号 | whoate 图标 | `exclamationmark.triangle` | `exclamationmark.triangle.fill` | 宽度不变；一眼能看出变了 | 平时的图标仍像「….」 |
| B 前缀 | whoate 图标 | 数字前加「!」 | 数字前加「‼」 | 图标不变 | 符号小，不容易注意到；图标仍像「….」 |
| C 压力词 | whoate 图标 | 数字后加「偏紧」 | 数字后加「紧张」 | 意思最直白 | 变宽，更容易被截断；图标仍像「….」 |
| D 系统符号（推荐） | `memorychip` | 同 A | 同 A | 平时也是清楚的系统符号，不再像「….」；告警同 A | 菜单栏上不再出现 whoate 自己的图形 |

owner 2026-10-21 设计关卡选定 **方案 D**。R20（2026-10-25）起平时一律显示饼图 logo，不再用 `memorychip`；告警仍按 D（同 A）换成三角符号。四个方案仍保留在自动化画廊里，便于以后对照。菜单栏图像一律经 `MenuBarItem.image` 生成，任何状态下都是模板图。

依据：HIG The menu bar → Menu bar extras「Consider using a symbol to represent your menu bar extra… the system can apply other colors to the black areas in each image so it looks good on both dark and light menu bars」「The menu bar's height is 24 pt」；HIG Color「Avoid relying solely on color」。

## 7. 面板保留弹出形式的理由

HIG 默认要求点菜单栏项弹出菜单，不是弹出面板。whoate 属于 HIG 写明的例外「Unless the app functionality you want to expose is too complex for a menu」：

- 每行有迷你曲线和风险胶囊，菜单项画不了曲线；
- 关闭是两段式（确认框列出每个进程和预估，5 秒后问是否强制），需要在原地显示进度和结果；
- 头部有内存条和内存/CPU 切换。

所以保留 `MenuBarExtra(.window)` 弹出面板。面板里只放这些「菜单做不到」的内容，其余（详情、概览、建议出处）放主窗口。

依据：HIG The menu bar → Menu bar extras「Display a menu — not a popover — when people click your menu bar extra. Unless the app functionality you want to expose is too complex for a menu」。

## 8. 风险标签

面板行、主窗口中栏行、详情头部用同一个 `Pill` 视图，输入同一个 `PanelRow`（反馈 4）。详情至少包含面板行的标题、副标题、胶囊、数值，并展开全部建议原文和出处。

| 档位 | 胶囊文字（示例） | 符号 | 颜色 |
|---|---|---|---|
| act | 建议关闭 | `exclamationmark.triangle.fill` | 系统橙符号，橙色 15% 底 |
| watch | 留意 / 还在涨 / 疑似泄漏 / 空闲 3 小时 | `exclamationmark.circle.fill` | 系统黄符号，黄色 15% 底 |
| 正常但有建议 | 有 2 条建议 | `lightbulb.fill` | 系统黄符号，黄色 15% 底 |
| ok | 正常 / 系统进程 / 系统服务 | 无 | 次要色文字，`.quaternary` 底 |

可信度胶囊（详情的证据、会话卡）写 certain、inferred、weak 原词，次要色，`.quaternary` 底，不用颜色区分。

依据：HIG Color「Avoid relying solely on color to differentiate between objects」；HIG Accessibility（符号与文字并用）。

## 9. 曲线

| 项 | 规则 |
|---|---|
| 面板与中栏迷你曲线 | 自绘 `Path`，线宽 1.5，圆头圆角；面板 64×18 pt，中栏 72×16 pt |
| 详情曲线 | Swift Charts `LineMark`，线宽 1.5，高 140 pt，纵轴标签用 `Format.bytes` |
| 颜色 | 与风险档一致：act 橙、watch 黄、其余次要色 |
| 断点 | 相邻两点间隔 > 45 秒就断开；迷你曲线只画最后一段，详情曲线每段各画一条线，缺口留空，不插值 |
| 「采样不足」 | 只在最后一段不足 30 秒时出现，用 `.caption` 次要色文字代替曲线 |
| 读不到内存 | 曲线照常画（只计读得到的进程），旁边不放图标。面板行、中栏行、详情头部、组成卡的内存数字前加「≥」（如「≥ 596M」，owner 2026-10-21 点验 R5），都走 `Format.memory`；悬停文案：「N 个进程读不到内存」/「N processes unreadable」 |
| CPU 模式（owner D4：画 CPU） | CPU 模式下迷你曲线画 CPU 百分比（history 点里的 `cpu_pct`，缺值的点不画），数值也是 CPU；内存模式画内存 |
| 读不全 CPU（owner 2026-10-24 R12） | 与内存同一规矩：有进程读不到 CPU 时显示读得到部分之和，数字前加「≥」（如「≥ 6.4%」），悬停「N 个进程读不到 CPU」/「CPU unreadable for N processes」；全部读不到才是「—」。面板、中栏、详情头部、复制文本都走 `Format.cpu(_:readable:unknown:)`；曲线按读得到的部分画。按 CPU 排序仍只看 `cpu_pct`（null 排最后） |
| 详情 CPU 曲线（R11） | 内存曲线卡下面一张「CPU · 最近 30 分钟」，高 100 pt，读不到的点断开，不画成 0 |
| 进程群 | 没有历史，不画曲线，也不显示「采样不足」 |

依据：HIG Charts（用系统图表框架，标签清楚，不用颜色单独编码）；HIG Color「Avoid relying solely on color」。

## 10. 行名规则

| 行 | 标题 | 副标题 |
|---|---|---|
| 应用 | 应用显示名（如「Claude」） | 种类词「桌面应用」/「App」· 最大的一个组成部分 · 进程数。只列一个组成部分，并去掉与应用同名的前缀（「Brave Browser Helper (Renderer)」写作「Helper (Renderer)」）；与应用同名的组成部分不列 |
| 助手会话 | 会话名；工具自己起的名字（如「7-74」）换成会话标题 | 工具 · tmux 窗口（pane 标题）· 目录 · 空闲时长 |
| 孤儿 | 进程名（如「wrangler (node)」） | 启动它的 pane 已关闭 · 端口 · 目录 |
| tmux pane | `会话:窗口.pane` | tmux 窗口 · 目录 · 「会话：A、B 等 N 个」（pane 里有助手会话时，最多列两个，R3） |
| Codex 会话（ChatGPT 里的 Codex，R7） | 会话名；没有名字时「Codex 会话 · <目录名>」，目录也没有时「Codex 会话」 | 同助手会话；进「助手会话」分类 |
| 同名进程群 | 「python3.13 × 74」 | 「分属 22 个归属：Claude Code 会话 15、opencode 会话 4、孤儿进程 2…」，最多列 3 类，其余用「…」；份数已在标题里，不重复 |

同名去重：同一列表里标题相同的行，id 最短的那行保留原标题，其余追加「 · 区分词」。区分词按顺序取：

1. 应用：最大组成部分的进程名，且和标题不同（如「ChatGPT · codex」）；
2. 孤儿、服务、系统：种类词（如「wrangler (node) · 孤儿进程」）；
3. 都不行时：id 最后一段。

种类词：桌面应用、孤儿进程、后台服务、系统（英文 App、Orphan、Background service、System）。

「进程关系」（R10，owner 2026-10-24，取代 R8 的「为什么归到这里」）：所有种类都显示。主体是自上而下的缩进树，每层缩进 16 pt，前面画「└─」：最上面是来处（tmux 分屏写「tmux 分屏 会话:窗口.pane（窗口名）」，分屏已关闭写「tmux 分屏（已关闭）」，桌面应用写「X（桌面应用）」），往下是归属外的父进程（只有名字，最上面一个若属于某个桌面应用就写成该应用），再往下是归属内进程的父子关系：程序名、能认出的用途（会话标题「…」、识别器部件名如「标签页进程」、MCP 写「外接小工具 名字」）、内存。父进程已退出的写一行「启动它的进程　已退出」；归属只有一个进程时那一行末尾标「← 就是它」。同父同名、没有子进程的兄弟合成一行「名字 ×N」；可见行不超过 12 行，其余收进可展开的「其余 N 个进程 共 X」。下面一句「怎么判断的：…」，只有推测（inferred / weak）时再加「这是推测，可能不准。」。卡片里不出现环境变量名、`%数字`、bundle id、`pid`、证据 kind 原名和「确定 / 推断」；原始证据只放进复制文本。还没拿到进程明细时，树下半部分用组成部分代替。

pane 详情（R3）多一张「这个 pane 里的助手会话」卡：每行标题、助手种类、内存，按内存降序，点一行跳到那个会话的详情。ChatGPT 应用详情（R7）多一张「按工作目录分组」卡：没有会话号的进程按工作目录分组，最多 8 组，其余合成「其他」；回放 report 时只有 report 自带 `procs`（设计数据）才有进程明细，否则这张卡不出现、进程关系用组成部分代替。

详情里不出现引擎内部词：证据种类（如 `app_bundle`）、会话状态（如 `working`）、可信度（`certain` 等）都换成中英对照的人话（`WhoateKit/Words.swift`）；查不到对照的词原样显示，不隐藏。

依据：HIG Writing（用户能看懂的名字，不用内部 id）；HIG Sidebars（列表项用简短、具体的名字）。

## 11. 深色模式与中英文

- 所有颜色是系统色和语义色，浅色、深色自动切换，不分别写两套。设计稿与截图都出浅色、深色两套。
- 中英文由 `Language.system()` 统一决定，界面文字全部走 `L10n`，英文界面除数据字段外不得出现中文（WhoateChecks `englishTextHasNoCJK`）。
- 截断：标题、副标题、状态文字单行，中间省略（`.truncationMode(.middle)`），保住开头的名字和结尾的数字；进程群副标题例外，截尾，保住开头的「分属 N 个归属：最大的一类」；详情标题最多两行；会话首句最多三行；建议原文、证据说明不截断，自动换行。
- 按钮文字不截断：面板底栏只放一个按钮和一个「⋯」菜单；详情底部操作条固定在详情栏底部，不随内容滚动，按钮按内容定宽。

依据：HIG Dark Mode（用语义色适配两种外观）；HIG Right to left / Localization（为文字变长留空间）；HIG Typography（截断时保留关键信息）。

## 例外

静态扫描（verify.sh QA18）在 `app/Sources/Whoate/` 里找 `.font(.system(size:`、`hex(0x`、`NSColor(srgbRed`、`Color(red:`。命中只能是下表登记的行，按「文件 + 片段」放行。

| 文件 | 片段 | 理由 |
|---|---|---|
| （无） | — | 目前没有例外 |
