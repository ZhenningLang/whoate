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
| 读不全 CPU（owner 2026-10-24 R12） | 与内存同一规矩：有进程读不到 CPU 时显示读得到部分之和，数字前加「≥」（如「≥ 6.4%」），悬停「N 个进程读不到 CPU」/「CPU unreadable for N processes」；全部读不到才是「—」（左栏分类合计同理：分类里所有行都读不到时是「—」，空分类是「0.0%」）。面板、中栏、详情头部、复制文本都走 `Format.cpu(_:readable:unknown:)`；曲线按读得到的部分画。按 CPU 排序仍只看 `cpu_pct`（null 排最后） |
| 详情 CPU 曲线（R11） | 内存曲线卡下面一张「CPU · 最近 30 分钟」，高 100 pt，读不到的点断开，不画成 0 |
| 进程群 | 没有历史，不画曲线，也不显示「采样不足」 |

依据：HIG Charts（用系统图表框架，标签清楚，不用颜色单独编码）；HIG Color「Avoid relying solely on color」。

## 10. 行名规则

| 行 | 标题 | 副标题 |
|---|---|---|
| 应用 | 应用显示名（如「Claude」） | 种类词「桌面应用」/「App」· 最大的一个组成部分 · 进程数。只列一个组成部分，并去掉与应用同名的前缀（「Brave Browser Helper (Renderer)」写作「Helper (Renderer)」）；与应用同名的组成部分不列 |
| AI & Agent 会话 | 会话名；工具自己起的名字（如「7-74」）换成会话标题；没有标题时「<工具名> · <目录名>」（R25，如「kilo · oss-atlas」），不用内部编号 | 启动位置 · 工具 · tmux 窗口（pane 标题）· 目录 · 空闲时长 |
| 孤儿 | 进程名（如「wrangler (node)」） | 启动位置（分屏已关闭写「tmux（分屏已关闭）」；判不出来时照旧「启动它的 pane 已关闭」或不写）· 端口 · 目录 |
| tmux pane | `会话:窗口.pane` | 启动位置 · tmux 窗口 · 目录 · 「会话：A、B 等 N 个」（pane 里有会话时，最多列两个，R3） |
| Codex 会话（ChatGPT 里的 Codex，R7） | 会话名；没有名字时「Codex 会话 · <目录名>」，目录也没有时「Codex 会话」 | 同 AI & Agent 会话（启动位置「ChatGPT 桌面版」）；进「AI & Agent」分类 |
| 同名进程群 | 「python3.13 × 74」 | 「分属 22 个归属：Claude Code 会话 15、opencode 会话 4、孤儿进程 2…」，最多列 3 类，其余用「…」；份数已在标题里，不重复 |

同名去重：同一列表里标题相同的行，id 最短的那行保留原标题，其余追加「 · 区分词」。区分词按顺序取：

1. 应用：最大组成部分的进程名，且和标题不同（如「ChatGPT · codex」）；
2. 孤儿、服务、系统：种类词（如「wrangler (node) · 孤儿进程」）；
3. AI & Agent 会话：启动位置，没有时工具名；第二轮取会话号前 8 位或「pid N」，第三轮取序号（R25：界面上不出现 `agent:…:pid:` 这类内部编号，复制文本里可以有）；
4. 都不行时：id 最后一段。

启动位置（R25，owner 2026-10-26）：列表和面板的副标题最前面一段（会话行例外：owner 2026-10-29 把工具名放在最前，写成「claude · Ghostty · tmux main:2.2 · …」，因为窄列会截掉中间，工具名放中间就看不到；标题已以工具名开头时不重复），「怎么判断的」句末「启动位置：X。」用同一个词。数据来自核心可选字段 `origin.launched_from`：

