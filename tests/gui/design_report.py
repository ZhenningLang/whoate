#!/usr/bin/env python3
"""Writes tests/gui/design-report.json: the whoate/v1 report the Phase 10
design gate replays (debug build, WHOATE_UI_FIXTURE). Every name, path, title
and pid is made up; nothing comes from a real machine.

Covers: ChatGPT and its Codex background service (same label, for the
duplicate-name rule), the Claude desktop app, a Claude Code session in tmux
with insights and an auto-generated title, an opencode session, an act-level
orphan with a port, a watch-level Brave with 3 unreadable processes, a
python3.13 group spread over 22 owners, a system process and a launchd
service, and a host with cpu, wired, kernel zones, GPU and diagnostics.
Round 3 (owner check 2026-10-21): a Codex session run by ChatGPT, tmux pane
%22 running two agent sessions, and an opencode whose pane was closed (no
session; grouped under the agent since the D1 change, not an orphan).
Round 4 (owner check 2026-10-24): Brave with three processes whose CPU is
unreadable (R12), the pane %15 the Claude Code session runs in, live panes'
`tmux_target`, and made-up `procs` that debug replay hands to whoate_why for
the process trees (R10), with real-machine kernel names (review B:B1).
History: 30 minutes at 20 s, the Claude Code session with one 120 s gap.

  design_report.py [out]    (default: tests/gui/design-report.json next to this file)
"""
import json, os, sys

END = 1792652400  # fixed, so the file is reproducible
STEP, SPAN = 20, 1800
G, M = 1 << 30, 1 << 20


def history(start, end, cpu=1.0, unknown=None, gap=None, cpu_unknown=None):
    """Linear from start to end bytes over 30 minutes; `gap` = (from, to) seconds before END left out.
    `cpu_unknown` processes without CPU (R12): cpu_pct null, the readable part beside it."""
    points = []
    for t in range(END - SPAN, END + 1, STEP):
        if gap and END - gap[0] < t < END - gap[1]:
            continue
        f = (t - (END - SPAN)) / SPAN
        p = {"timestamp_s": t, "mem_bytes": int(start + (end - start) * f), "cpu_pct": round(cpu * (0.8 + 0.4 * f), 1)}
        if unknown is not None:
            p["unknown_mem_count"] = unknown
        if cpu_unknown is not None:
            p["cpu_pct_readable"], p["cpu_pct"], p["unknown_cpu_count"] = p["cpu_pct"], None, cpu_unknown
        points.append(p)
    return points


def ramp_then_flat(peak):
    """The orphan: grew in the first 10 minutes, flat since."""
    points = []
    for t in range(END - SPAN, END + 1, STEP):
        f = min(1.0, (t - (END - SPAN)) / 600)
        points.append({"timestamp_s": t, "mem_bytes": int(peak * (0.55 + 0.45 * f)), "cpu_pct": 0.1})
    return points


def part(name, mem, count, first_pid, unknown=0):
    return {"name": name, "mem_bytes": int(mem), "process_count": count,
            "pids": list(range(first_pid, first_pid + count)), "unknown_mem_count": unknown}


def owner(id, kind, label, mem, cpu, pids, verdict, origin=None, agent=None, evidence=(), parts=(), insights=(),
          hist=(), unknown=0, cpu_unknown=None):
    o = {"id": id, "kind": kind, "label": label, "mem_bytes": int(mem), "unknown_mem_count": unknown,
         "cpu_pct": cpu, "pids": pids, "origin": origin, "agent": agent, "evidence": list(evidence),
         "parts": list(parts), "facts": [], "insights": list(insights), "history": list(hist), "verdict": verdict}
    if cpu_unknown is not None:  # R12: `cpu` is then only the readable part
        o["cpu_pct"], o["cpu_pct_readable"], o["unknown_cpu_count"] = None, cpu, cpu_unknown
    return o


def proc(pid, ppid, comm, mem=None, member=True, name=None):
    """A whoate_why process (R10 screenshots) in the real machine's shape: `comm` is the
    kernel's short name (Claude Code's is its version, others cut at 16 bytes) and
    `name` the core's readable one; members carry argv-free memory here, parents too."""
    p = {"pid": pid, "ppid": ppid, "comm": comm, "name": name or comm, "in_owner": member,
         "mem_bytes": None if mem is None else int(mem)}
    return p


