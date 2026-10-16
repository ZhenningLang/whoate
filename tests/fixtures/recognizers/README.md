# Phase 03 fixture 来源

这些文件不是 owner 的会话、配置或全量进程快照。
原有 capture/attribution fixture 保持不变。

## 本机受控 venv

`venv-spike.json` 由 `phase03_evidence` 的显式录制模式生成。
录制时间为 2026-10-15 23:30:33 UTC。
每个 Python 子进程通过管道先报告 `sys.prefix`、`sys.base_prefix`、
`sys.executable`；随后真实 `MacSampler` 采其 exe、argv0、cwd、comm。
只存这些字段，不存完整 argv 或环境。进程正常退出，临时环境已清理。

临时根替换为 `$FIXTURE`，HOME 替换为 `~`。
本期 workspace 中源证据 SHA256：
`bfcd83a65d080b85c68db806c6991f80f2468e4d06b35a8cb33b588077bd6a32`。
当前 fixture 只是同一 JSON 的排版变换；独立 Python 比较已确认语义相等。

第一次试跑把系统的默认 Python 当基解释器，实测它也处于另一个 venv。
那份试跑不作为 fixture。最终通过 `sys._base_executable` 定位真正基解释器。
基解释器对照使用 `exec -a python`，使 argv0 与激活态一致。
二者采集字段完全相同而 `is_venv` 相反，证明裸入口不可唯一识别。
这是有实证的已知限制，按本期裁决不增采 PATH/VIRTUAL_ENV，也不扫描 cwd 猜测。

## 本机受控 Docker

`docker-spike.json` 由本期 workspace 的 `docker_spike.py` 生成。
2026-10-15 23:29:22 至 23:29:24 UTC，在 30 分钟盒内完成。
探针显式连接本机 `~/.docker/run/docker.sock`，不继承远端 context。
从已有本地镜像起一个禁网、禁止拉取的独立测试容器，采一次 stats，随后只删该容器。
没有停止已有容器或 daemon，没有下载镜像。初始 daemon 未运行，已按 spike 授权启动 Docker Desktop。
未在结束时停止 Desktop，避免影响已经启动的本机服务。

VM footprint 通过 `proc_pid_rusage` v4 取值；布局来自本机 SDK `sys/resource.h`。
`ri_phys_footprint` 位于 UUID 16 字节后的第八个 uint64（偏移72）。
stats 是 Linux 容器的使用量，不等于 VM 的 macOS footprint，更不能相加。
实测容器统计成功，采用独立统计分支，不采用 VM-only 降级。

源证据 SHA256：
`bfad4dfad1b84556e1421e2c312adf5e5648b43599dd2ee8747dfdd5914f6f92`。
fixture 与源 JSON 语义相等。源只含公开进程名、数值与操作状态；
不含镜像、容器名、socket 用户路径、stderr 或既有容器信息。

## 回放分类场景

`phase03_recognizers` 中的进程表是 synthetic。
每行 pid、内存与目标类型由测试手写，预期不调用生产分类函数生成。
其参数来自已批准调研里的公开协议，不声称取自 owner 的真实浏览器或 IDE。

JetBrains 测试 `phase03_jetbrains_runtime_xmx_and_running_version_config_remain_distinct` 为 synthetic；它不来自本机运行中的 PyCharm。

`mixed-holdout.json` 的预期先于 MCP 优先级实现冻结；它实际打红并推动了修复，
因此现在属于开发/回归样本，不能继续冒充未见样本。

`frozen-unseen.json` 在业务实现和 simplify 冻结后新增，首次执行前写入独立预期。
它交叉使用另一种助手、Brave、Docker MCP 入口、IDE 文件监视与语言服务；
同时包含未知内存和同名伪装负例。八个成员已知内存手算为308 bytes，未知一份。
MCP 只含960110/960111，Brave独立标签页只含960120；预期不调用生产分类函数生成。
这仍是自造的混合/负控样本，不是对真实本机分类准确率的抽样证明。