| 来源 | 写法 |
|---|---|
| tmux 分屏 | 「<终端名> · tmux <会话:窗口.分屏>」，如「Ghostty · tmux main:10.1」；终端取这个 tmux 会话最近活跃的客户端所在的终端；没有客户端写「tmux main:10.1（未连接）」；分屏已关闭写「tmux（分屏已关闭）」 |
| 直接在终端里 | 终端名，如「iTerm2」「Ghostty」；Terminal.app 写「终端」 |
| 桌面应用里起的 | 「<应用名> 桌面版」，如「ChatGPT 桌面版」 |
| 由 macOS 启动的桌面应用 | 不写，副标题照旧以「桌面应用」开头 |
| 判不出来 | 不写，也不写「未知」 |

左栏分类（R24，owner 2026-10-26）：「AI & Agent」（中英文同名，取代「助手会话」）收所有会话行，加上 AI 应用名单里的桌面应用整行，这些应用不再出现在「应用」里；一行只属于一个分类，排序与其他分类一样按占用。名单只在 `WhoateKit/MainWindow.swift` 的 `MainWindowModel.aiApps` 一处。

种类词：桌面应用、孤儿进程、后台服务、系统（英文 App、Orphan、Background service、System）。

「进程关系」（R10，owner 2026-10-24，取代 R8 的「为什么归到这里」）：所有种类都显示。主体是自上而下的缩进树，每层缩进 16 pt，前面画「└─」：最上面是来处（tmux 分屏写「tmux 分屏 会话:窗口.pane（窗口名）」，分屏已关闭写「tmux 分屏（已关闭）」，桌面应用写「X（桌面应用）」），往下是归属外的父进程（只有名字，最上面一个若属于某个桌面应用就写成该应用），再往下是归属内进程的父子关系：程序名、能认出的用途（会话标题「…」、识别器部件名如「标签页进程」、MCP 写「外接小工具 名字」）、内存。父进程已退出的写一行「启动它的进程　已退出」；归属只有一个进程时那一行末尾标「← 就是它」。同父同名、没有子进程的兄弟合成一行「名字 ×N」；可见行不超过 12 行，其余收进可展开的「其余 N 个进程 共 X」。下面一句「怎么判断的：…」，只有推测（inferred / weak）时再加「这是推测，可能不准。」。卡片里不出现环境变量名、`%数字`、bundle id、`pid`、证据 kind 原名和「确定 / 推断」；原始证据只放进复制文本。还没拿到进程明细时，树下半部分用组成部分代替。

从 tmux 分屏里用 `open` 打开的应用（R22，owner 2026-10-26）归应用，不画分屏那一行；「怎么判断的」在「X 自己的进程。」后加「它是在 tmux 分屏里打开的，但由 macOS 启动，不归分屏管。」，不写分屏已关闭。

pane 详情（R3）多一张「这个 pane 里的 AI & Agent 会话」卡：每行标题、助手种类、内存，按内存降序，点一行跳到那个会话的详情。ChatGPT 应用详情（R7）多一张「按工作目录分组」卡：没有会话号的进程按工作目录分组，最多 8 组，其余合成「其他」；回放 report 时只有 report 自带 `procs`（设计数据）才有进程明细，否则这张卡不出现、进程关系用组成部分代替。

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

## 12. 内存数字的口径（owner 2026-10-25 R21）

界面上每个内存数字都写明是哪一种；有上限的写「X / 上限 · 百分比」。依据是第 8 轮 spike（本机：所有归属 footprint 之和 36.9G、RSS 之和 9.0G，物理内存已用 19.0G / 24G，压缩 9.1G，swap 9.5G / 10.0G），见第十期 `ack.md`。

