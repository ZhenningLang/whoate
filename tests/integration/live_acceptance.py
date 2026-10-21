#!/usr/bin/env python3
"""Phase 06 real-machine acceptance, read-only (U-ACC-8 / U-ACC-9 on this Mac).

  live_acceptance.py chrome <whoate> <evidence dir>   QA16: a Chrome renderer's owner has the tab-process part
  live_acceptance.py top <whoate> <evidence dir>      QA17: `whoate top` shows a chromium and a coding-agents insight
  live_acceptance.py selftest                         QA17 reporting negative control (no machine reads)

QA17 counts natural hits only. Coding-agent entries need a Claude session over
4 GiB, an opencode session over 2 GiB or a recognised MCP child; with none, QA17
fails as an unmet precondition. It then runs a controlled diagnostic (an idle
`node <tmp>/whoate-probe-mcp.js` child of this script), reported as
"受控、不计入通过", which never turns QA17 green.

Reads the owner's real processes and session signals, as `whoate` itself does;
the cache goes to a temp dir (WHOATE_CACHE_DIR) and nothing else is written
outside <evidence dir>. A missing precondition (Chrome not running) is a
failure with the reason, never a skip. Evidence: the JSON, the text output and
a summary with owner ids, entry ids, trigger inputs and the values the same
JSON exposes ("JSON 未暴露" when it does not).
"""
import glob, json, os, shutil, subprocess, sys, tempfile, time

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    sys.exit("python 3.11+ needed for tomllib")

TAB_PART = {"标签页进程", "Tab processes"}
# Count signals that the JSON shows as a part (by name); other counts are matching-only.
SIGNAL_PARTS = {"chrome_extensions": {"Extensions", "扩展"}, "brave_extensions": {"Extensions", "扩展"}}


def whoate(binary, cache, *args):
    env = dict(os.environ, WHOATE_CACHE_DIR=cache)
    env.pop("WHOATE_SESSION_PID", None)
    return subprocess.run([binary, *args], capture_output=True, text=True, env=env, timeout=120)


def write(out, name, text):
    with open(os.path.join(out, name), "w") as f:
        f.write(text)


def chrome(binary, out):
    # By bundle path and --type: Chrome 155 renamed the helper to "Google Chrome Helper (Aperitif Renderer)".
    pgrep = subprocess.run(["pgrep", "-f", r"/Google Chrome\.app/.* --type=renderer( |$)"], capture_output=True, text=True)
    # Tab renderers only: extension renderers carry --extension-process and belong to the Extensions part.
    pids = []
    if pgrep.stdout.split():
        ps = subprocess.run(["ps", "-o", "pid=,args=", "-p", ",".join(pgrep.stdout.split())], capture_output=True,
                            text=True).stdout
        pids = [int(line.split()[0]) for line in ps.splitlines() if "--extension-process" not in line]
    if not pids:
        print("FAIL QA16 前提不满足：请打开 Google Chrome（没有 --type=renderer 进程）")
        return 1
    with tempfile.TemporaryDirectory() as cache:
        # Prefer a renderer of the owner's own Chrome over one started by a test
        # browser in a tmux pane (agent-browser), which whoate files under that pane.
        listing = json.loads(whoate(binary, cache, "top", "--json", "-n", "1000").stdout)
        app = next((o for o in listing.get("owners", []) if o["id"] == "app:com.google.Chrome"), None)
        pid = next((p for p in pids if app and p in app.get("pids", [])), pids[0])
        proc = whoate(binary, cache, "why", str(pid), "--json")
    write(out, "qa16-why.json", proc.stdout)
    report = json.loads(proc.stdout)
    owners = [o for o in report.get("owners", []) if pid in o.get("pids", [])]
    if not owners:
        print(f"FAIL QA16 no owner contains renderer pid {pid} (exit {proc.returncode})")
        return 1
    owner = owners[0]
    parts = [p for p in owner.get("parts", []) if p["name"] in TAB_PART]
    summary = {"renderer_pids_running": len(pids), "pid": pid, "owner": owner["id"], "label": owner.get("label"),
               "parts": [(p["name"], p["process_count"]) for p in owner.get("parts", [])],
               "tab_part": parts[0] if parts else None, "pid_in_tab_part": bool(parts) and pid in parts[0]["pids"]}
    write(out, "qa16-summary.json", json.dumps(summary, ensure_ascii=False, indent=2))
    if not summary["pid_in_tab_part"]:
        print(f"FAIL QA16 renderer {pid} is not in a tab-process part of {owner['id']}: {summary['parts']}")
        return 1
    print(f"ok QA16 pid {pid} -> {owner['id']} part {parts[0]['name']} process_count={parts[0]['process_count']} "
          f"pid_in_part={summary['pid_in_tab_part']}")
    return 0