def helper(pid, ppid, name, mem):
    """A Chromium helper: the kernel keeps only its first 16 bytes."""
    return proc(pid, ppid, name[:16], mem, name=name)


def procs():
    """Made-up process trees for the screens that show the process relation card."""
    out = [proc(969000, 1, "tmux", member=False), proc(969999, 969000, "zsh", 12 * M, member=False),
           # Claude Code session in pane %15: zsh → claude → its tools.
           proc(970000, 969999, "2.1.306", 0.6 * G, name="claude"), proc(970001, 970000, "zsh", 0.05 * G),
           proc(970007, 970000, "zsh", 0.05 * G), proc(970005, 970001, "python3.13", 0.15 * G),
           proc(970006, 970001, "python3.13", 0.15 * G)]
    out += [proc(970002 + i, 970000, "node", 0.4 * G) for i in range(3)]
    # Brave: the main process and its helpers; three renderers unreadable.
    out += [proc(951000, 1, "Brave Browser", 0.5 * G), helper(951002, 951000, "Brave Browser Helper (GPU)", 0.6 * G)]
    out += [helper(951004 + i, 951000, "Brave Browser Helper (Renderer)", None if i < 3 else 3.1 * G / 11)
            for i in range(14)]
    out += [helper(pid, 951000, "Brave Browser Helper", 20 * M) for pid in (951001, 951003, 951018)]
    # The orphan: its parent exited, the pane it ran in is closed.
    out += [proc(948211, 1, "node", 7.5 * G), proc(948214, 948211, "node", 1.0 * G),
            proc(948215, 948211, "node", 1.0 * G), proc(948219, 948214, "esbuild", 0.2 * G)]
    # Pane %22 and the Codex session ChatGPT runs.
    out += [proc(968000, 1, "tmux", member=False), proc(972000, 968000, "zsh", 0.1 * G),
            proc(972001, 972000, "cargo", 0.2 * G)]
    out += [proc(960000, 1, "ChatGPT", member=False), proc(960001, 960000, "codex", member=False),
            proc(960100, 960001, "python3.13", 0.25 * G), proc(960101, 960001, "python3.13", 0.25 * G),
            proc(960102, 960001, "node", 0.2 * G)]
    return out


def verdict(level, rule, text, action=None):
    v = {"level": level, "rule": rule, "text": text}
    if action:
        v["action"] = action
    return v


def ev(kind, detail, confidence):
    return {"kind": kind, "detail": detail, "confidence": confidence}


def agent(tool, session, name, source, title, prompt, reply, status, cwd, branch, idle):
    return {"tool": tool, "session_id": session, "name": name, "name_source": source, "title": title,
            "first_prompt": prompt, "first_reply": reply, "status": status, "cwd": cwd, "branch": branch,
            "model": None, "cost_usd": None, "confidence": "certain", "updated_at_ms": (END - idle) * 1000,
            "last_activity_at": None, "idle_seconds": idle, "stale": False}


def codex_agent(session, name, cwd, branch, updated_s):
    """Codex sessions carry the session table's name, cwd and branch; never conversation text."""
    return {"tool": "codex", "session_id": session, "name": name, "name_source": "codex", "title": None,
            "first_prompt": None, "first_reply": None, "status": None, "cwd": cwd, "branch": branch,
            "model": None, "cost_usd": None, "confidence": "certain", "updated_at_ms": updated_s * 1000, "stale": False}