| 叫法 | 是什么 | 数据来源 | 出现在 |
|---|---|---|---|
| 物理内存（整机） | 已用 / 总量 · 百分比。主窗口工具栏分两行（物理内存与压缩一行、swap 一行），保证「内存 / CPU」切换始终完整在最右；系统概览左列是标签（物理内存、其中压缩、其中联动、swap），右列只写数值，不再重复标签。已用 = 匿名页 − 可清除页 + wired + 压缩器占用（活动监视器「已使用内存」） | `Host.mem_used_bytes`、`mem_total_bytes`（`sample/sysctl.rs` `host_from_pages`） | 主窗口工具栏、面板头部、系统概览 |
| 其中压缩 | 压缩器占用的物理内存，是「物理内存已用」的一部分 | `Host.compressed_bytes`（compressor_page_count） | 同上 |
| 其中联动 | wired 内存，也是已用的一部分 | `Host.wired_bytes` | 系统概览 |
| swap | 已用 / 当前 swap 文件总量 · 百分比；macOS 会按需扩大 swap 文件。核心没有总量时只写「swap 已用 X」 | `Host.swap_used_bytes`、`swap_total_bytes`（`vm.swapusage`） | 工具栏、面板头部、系统概览 |
| 占用（单个归属、单个进程） | phys_footprint，与活动监视器「内存」列同口径：在物理内存里的部分 + 被压缩或换到 swap 的部分（按压缩前大小）。各归属相加会超过物理内存总量 | `Owner.mem_bytes`、进程 `mem_bytes` | 列表与面板的主数字（列头「按占用排序」，下一行小字「占用 = 物理内存 + 压缩/swap」、面板列表上方「占用 = 物理内存 + 压缩/swap」）、详情「占用 X」、曲线「占用 · 最近 30 分钟」、进程关系 / 组成 / 按工作目录 / pane 会话 / 谁在用等卡片（标题后缀「· 占用」）、关闭确认的进程清单（上方说明） |
| 物理内存约（单个归属） | owner 2026-10-28：主窗口每个内存数都写成「总（物理内存 + 压缩/swap）」：列表行在占用后跟小字「(656M + 4.4G)」（按 CPU 排序时第二行「占用 X」后同样跟，这组数字不压缩，副标题让位），详情写「占用 5.0G（物理内存约 656M + 压缩/swap 约 4.4G）」（详情栏放不下一行时，括号整段移到数字下一行，不从中间折开），左栏分类悬停给同样的拆分；拆不出（有进程读不到、缺 RSS）时只写总数，详情下方一行说明原因。小面板太窄，只写总数。**估算**：`min(RSS, 占用)`。RSS 是该归属进程常驻内存之和，含共享页、会重复计入，所以取不超过它自己的占用；它不是互斥的专属物理内存 | `Owner.reclaimable_physical.bytes` 与 `mem_bytes` | 详情拆分行 |
| 压缩或 swap 约（单个归属） | 占用 − 物理内存约，同样是估算 | 同上相减 | 详情拆分行 |
| 拆分的前提 | 只有占用和 RSS 都完整读到时才拆。有进程读不到占用：写「合计（占用）≥ X · 有进程读不到，拆不出物理内存和压缩/swap」，悬停「N 个进程读不到内存」；全部读不到：「合计（占用）读不到」；只缺 RSS：「合计（占用）X · 物理内存部分读不到」 | `unknown_mem_count`、`reclaimable_physical` | 详情拆分行 |
| 左栏分类合计 | 切到 CPU 时（owner 2026-10-29）改为各行 CPU 之和，按单核算、与行同口径，区块标题「归属 · CPU」，悬停说明可超过 100% 且与右上角整机 CPU 口径不同；有读不到的写「≥」。内存时悬停另给「物理内存约 X + 压缩/swap 约 Y」（各行都能拆时才给，owner 2026-10-28）。R23（owner 2026-10-26 取代 R21 的物理内存估算）：该分类列表里各归属行的占用之和，与中间列表同一过滤、同一数据源，所以分类数不会小于里面任何一行；有读不到的进程时写「≥ X」，悬停同列表。区块标题「归属 · 占用」。「全部」可以超过物理内存总量，悬停说明「占用 = 物理内存 + 压缩/swap，所以各归属相加可以超过物理内存总量」；工具栏悬停说明同步改写 | `Owner.mem_bytes`、`unknown_mem_count` 求和 | 主窗口左栏 |
| 预计腾出（关闭确认） | 要结束的进程常驻内存之和（含共享页），写作「预计腾出约 X 物理内存」 | `reclaimable_physical.bytes` | 关闭确认框 |
| 诊断句里的内存数（详情头部下方） | 核心诊断原文（`verdict/rules.rs`，冻结，CLI/JSON 原样）写作「占 9.70 GiB」「+500 MiB」；GUI 显示时换成全局单位并带口径：「占」改成「占用」，否则在数字后加「（占用）」，如「占用 9.7G」「+500M（占用）」 | `MemoryWording.verdict` | 主窗口详情 |
| 整机物理内存已用的变化（关闭结果） | 关闭前后 `mem_used_bytes` 之差 | kill 结果 | 关闭结果 |