def catalog():
    entries = {}
    for path in sorted(glob.glob("crates/whoate-core/src/insights/*.toml")):
        for entry in tomllib.load(open(path, "rb"))["entries"]:
            entries[entry["id"]] = entry
    return entries


def leaves(trigger):
    if trigger["kind"] == "all":
        for t in trigger["triggers"]:
            yield from leaves(t)
    else:
        yield trigger


UNEXPOSED = "JSON 未暴露"


def trigger_inputs(entry, owner):
    """Each trigger leaf with the value the report exposes for it. A raw signal
    the JSON does not carry stays UNEXPOSED even when a proxy field is recorded."""
    facts = {f["key"]: f["value"] for f in owner.get("facts", [])}
    rows = []
    for leaf in leaves(entry["trigger"]):
        if leaf["kind"] == "memory":
            rows.append({"input": f"memory >= {leaf['min_bytes']}", "json": "owner.mem_bytes", "value": owner.get("mem_bytes")})
            continue
        if leaf["kind"] == "config":
            key = leaf["key"]
            rows.append({"input": f"config {key} == {leaf.get('equals')}", "json": f"facts[{key}]" if key in facts else None,
                         "value": facts.get(key, UNEXPOSED)})
            continue
        signal = leaf["signal"]
        row = {"input": f"count {signal} >= {leaf['min']}", "json": None, "value": UNEXPOSED}
        names = SIGNAL_PARTS.get(signal, set())
        part = next((p for p in owner.get("parts", []) if p["name"] in names), None)
        mcp = [p for p in owner.get("parts", []) if p["name"].startswith("MCP: ")]
        if signal in facts:
            row.update(json=f"facts[{signal}]", value=facts[signal])
        elif part:
            row.update(json=f"parts[{part['name']}].process_count", value=part["process_count"])
        elif signal == "mcp_count" and mcp:
            row.update(proxy="parts[MCP: *].process_count", proxy_value=sum(p["process_count"] for p in mcp))
        elif signal.startswith("software_"):
            row.update(proxy="owner.id", proxy_value=owner["id"])
        rows.append(row)
    return rows


def describe(row):
    proxy = f" (proxy {row['proxy']}={row['proxy_value']})" if "proxy" in row else ""
    return f"{row['input']} -> {row['json'] or '-'}={row['value']}{proxy}"


def top_hits(binary, out, tag, entries):
    with tempfile.TemporaryDirectory() as cache:
        proc = whoate(binary, cache, "top", "--json", "-n", "100")
        text = whoate(binary, cache, "top", "-n", "100")
    write(out, f"qa17-top-{tag}.json", proc.stdout)
    write(out, f"qa17-top-{tag}.txt", text.stdout)
    report = json.loads(proc.stdout)
    hits = []
    for owner in report.get("owners", []):
        for entry_id in owner.get("insights", []):
            entry = entries.get(entry_id)
            if entry is None:
                continue
            hits.append({"owner": owner["id"], "label": owner.get("label"), "entry": entry_id,
                         "recognizer": entry["recognizer"], "in_text": entry_id in text.stdout
                         and (entry["cause"]["en"] in text.stdout or entry["cause"]["zh"] in text.stdout),
                         "trigger": entry["trigger"], "inputs": trigger_inputs(entry, owner),
                         "pids": owner.get("pids", [])})
    return proc, text, hits