def report():
    owners = [
        owner("orphan:948211", "orphan", "wrangler (node)", 9.7 * G, 0.1, [948211, 948214, 948215, 948219],
              verdict("act", "orphan-dev-server", "启动它的 tmux pane %266 已关闭，端口 8799 当前 0 个连接；启动后 10 分钟内涨到 9.7G，之后不再变化。",
                      "whoate kill 948211 948214 948215 948219"),
              origin={"tmux_pane": "%266", "pane_alive": False, "cwd": "/Users/demo/src/site/worker", "ports": [8799]},
              evidence=[ev("tmux_environment", "%266", "certain"),
                                                ev("runtime_home", "runtime in HOME; launchd ancestry", "weak")],
              parts=[part("node", 9.5 * G, 3, 948211), part("esbuild", 0.2 * G, 1, 948219)],
              hist=ramp_then_flat(9.7 * G)),
        owner("app:com.brave.Browser", "app", "Brave Browser", 4.2 * G, 6.4, list(range(951000, 951019)),
              verdict("watch", "fast-growth", "近 30 分钟 +600M，渲染进程 14 个。"),
              evidence=[ev("app_bundle", "com.brave.Browser", "certain")],
              parts=[part("Brave Browser Helper (Renderer)", 3.1 * G, 14, 951004, unknown=3),
                     part("Brave Browser Helper (GPU)", 0.6 * G, 1, 951002), part("Brave Browser", 0.5 * G, 1, 951000)],
              insights=["brave.memory-saver-off", "brave.extensions"],
              hist=history(3.6 * G, 4.2 * G, cpu=6, unknown=3, cpu_unknown=3), unknown=3, cpu_unknown=3),
        owner("app:com.openai.codex", "app", "ChatGPT", 3.1 * G, 4.2, list(range(960000, 960020)),
              verdict("ok", "default", "正常"),
              evidence=[ev("app_bundle", "com.openai.codex", "certain")],
              parts=[part("ChatGPT Helper (Renderer)", 1.2 * G, 6, 960002), part("node", 1.1 * G, 12, 960008),
                     part("ChatGPT", 0.5 * G, 1, 960000), part("codex", 0.3 * G, 1, 960001)],
              hist=history(3.0 * G, 3.1 * G, cpu=4)),
        owner("app:com.openai.codex.cli", "app", "ChatGPT", 0.4 * G, 0.3, [961000, 961001],
              verdict("ok", "default", "正常"),
              evidence=[ev("app_bundle", "com.openai.codex.cli", "certain")],
              parts=[part("codex", 0.4 * G, 2, 961000)], hist=history(0.4 * G, 0.4 * G, cpu=0.3)),
        owner("app:com.anthropic.claudefordesktop", "app", "Claude", 1.6 * G, 2.1, list(range(962000, 962007)),
              verdict("ok", "default", "正常"),
              evidence=[ev("app_bundle", "com.anthropic.claudefordesktop", "certain")],
              parts=[part("Claude Helper (Renderer)", 1.0 * G, 5, 962002), part("Claude", 0.6 * G, 1, 962000)],
              hist=history(1.5 * G, 1.6 * G, cpu=2)),
        owner("agent:claude:9f2c41d7-5b0e-4c1a-9d3e-2a7f60c1b8e4", "agent_session", "给归属规则补回归测试", 2.6 * G, 31.0,
              list(range(970000, 970008)),
              verdict("watch", "fast-growth", "近 10 分钟 +400M，还在涨。"),
              origin={"tmux_pane": "%15", "pane_alive": True, "tmux_target": "main:1.0", "cwd": "/Users/demo/src/atlas", "ports": [5173],
                      "window_name": "main", "pane_title": "claude"},
              agent=agent("claude", "9f2c41d7-5b0e-4c1a-9d3e-2a7f60c1b8e4", "7-74", "derived", "给归属规则补回归测试",
                          "孤儿判定把应用的子进程也算进去了，帮我查一下原因",
                          "先读 attribute 模块的分支顺序。runtime_home 排在 .app 检查前面，所以挂在应用下面的 node 也被当成了孤儿……",
                          "working", "/Users/demo/src/atlas", "fix/orphan-chain", 40),
              evidence=[ev("pane_ancestor", "%15", "certain"), ev("agent_source", "claude", "certain")],
              parts=[part("MCP: playwright", 1.2 * G, 3, 970002), part("claude", 0.6 * G, 1, 970000),
                     part("python3.13", 0.3 * G, 2, 970005), part("zsh", 0.1 * G, 2, 970001)],
              insights=["claude.mcp"],
              hist=history(2.0 * G, 2.6 * G, cpu=28, gap=(900, 780))),
        owner("agent:opencode:ses_7Hq2kV", "agent_session", "整理发布说明", 0.9 * G, 0.4, [971000, 971001, 971002],
              verdict("watch", "idle-agent", "空闲 3 小时，可以关掉。"),
              origin={"tmux_pane": "%7", "pane_alive": True, "tmux_target": "main:0.1", "cwd": "/Users/demo/src/notes", "ports": None,
                      "window_name": "main", "pane_title": "opencode"},
              agent=agent("opencode", "ses_7Hq2kV", "整理发布说明", "manual", "整理发布说明", "把这周的改动整理成发布说明",
                          "好的，先列出合并过的提交……", "idle", "/Users/demo/src/notes", "main", 3 * 3600 + 12 * 60),
              evidence=[ev("pane_ancestor", "%7", "certain"), ev("agent_source", "opencode", "certain")],
              parts=[part("opencode", 0.8 * G, 1, 971000), part("node", 0.1 * G, 2, 971001)],
              hist=history(0.9 * G, 0.9 * G, cpu=0.4)),
        owner("system:312", "system", "WindowServer", 1.1 * G, 9.5, [312],
              verdict("ok", "system.WindowServer", "macOS 图形合成进程"),
              evidence=[ev("other_uid", "88", "certain")],
              insights=["system.WindowServer"], hist=history(1.1 * G, 1.1 * G, cpu=9)),
        owner("service:com.apple.mds_stores", "service", "mds_stores", 0.6 * G, 1.2, [418],
              verdict("ok", "system.mds_stores", "Spotlight 索引服务"),
              evidence=[ev("service_fallback", "launchd ancestry", "weak")],
              insights=["system.mds_stores"], hist=history(0.6 * G, 0.6 * G, cpu=1)),
        owner("pane:%15", "tmux_pane", "main:1.0", 12 * M, 0.1, [969999],
              verdict("ok", "default", "正常"),
              origin={"tmux_pane": "%15", "pane_alive": True, "tmux_target": "main:1.0", "cwd": "/Users/demo/src/atlas", "ports": None,
                      "window_name": "main", "pane_title": "claude"},
              evidence=[ev("pane_ancestor", "%15", "certain")],
              parts=[part("zsh", 12 * M, 1, 969999)], hist=history(12 * M, 12 * M, cpu=0.1)),
        owner("pane:%22", "tmux_pane", "main:3.1", 0.3 * G, 0.2, [972000, 972001],
              verdict("ok", "default", "正常"),
              origin={"tmux_pane": "%22", "pane_alive": True, "tmux_target": "main:3.1", "cwd": "/Users/demo/src/atlas", "ports": None,
                      "window_name": "main", "pane_title": "zsh"},
              evidence=[ev("pane_ancestor", "%22", "certain")],
              parts=[part("cargo", 0.2 * G, 1, 972001), part("zsh", 0.1 * G, 1, 972000)],
              hist=history(0.3 * G, 0.3 * G, cpu=0.2)),
        owner("agent:codex:019a7c2e-4f1b-7d20-9c3a-5e8b1d6f0a42", "agent_session", "重构导出模块", 0.7 * G, 3.1,
              [960100, 960101, 960102],
              verdict("ok", "default", "正常"),
              origin={"tmux_pane": None, "pane_alive": None, "cwd": "/Users/demo/src/export-kit", "ports": None},
              agent=codex_agent("019a7c2e-4f1b-7d20-9c3a-5e8b1d6f0a42", "重构导出模块", "/Users/demo/src/export-kit",
                                "refactor/export", END - 300),
              evidence=[ev("agent_source", "codex", "certain"),
                        ev("codex_thread_env", "019a7c2e-4f1b-7d20-9c3a-5e8b1d6f0a42", "certain")],
              parts=[part("python3.13", 0.5 * G, 2, 960100), part("node", 0.2 * G, 1, 960102)],
              hist=history(0.6 * G, 0.7 * G, cpu=3)),
        owner("agent:claude:5d1e8a90-2c4b-4f7e-8a1d-6b3c9e0f2a17", "agent_session", "补 README 示例", 0.5 * G, 0.6,
              [973000, 973001],
              verdict("ok", "default", "正常"),
              origin={"tmux_pane": "%22", "pane_alive": True, "tmux_target": "main:3.1", "cwd": "/Users/demo/src/atlas", "ports": None,
                      "window_name": "main", "pane_title": "claude"},
              agent=agent("claude", "5d1e8a90-2c4b-4f7e-8a1d-6b3c9e0f2a17", "补 README 示例", "manual", "补 README 示例",
                          "给 README 补两个用法示例", "好，先看现有的命令列表……", "idle", "/Users/demo/src/atlas", "main", 600),
              evidence=[ev("pane_ancestor", "%22", "certain"), ev("agent_source", "claude", "certain")],
              parts=[part("claude", 0.4 * G, 1, 973000), part("node", 0.1 * G, 1, 973001)],
              hist=history(0.5 * G, 0.5 * G, cpu=0.6)),
        owner("agent:opencode:ses_Lk92wQ", "agent_session", "检查文档死链", 0.2 * G, 0.3, [974000],
              verdict("ok", "default", "正常"),
              origin={"tmux_pane": "%22", "pane_alive": True, "tmux_target": "main:3.1", "cwd": "/Users/demo/src/atlas", "ports": None,
                      "window_name": "main", "pane_title": "opencode"},
              agent=agent("opencode", "ses_Lk92wQ", "检查文档死链", "manual", "检查文档死链", "检查 docs 目录的死链",
                          "共找到 3 个失效链接……", "idle", "/Users/demo/src/atlas", "main", 900),
              evidence=[ev("pane_ancestor", "%22", "certain"), ev("agent_source", "opencode", "certain")],
              parts=[part("opencode", 0.2 * G, 1, 974000)],
              hist=history(0.2 * G, 0.2 * G, cpu=0.3)),
        owner("agent:opencode:pid:975000", "agent_session", "opencode", 0.4 * G, 0.1, [975000, 975001],
              verdict("ok", "default", "正常"),
              origin={"tmux_pane": "%301", "pane_alive": False, "cwd": "/Users/demo/src/old-api", "ports": None},
              agent={**agent("opencode", None, None, None, None, None, None, None, "/Users/demo/src/old-api", None, 0),
                     "confidence": "weak", "updated_at_ms": None, "idle_seconds": None},
              evidence=[ev("tmux_environment", "%301", "certain"), ev("agent_source", "opencode", "weak")],
              parts=[part("opencode", 0.3 * G, 1, 975000), part("node", 0.1 * G, 1, 975001)],
              hist=history(0.4 * G, 0.4 * G, cpu=0.1)),
    ]
    members = ([{"id": f"agent:claude:session-{i:02d}", "count": c} for i, c in
                enumerate([6, 5, 5, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 2, 2], 1)]
               + [{"id": f"agent:opencode:ses_{i}", "count": c} for i, c in enumerate([3, 3, 2, 2], 1)]
               + [{"id": "app:com.anthropic.claude-code", "count": 8},
                  {"id": "orphan:980001", "count": 1}, {"id": "orphan:980002", "count": 1}])
    count = sum(m["count"] for m in members)
    swarm = {"signature": "exe:python3.13", "label": "python3.13", "count": count, "mem_bytes": count * 40 * M,
             "unknown_mem_count": 0, "cpu_pct": 3.2, "shape": "spread", "owners": members,
             "parents": [970000, 971000], "started_span_s": 5400.0,
             "pids": list(range(990000, 990000 + count)),
             "evidence": [ev("same_executable", "可执行文件相同：python3.13", "certain")],
             "verdict": verdict("ok", "swarm-spread", "分散在多个归属里，各自 1–6 份。")}
    taken = "2026-10-21T00:20:00+08:00"
    host = {"mem_total_bytes": 48 * G, "mem_used_bytes": 44_237_000_000, "compressed_bytes": 9 * G,
            "wired_bytes": 5 * G, "swap_used_bytes": 6 * G, "pressure": "warn",
            "kernel_zones": {"status": "fresh", "as_of": taken, "basis": "zprint",
                             "top": [{"name": "VM.object", "bytes": 610 * M, "collectible": None, "pageable": False},
                                     {"name": "kalloc.type.var4.256", "bytes": 420 * M, "collectible": True, "pageable": False},
                                     {"name": "vnode", "bytes": 310 * M, "collectible": None, "pageable": False},
                                     {"name": "kalloc.data.4096", "bytes": 205 * M, "collectible": True, "pageable": False},
                                     {"name": "ipc.ports", "bytes": 96 * M, "collectible": None, "pageable": False}],
                             "reason": None},
            "gpu_memory": {"status": "fresh", "as_of": taken, "bytes": int(2.1 * G), "source": "IOAccelerator", "reason": None},
            "memory_diagnostics": [verdict("watch", "swap-high", "swap 已用 6.0G。内存偏紧时，先关闲置会话和孤儿进程收益最大。")],
            "cpu": {"status": "fresh", "usage_pct": 37.5, "load_1m": 6.2, "logical_cores": 12, "reason": None}}
    return {"schema": "whoate/v1", "taken_at": taken, "host": host,
            "system_procs": {"status": "fresh", "as_of": taken}, "owners": owners, "swarms": [swarm], "issues": [],
            "procs": procs()}


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "design-report.json")
    with open(out, "w") as f:
        json.dump(report(), f, ensure_ascii=False, indent=1)
        f.write("\n")
    print(out)