建议卡片里知识库写的阈值（如「此归属超过 2 GiB」）是规则的门槛，不是测得的内存数，不在本表与口径断言之内。

菜单栏图标旁的数字（整机物理内存已用）因菜单栏宽度不加文字标签；它的口径与面板头部第一行相同。

横条（面板头部与系统概览，第 8 轮评审）：物理内存一根条，按总量画「其他已用」和「压缩」两段（两段合起来就是已用）；swap 单独一根细条，按 swap 当前总量画已用。swap 不是物理内存，不画在物理内存条上。没有 swap 总量时不画 swap 条，只在文字里写。


## 12a. 版本号（owner 2026-10-28）

三处显示同一个版本：关于页「版本 0.1.0 · 构建 e12f515 · 2026-10-28 23:22」，小面板头部右上角、主窗口左栏左下角小字「v0.1.0 · e12f515」（左栏右下角是设置齿轮）。提交号与打包时间由 `scripts/bundle-app.sh` 写进 Info.plist 的 `WhoateCommit`（`git describe --always --dirty`，工作树有改动时带 `-dirty`）与 `WhoateBuiltAt`；没有这两个键（`swift run`、测试）时写「开发版」。

## 13. 设置窗口（Phase 11）

标准 macOS 设置窗口：SwiftUI `Settings` 场景，五个分页（通用、菜单栏、通知、命令行、关于），每页一个分组表单（`Form` + `.formStyle(.grouped)`），控件全用系统的 `Toggle`、`Picker`、`Button`、`LabeledContent`，不自造样式、不写死字号和颜色。窗口宽 540 pt，高度随内容。

- 入口三个，走同一个打开动作：面板底栏齿轮、应用菜单「设置…」/「Settings…」（⌘,，文案走 L10n）、自动化 `WHOATE_UI_OPEN_SETTINGS_AFTER`。打开后显式激活应用，设置窗口算进激活策略（开着时是 regular，有 Dock 图标、⌘Tab 可达），但不算主窗口。
- 齿轮放在底栏右侧「完整面板」左边，无边框、次要色，悬停与读屏都是「设置」。底栏状态文字最多两行、居中，英文放不下一行时折成两行，不从中间截断（2026-10-29）。退出仍单独在最左（第 5 节 owner 设计关卡的位置），避免误点退出。
- 选项文字：选项用名词（跟随系统 / 浅色 / 深色），说明文字用 `.callout` 次要色，写清效果（如省电档的三个间隔）。
- 菜单栏「只显示图标」时，菜单栏项的读屏标签仍读出用量和压力。
- 关于页的「支持」区：列表为空时整块不显示；有项时链接是按钮，二维码是 64 pt 小图，点开在弹出框里放大。不放占位链接或示例二维码。
- 首次启动提示：菜单栏项下方的弹出框（8 秒或点别处消失）；找不到菜单栏项或它被刘海挡住时，改在主窗口顶部放一条带「知道了」的横幅（`.bar` 材质，下边一条分隔线）。

依据：HIG Settings「Make settings easy to find」「Avoid putting settings in multiple places」（一个设置窗口，三个入口都打开它）与「Use the standard Settings menu item and its ⌘, shortcut」；HIG Toggles「Use a switch … to toggle the state of a single setting」；HIG Pickers（互斥选项用 picker，选项少时用单选组）；HIG Accessibility「Provide alternative text labels for icons」（齿轮与只显示图标的菜单栏项）。
