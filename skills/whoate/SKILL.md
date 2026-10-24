---
name: whoate
description: Attribute macOS memory and CPU to applications, tmux panes and coding sessions; inspect before proposing termination.
---

# whoate

Use the local `whoate` binary. It reads local process and session evidence.
It does not call a model, change software settings, or diagnose by process name alone.

## Install this skill

Copy this directory (including this SKILL.md) into the appropriate project skill directory:

- Claude Code: `.claude/skills/whoate/SKILL.md`
- Kilo: `.kilocode/skills/whoate/SKILL.md`
- OpenCode: `.opencode/skills/whoate/SKILL.md`

The CLI does not install skills or edit any of these applications' configuration.
`whoate agent-guide` prints this exact file, with no sampling or session reads.

## Inspect

```sh
whoate top --json
whoate top --by cpu -n 20 --json
whoate why 12345 --json
whoate why 'agent:claude:session-id' --json
whoate orphans --json
whoate agents --json
whoate pane '%12' --json
whoate pane 'work:0.1' --json
whoate swarms --json
whoate watch --interval 2s --json-lines
whoate insights --list
```

JSON uses `schema: "whoate/v1"`. Read the versioned Schema in `schema/whoate-v1.schema.json`.
Memory is in **bytes**, not MB; CPU is percent of one core and may exceed 100.
Null means unknown, never zero. Empty arrays mean no recorded items.
`unknown_mem_count` counts missing footprints: `mem_bytes` is only the known sum.
An owner's CPU is null if any member lacks a valid CPU difference.
Then optional `cpu_pct_readable` is the sum over the members that were read (a lower
bound, absent when none was) and `unknown_cpu_count` how many were not; both keys are
absent when every member was read. Swarms and history points carry the same pair.
`system_procs.status` is fresh, stale or missing; `as_of` is the source timestamp.
`taken_at` is RFC3339 with a timezone. Single queries have empty history;
watch samples once per second and emits at the requested interval (minimum 1s).
History has real gaps, not interpolation; tmux failures can change conservative identities.
Docker container facts are **independent Linux statistics**, not macOS PID parts;
never add them to the VM footprint.

Views preserve the full Report envelope and host totals. `top -n` limits owners exactly.
`swarm.owners` contains complete id/count distributions, including IDs outside the view.
Use `whoate why <id> --json` to inspect an outside owner. Do not sum owners and swarms:
they are overlapping views of the same processes.

The default top sort is memory, descending; unknown CPU sorts after known values.
`--by` and `-n` apply only to top. JSON is available on the single-query commands;
watch uses `--json-lines`. Kill also accepts `--json` for its separate result envelope;
other query flags remain unsupported by kill, insights and agent-guide.
An act diagnosis anywhere in the selected population can produce exit 1 even if
`-n` hides that row. Only top/orphans/agents use this diagnostic exit status.

`--fresh` does not read the slow system-memory disk cache. It does not wait for top;
the report marks system data missing and records `fresh_requested`.
Other-account CPU remains null. It does not delete or replace the normal cache.

## Privacy and suggestions

By default, `agent.first_prompt` and `agent.first_reply` are explicitly null in
JSON, text and watch. Do not ask for or reveal conversation contents without the
person's explicit permission. Only then add `--prompts`; it authorizes those two
fields, not arbitrary command arguments, tokens or URLs. The prompt is at most
200 characters; reply at most 120 after wrapper removal. Name/title remain
locating metadata. `--redact` also hides private paths and session name/title;
it conflicts with `--prompts`. Signatures are projected without raw argument values.

For explicitly authorized single-session validation, set `WHOATE_SESSION_PID` to
one positive Claude PID before starting whoate. This only narrows reads: all other
Claude/OpenCode/Kilo session files stay unopened; the selected transcript stops at
the first user record and first assistant record, even if that assistant has no
text. No tail or read-ahead is used. Title comes only from session metadata (null
if absent), not later transcript records. Invalid values fail closed. Unset keeps
normal product behavior. This does not grant consent, filter process sampling,
or expand `--prompts` authorization.

