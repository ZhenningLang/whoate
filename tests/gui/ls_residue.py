#!/usr/bin/env python3
"""LaunchServices residue check (Phase 08 incident, QA15). Read-only.

A whoate entry left `exited-with-subordinates`, or one whose coalition or
relatedCoalitionPIDs contains a tmux server, is what turned a Dock force quit
into the loss of every tmux pane. This script never quits, signals or edits
any entry; it only reads `lsappinfo list`.

  ls_residue.py snapshot <file>                 tmux server pids + existing ASNs
  ls_residue.py check --baseline <file> [--exempt <file>] [--wait 15]

Only entries that are new since the snapshot fail the check. Entries already
present are printed as PREEXISTING_LS_RESIDUE; exempt pids (the negative
control's own process) as EXEMPT_LS_RESIDUE. Neither fails.
"""
import argparse, json, os, re, subprocess, sys, time

NAME = re.compile(r"whoate", re.IGNORECASE)
HEADER = re.compile(r'^\s*(\d+)\) "(.*)" (ASN:[0-9a-fx-]+):', re.IGNORECASE)


def _numbers(text):
    return [int(n) for n in re.findall(r"\d+", text or "")]


def parse(text):
    """Entries of `lsappinfo list`: name, asn, paths, pid, type, flags, coalition data, raw text."""
    entries, current = [], None
    for line in text.splitlines():
        head = HEADER.match(line)
        if head:
            current = {"index": int(head.group(1)), "name": head.group(2), "asn": head.group(3), "raw": [line],
                       "bundle_path": None, "exe_path": None, "pid": None, "type": None, "flags": [],
                       "coalition": None, "members": [], "related_ids": [], "related_pids": []}
            entries.append(current)
            continue
        if current is None:
            continue
        current["raw"].append(line)
        stripped = line.strip()
        if stripped.startswith("bundle path="):
            current["bundle_path"] = stripped.partition("=")[2].strip('"')
        elif stripped.startswith("executable path="):
            current["exe_path"] = stripped.partition("=")[2].strip('"')
        elif stripped.startswith("pid = "):
            current["pid"] = int(stripped.split()[2])
            kind = re.search(r'type="([^"]*)"', stripped)
            current["type"] = kind.group(1) if kind else None
            # Flags such as (exited-with-subordinates); the token's own parens are not flags.
            tail = stripped.split("]", 1)[-1]
            current["flags"] = re.findall(r"\(([a-z][a-z-]*)\)", tail)
        elif stripped.startswith("coalition:"):
            m = re.match(r"coalition:\s*(\d+)\s*(\{[^}]*\})?", stripped)
            current["coalition"] = int(m.group(1)) if m else None
            current["members"] = _numbers(m.group(2)) if m and m.group(2) else []
            ids = re.search(r"relatedCoalitionIDs:\s*\(([^)]*)\)", stripped)
            pids = re.search(r"relatedCoalitionPIDs:\s*\(([^)]*)\)", stripped)
            current["related_ids"] = _numbers(ids.group(1)) if ids else []
            current["related_pids"] = _numbers(pids.group(1)) if pids else []
    for entry in entries:
        entry["raw"] = "\n".join(entry["raw"])
    return entries


def entry_for(pid, entries=None):
    """The entry of a pid, from `entries` or a fresh `lsappinfo list`."""
    entries = parse(lsappinfo()) if entries is None else entries
    return next((e for e in entries if e["pid"] == pid), None)


def is_whoate(entry):
    return any(NAME.search(v or "") for v in (entry["name"], entry["bundle_path"], entry["exe_path"]))


def problems(entry, tmux_pids):
    """Why this entry is dangerous to force quit; empty when it is clean."""
    found = []
    if "exited-with-subordinates" in entry["flags"]:
        found.append("exited-with-subordinates")
    tmux = set(tmux_pids)
    if tmux & set(entry["members"]):
        found.append(f"coalition_contains_tmux={sorted(tmux & set(entry['members']))}")
    if tmux & set(entry["related_pids"]):
        found.append(f"related_contains_tmux={sorted(tmux & set(entry['related_pids']))}")
    return found


