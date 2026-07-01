#!/usr/bin/env python3
"""ccwatch zero-install remote probe.

Piped over `ssh <host> python3 -` by ccwatchd, so nothing has to be installed
on the remote machine. Reads the remote's ~/.claude (or argv[1] /
$CLAUDE_CONFIG_DIR) and prints a Snapshot JSON matching ccwatch-core's serde
model exactly. Stdlib only; must stay compatible with python3.6+.
"""
import glob
import json
import os
import subprocess
import sys
import time
from datetime import datetime

ROOT = (
    sys.argv[1]
    if len(sys.argv) > 1
    else os.environ.get("CLAUDE_CONFIG_DIR", os.path.expanduser("~/.claude"))
)
NOW_MS = int(time.time() * 1000)
WINDOW_MS = 5 * 60 * 1000   # rate window, matches core default
IDLE_MS = 120 * 1000        # idle threshold, matches core default
MAX_TAIL = 16 * 1024 * 1024  # cap transcript read for huge histories
BUCKET_MS = 5 * 60 * 1000    # governor usage buckets
HORIZON_MS = 6 * 3600 * 1000  # how far back buckets are reported

# bucket_ts -> billable tokens, fed by every transcript scan
USAGE_BUCKETS = {}


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except Exception:
        return False


def parse_ts(ts):
    """ISO-8601 'Z' timestamp -> epoch ms, or None."""
    try:
        return int(
            datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp() * 1000
        )
    except Exception:
        return None


def proc_stat(pid):
    """(cpu_pct, rss_mb) via ps — portable across macOS and Linux."""
    try:
        out = subprocess.run(
            ["ps", "-o", "pcpu=,rss=", "-p", str(pid)],
            capture_output=True, text=True, timeout=3,
        ).stdout.split()
        return float(out[0]), int(out[1]) // 1024
    except Exception:
        return 0.0, 0


# session id -> transcript path
transcripts = {}
for p in glob.glob(os.path.join(ROOT, "projects", "*", "*.jsonl")):
    transcripts[os.path.basename(p)[:-6]] = p


def scan_transcript(path):
    """Fold assistant usage: (ledger, window_billable, last_activity, model)."""
    led = dict(
        input=0, output=0, cache_write=0, cache_read=0,
        web_search=0, web_fetch=0, messages=0,
    )
    window_billable = 0
    last_act = None
    model = None
    try:
        size = os.path.getsize(path)
        fh = open(path, "rb")
        if size > MAX_TAIL:
            fh.seek(size - MAX_TAIL)
            fh.readline()  # drop the partial line
        for raw in fh:
            try:
                d = json.loads(raw)
            except Exception:
                continue
            if d.get("type") != "assistant":
                continue
            m = d.get("message") or {}
            u = m.get("usage") or {}
            if not u:
                continue
            g = lambda k: u.get(k) or 0
            led["input"] += g("input_tokens")
            led["output"] += g("output_tokens")
            led["cache_write"] += g("cache_creation_input_tokens")
            led["cache_read"] += g("cache_read_input_tokens")
            srv = u.get("server_tool_use") or {}
            led["web_search"] += srv.get("web_search_requests") or 0
            led["web_fetch"] += srv.get("web_fetch_requests") or 0
            led["messages"] += 1
            # Skip internal markers like "<synthetic>".
            if m.get("model") and not m["model"].startswith("<"):
                model = m["model"]
            ts = parse_ts(d.get("timestamp") or "")
            if ts:
                last_act = max(last_act or 0, ts)
                billable = (
                    g("input_tokens")
                    + g("output_tokens")
                    + g("cache_creation_input_tokens")
                )
                if ts >= NOW_MS - WINDOW_MS:
                    window_billable += billable
                if ts >= NOW_MS - HORIZON_MS:
                    bucket = ts - (ts % BUCKET_MS)
                    USAGE_BUCKETS[bucket] = USAGE_BUCKETS.get(bucket, 0) + billable
        fh.close()
    except Exception:
        pass
    return led, window_billable, last_act, model


def read_tasks(sid):
    out = []
    files = glob.glob(os.path.join(ROOT, "tasks", sid, "*.json"))

    def order(p):
        stem = os.path.basename(p)[:-5]
        return int(stem) if stem.isdigit() else 1 << 60

    for tf in sorted(files, key=order):
        try:
            td = json.load(open(tf))
        except Exception:
            continue
        if not td.get("subject"):
            continue
        out.append({
            "subject": td["subject"],
            "status": td.get("status", ""),
            "blocked": bool(td.get("blockedBy")),
            "active_form": td.get("activeForm"),
        })
    return out


sessions = []
for f in sorted(glob.glob(os.path.join(ROOT, "sessions", "*.json"))):
    try:
        meta = json.load(open(f))
    except Exception:
        continue
    sid = meta.get("sessionId")
    pid = meta.get("pid")
    if not sid or not pid or not alive(pid):
        continue

    led, window_billable, last_act, model = (
        scan_transcript(transcripts[sid])
        if sid in transcripts
        else (dict(input=0, output=0, cache_write=0, cache_read=0,
                   web_search=0, web_fetch=0, messages=0), 0, None, None)
    )
    tpm = window_billable / (WINDOW_MS / 60000.0)
    last = last_act or meta.get("startedAt")
    state = "running" if (last is None or NOW_MS - last <= IDLE_MS) else "idle"
    cpu, rss = proc_stat(pid)

    sessions.append({
        "id": sid,
        "name": meta.get("name") or sid,
        "cwd": meta.get("cwd", ""),
        "pid": pid,
        "kind": meta.get("kind") or "interactive",
        "entrypoint": meta.get("entrypoint", ""),
        "version": meta.get("version", ""),
        "model": model,
        "state": state,
        "started_at": meta.get("startedAt"),
        "last_activity": last,
        "tokens": led,
        "tokens_per_min": tpm,
        "cpu_pct": cpu,
        "rss_mb": rss,
        "agents": [],
        "tasks": read_tasks(sid),
        "watchers": [],
        "host": {"kind": "local"},
        "remote_name": None,
    })

agg = dict(input=0, cache_write=0, cache_read=0)
total_tokens = 0
for s in sessions:
    t = s["tokens"]
    total_tokens += t["input"] + t["output"] + t["cache_write"] + t["cache_read"]
    for k in agg:
        agg[k] += t[k]
denom = agg["input"] + agg["cache_write"] + agg["cache_read"]
print(json.dumps({
    "generated_at": NOW_MS,
    "sessions": sessions,
    "alerts": [],
    "usage_buckets": sorted(USAGE_BUCKETS.items()),
    "totals": {
        "active_sessions": len(sessions),
        "tokens_per_min": sum(s["tokens_per_min"] for s in sessions),
        "total_tokens": total_tokens,
        "cache_hit_pct": (agg["cache_read"] / denom * 100.0) if denom else 0.0,
    },
}))