Use confidence/evidence, `parts`, `facts`, `insights` and `verdict`, not guesses.
For already attached insight IDs, display the catalog's cause/advice/source/checked.
Do not re-evaluate triggers using public facts: temporary matching counts are absent.
Advice is read-only. Never automatically change application settings.

## System services, kernel memory and physical reclaim

System diagnoses use explicit product heuristics, **not Apple normal baselines**.
Read each system insight's typed provenance, thresholds and safety metadata.
fseventsd and Spotlight may offer an administrator restart command; it is display-only.
launchd may restart these services. File watchers may fail to reconnect, and Spotlight
may reindex. Never execute a displayed command without separate informed authorization.
WindowServer has no restart command: signing out interrupts the graphical session.
kernel_task is never diagnosed for termination. An act diagnosis does not relax kill protection.

`host.wired_bytes` remains the host page-count measurement. `host.kernel_zones` is
an independently cached top five by **element size × current in-use count** from zprint.
It is not lifetime allocations, wasted space, or extra memory to add to wired/host totals.
Check status/as_of/reason; an initial query can be missing while watch refreshes the cache.
Only a zone explicitly reported non-collectible and non-pageable gets the limited
reboot-only warning. Unknown/collectible zones get no such claim. Absolute thresholds
do not establish continuous growth. GPU is missing until units and overlapping driver
counters have a proven contract; missing is never zero. `--fresh` bypasses these caches too.

`owner.reclaimable_physical.bytes` is a coarse unique-PID **RSS sum**, not footprint.
Shared pages may overlap; this is not guaranteed exclusive physical memory released.
Any unknown member makes bytes null; inspect unknown_count/reason. Never substitute
mem_bytes for a missing estimate. Kill previews report the same resident estimate.
Kill JSON adds a memory object with before/after measurements and signed
physical_memory_change_bytes = before host used − after host used. Negative changes
are retained. This observation is **non-causal**: unrelated software can change memory.
Target resident change is reported separately. Dry-run has no after measurement.

`agent.last_activity_at` comes from message timestamps, not process age, file mtime
or title/session updates. `idle_seconds` uses the same snapshot time. Unknown, future,
stale or restricted activity cannot justify closing a session. The new activity path
uses bounded timestamp metadata only; it does not decode or retain message contents.
Existing prompt extraction retains its previous consent rules. Restricted first-pair
capability never reads a tail for activity. Origin adds window_name and pane_title;
human output escapes control characters and --redact hides both titles. A live pane's
origin also carries optional `tmux_target` (`session:window.pane`, the same address a
tmux pane owner uses as its label). The GUI's `whoate_why` process list gives each
process an optional readable `name`: `comm` is the kernel's short name (Claude Code's
is its version, others stop at 15–16 bytes).

## Termination is a separate authorization

First inspect targets and preview:

```sh
whoate kill 12345 'pane:%12' --dry-run
```

List every expanded, deduplicated PID and request explicit approval. Only after
approval use `whoate kill ... --yes`. Without `--yes` the command only previews.
`--dry-run` never signals, even together with `--yes`.
System/root processes, Apple platform binaries and whoate itself are refused.
Never work around a refusal with another signal tool or elevated privileges.
The backend checks PID, UID and start time again immediately before signaling.

`--yes` approves only SIGTERM. The shared backend waits five seconds and reports
terminated/still_alive/refused/missing per PID. A TTY may then request another
explicit confirmation for SIGKILL, preserving the original identity. Non-TTY,
EOF or refusal never auto-escalates. `still_alive` is not proof of termination.

## Exit codes and failures

- 0: request processed normally, **not necessarily all processes exited**.
- 1: act diagnosis (top/orphans/agents) **or a fatal runtime error**.
  With JSON, an `error` object distinguishes a fatal error from a report.
- 2: invalid arguments.
- 3: requested target does not exist.
- 4: permission/safety refusal. For mixed termination outcomes, 4 takes precedence
  over 3, then 0. Read every per-PID result rather than treating 0 as all terminated.

Fatal JSON has `{"schema":"whoate/v1","error":{"code":1,"message":"..."}}`.
Diagnostics go to stderr; stdout is only the requested output. A broken stdout
pipe exits quietly with 0. Non-TTY and NO_COLOR output contain no ANSI or frames.
Never copy raw transcripts into logs, fixtures or issue reports.