def tmux_servers(environ=os.environ, table=None):
    """$TMUX's server pid plus every ppid-1 process named tmux."""
    pids = set()
    parts = environ.get("TMUX", "").split(",")
    if len(parts) >= 2 and parts[1].isdigit():
        pids.add(int(parts[1]))
    if table is None:
        table = subprocess.run(["/bin/ps", "-axo", "pid=,ppid=,comm="], capture_output=True, text=True,
                               check=True).stdout
    for row in table.splitlines():
        fields = row.split(None, 2)
        if len(fields) == 3 and fields[1] == "1" and os.path.basename(fields[2]) == "tmux":
            pids.add(int(fields[0]))
    return sorted(pids)


def lsappinfo():
    return subprocess.run(["/usr/bin/lsappinfo", "list"], capture_output=True, text=True, check=True,
                          timeout=30).stdout


def classify(entries, baseline_asns, tmux_pids, exempt):
    """(failures, preexisting, exempted): lists of (entry, problems) for whoate entries."""
    failures, preexisting, exempted = [], [], []
    for entry in filter(is_whoate, entries):
        issues = problems(entry, tmux_pids)
        if entry["asn"] in baseline_asns:
            preexisting.append((entry, issues))
        elif entry["pid"] in exempt:
            exempted.append((entry, issues))
        elif issues:
            failures.append((entry, issues))
    return failures, preexisting, exempted


def check(baseline, exempt=(), wait_s=15, read=lsappinfo, sleep=time.sleep, out=print):
    """Waits up to wait_s for new residue to clear, then reports. Returns the failure count."""
    asns, tmux = set(baseline["asns"]), baseline["tmux_pids"]
    for attempt in range(int(wait_s) + 1):
        failures, preexisting, exempted = classify(parse(read()), asns, tmux, set(exempt))
        if not failures or attempt == int(wait_s):
            break
        sleep(1)
    for tag, rows in (("PREEXISTING_LS_RESIDUE", preexisting), ("EXEMPT_LS_RESIDUE", exempted)):
        for entry, issues in rows:
            out(f"{tag} pid={entry['pid']} name={entry['name']} issues={issues or ['none']}")
    for entry, issues in exempted:
        out(f"EXEMPT_NOTE pid={entry['pid']}: left by the negative control on purpose. "
            "Do not force quit it in the Dock or Activity Monitor.")
    for entry, issues in failures:
        out(f"LS_RESIDUE_FAIL pid={entry['pid']} issues={issues}\n{entry['raw']}")
    out(f"LS_RESIDUE_CHECK tmux_pids={tmux} new_failures={len(failures)} "
        f"preexisting={len(preexisting)} exempt={len(exempted)}")
    return len(failures)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="command", required=True)
    snap = sub.add_parser("snapshot")
    snap.add_argument("file")
    chk = sub.add_parser("check")
    chk.add_argument("--baseline", required=True)
    chk.add_argument("--exempt")
    chk.add_argument("--wait", type=float, default=15)
    a = ap.parse_args(argv)
    if a.command == "snapshot":
        entries = parse(lsappinfo())
        data = {"tmux_pids": tmux_servers(), "asns": [e["asn"] for e in entries],
                "taken_at": time.time()}
        with open(a.file, "x") as f:
            json.dump(data, f)
        print(f"LS_SNAPSHOT entries={len(entries)} tmux_pids={data['tmux_pids']} "
              f"whoate_entries={sum(map(is_whoate, entries))}")
        return 0
    baseline = json.load(open(a.baseline))
    exempt = []
    if a.exempt and os.path.exists(a.exempt):
        exempt = [int(x) for x in open(a.exempt).read().split()]
    return 1 if check(baseline, exempt, a.wait) else 0


if __name__ == "__main__":
    sys.exit(main())