def shown(hits, recognizer):
    return [h for h in hits if h["recognizer"] == recognizer and h["in_text"]]


def controlled_probe(binary, out, entries):
    """Diagnostic only, never counted toward QA17 (orchestrator ruling 2026-10-21 23:0x):
    an idle node script named like an MCP server, child of this script, shows
    whether collection -> session attribution -> catalog -> text output works."""
    if not shutil.which("node"):
        return {"status": "node not found"}
    probe_dir = tempfile.mkdtemp()
    script = os.path.join(probe_dir, "whoate-probe-mcp.js")
    with open(script, "w") as f:
        f.write("setTimeout(() => {}, 120000);\n")
    probe = subprocess.Popen(["node", script])
    try:
        time.sleep(1)
        _, _, hits = top_hits(binary, out, "controlled", entries)
    finally:
        probe.terminate()
        probe.wait()
        shutil.rmtree(probe_dir)
    own = [h for h in hits if probe.pid in h["pids"] and h["entry"].endswith(".mcp") and h["in_text"]]
    return {"status": "hit" if own else "no hit", "probe_pid": probe.pid,
            "hits": [{k: h[k] for k in ("owner", "entry", "in_text")} for h in own]}


def top(binary, out):
    entries = catalog()
    proc, text, hits = top_hits(binary, out, "natural", entries)
    for h in hits:
        h.pop("pids")
    unexposed = sorted({(h["entry"], describe(i)) for h in hits for i in h["inputs"] if i["value"] == UNEXPOSED})
    summary = {"exit_json": proc.returncode, "exit_text": text.returncode, "hits": hits,
               "unexposed_inputs": [f"{e}: {d}" for e, d in unexposed]}
    rc = 0
    for recognizer in ("chromium", "coding-agents"):
        found = shown(hits, recognizer)
        if found:
            h = found[0]
            print(f"ok QA17 {recognizer}: {h['owner']} {h['entry']} [{'; '.join(map(describe, h['inputs']))}] "
                  f"({len(found)} hit(s))")
        else:
            other = [h["entry"] for h in hits if h["recognizer"] == recognizer]
            print(f"FAIL QA17 前提不满足：本机进程没有自然命中的 {recognizer} 条目 (json hits: {other})")
            rc = 1
    if not shown(hits, "coding-agents"):
        summary["controlled_diagnostic"] = controlled_probe(binary, out, entries)
        print(f"DIAG QA17 受控、不计入通过: {json.dumps(summary['controlled_diagnostic'], ensure_ascii=False)}")
    write(out, "qa17-summary.json", json.dumps(summary, ensure_ascii=False, indent=2))
    for line in summary["unexposed_inputs"]:
        print(f"REPORT QA17 trigger input not in JSON: {line}")
    return rc


def selftest():
    """Negative control for the reporting: claude.mcp on an owner with an MCP part but no
    software_claude fact must report both count inputs as unexposed, proxies kept aside."""
    entry = catalog()["claude.mcp"]
    owner = {"id": "agent:claude:pid:123", "facts": [], "mem_bytes": 1,
             "parts": [{"name": "MCP: example-mcp", "process_count": 1, "pids": [124]}]}
    rows = trigger_inputs(entry, owner)
    unexposed = [describe(r) for r in rows if r["value"] == UNEXPOSED]
    print("\n".join(unexposed))
    ok = len(rows) == 2 and len(unexposed) == 2 and all("proxy" in r for r in rows)
    print(("ok" if ok else "FAIL") + " QA17 reporting: proxied trigger inputs stay in unexposed_inputs")
    return 0 if ok else 1


if __name__ == "__main__":
    if sys.argv[1:] == ["selftest"]:
        sys.exit(selftest())
    if len(sys.argv) != 4 or sys.argv[1] not in ("chrome", "top"):
        sys.exit(__doc__)
    os.makedirs(sys.argv[3], exist_ok=True)
    sys.exit((chrome if sys.argv[1] == "chrome" else top)(sys.argv[2], sys.argv[3]))
