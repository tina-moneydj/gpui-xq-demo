#!/usr/bin/env python3
"""網頁版壓測摘要：results.jsonl（每輪一行）→ summary.json（各 app×情境 3 輪中位數）＋印出 Markdown 表。
用法：summarize.py <outDir>"""
import json, statistics, sys

out = sys.argv[1]
rows = [json.loads(l) for l in open(f"{out}/results.jsonl") if l.strip()]
# 同一 app×情境×輪 重跑過（例如遇到 VM 暫停）時，只留最後一次
last = {}
for r in rows: last[(r["app"], r["mode"], r["run"])] = r
rows = list(last.values())
ok = [r for r in rows if r.get("ok")]
med = lambda v: (round(statistics.median(v), 1) if v else None)

def g(r, *ks):
    for k in ks:
        if r is None: return None
        r = r.get(k) if isinstance(r, dict) else None
    return r

S = {"runs": {}, "failed": [{"app": r["app"], "mode": r["mode"], "run": r["run"], "error": r.get("error")} for r in rows if not r.get("ok")]}
for app in sorted({r["app"] for r in ok}):
    A = S.setdefault(app, {})
    def pick(mode): return [r for r in ok if r["app"] == app and r["mode"] == mode]
    m = pick("main")
    if m:
        A["main"] = {"n": len(m),
            "first_frame_s": med([g(r, "load", "first_frame_s") for r in m if g(r, "load", "first_frame_s")]),
            "first_quote_s": med([g(r, "load", "first_quote_s") for r in m if g(r, "load", "first_quote_s")]),
            "rss10_mb": med([g(r, "mem10", "rss_mb") for r in m]), "pss10_mb": med([g(r, "mem10", "pss_mb") for r in m]),
            "rss60_mb": med([g(r, "mem60", "rss_mb") for r in m]), "pss60_mb": med([g(r, "mem60", "pss_mb") for r in m]),
            "renderer_pss60_mb": med([g(r, "mem60", "by", "renderer", "pss_mb") or 0 for r in m]),
            "gpu_pss60_mb": med([g(r, "mem60", "by", "gpu-process", "pss_mb") or 0 for r in m]),
            "js_heap60_mb": med([g(r, "js60", "used_mb") for r in m]), "wasm60_mb": med([r.get("wasm60_mb") for r in m if r.get("wasm60_mb")]),
            "cpu_pct": med([g(r, "cpu", "cpu_pct") for r in m]), "cpu_nollvm_pct": med([g(r, "cpu", "cpu_nollvm_pct") for r in m]),
            "fps": med([r["fps"] for r in m if r.get("fps") is not None]),
            "bytes_transferred": med([g(r, "load", "bytes_transferred") for r in m]),
            "loadavg": [r["loadavg"].split()[0] for r in m]}
    s = pick("stress")
    if s:
        A["stress"] = {"n": len(s),
            "cpu_tick_pct": med([g(r, "cpu_tick", "cpu_pct") for r in s]), "cpu_tick_nollvm_pct": med([g(r, "cpu_tick", "cpu_nollvm_pct") for r in s]),
            "cpu_scroll_pct": med([g(r, "cpu_scroll", "cpu_pct") for r in s]), "cpu_scroll_nollvm_pct": med([g(r, "cpu_scroll", "cpu_nollvm_pct") for r in s]),
            "rss_tick_mb": med([g(r, "mem_tick", "rss_mb") for r in s]), "pss_tick_mb": med([g(r, "mem_tick", "pss_mb") for r in s]),
            "pss_scroll_mb": med([g(r, "mem_scroll", "pss_mb") for r in s]), "wasm_tick_mb": med([r.get("wasm_tick_mb") for r in s if r.get("wasm_tick_mb")]),
            "tick_fps": med([g(r, "groupperf", "tick-end", "fps") for r in s if g(r, "groupperf", "tick-end", "fps") is not None]),
            "scroll_fps": med([g(r, "groupperf", "done", "fps") for r in s if g(r, "groupperf", "done", "fps") is not None]),
            "scroll_jank": med([g(r, "groupperf", "done", "scrollJankCount") for r in s if g(r, "groupperf", "done", "scrollJankCount") is not None]),
            "tick_paint_ms": med([g(r, "groupperf", "tick-end", "avgPaintMs") for r in s if g(r, "groupperf", "tick-end", "avgPaintMs") is not None]),
            "scroll_paint_ms": med([g(r, "groupperf", "done", "avgPaintMs") for r in s if g(r, "groupperf", "done", "avgPaintMs") is not None]),
            "loadavg": [r["loadavg"].split()[0] for r in s]}
    h = pick("heavy")
    if h:
        A["heavy"] = {"n": len(h), "heavy_frame_s": med([g(r, "load", "heavy_frame_s") for r in h if g(r, "load", "heavy_frame_s")]),
                      "loadavg": [r["loadavg"].split()[0] for r in h]}
        for ph in ("ticks", "crosshair", "pan"):
            A["heavy"][ph] = {"fps": med([g(r, "phases", ph, "perf", "fps") for r in h if g(r, "phases", ph, "perf", "fps") is not None]),
                              "paint_ms": med([g(r, "phases", ph, "perf", "avgPaintMs") for r in h if g(r, "phases", ph, "perf", "avgPaintMs") is not None]),
                              "cpu_pct": med([g(r, "phases", ph, "cpu_pct") for r in h]), "cpu_nollvm_pct": med([g(r, "phases", ph, "cpu_nollvm_pct") for r in h]),
                              "rss_mb": med([g(r, "phases", ph, "mem", "rss_mb") for r in h]), "pss_mb": med([g(r, "phases", ph, "mem", "pss_mb") for r in h]),
                              "wasm_mb": med([g(r, "phases", ph, "wasm_mb") for r in h if g(r, "phases", ph, "wasm_mb")])}
    for sc in ("s1k", "s10k", "s50k", "s100k", "burst"):
        t = pick(f"ticks:{sc}")
        if not t: continue
        A[f"ticks_{sc}"] = {"n": len(t),
            "lat_p50": med([g(r, "ticks", "latOldest", "p50") for r in t]), "lat_p99": med([g(r, "ticks", "latOldest", "p99") for r in t]),
            "lat_max": med([g(r, "ticks", "latOldest", "max") for r in t]),
            "transport_p50": med([g(r, "ticks", "breakdown_p50", "transport") for r in t]),
            "recv_to_apply_p50": med([g(r, "ticks", "breakdown_p50", "recvToApply") for r in t]),
            "apply_to_present_p50": med([g(r, "ticks", "breakdown_p50", "applyToPresent") for r in t]),
            "first5_p50": med([g(r, "ticks", "trend", "first5_p50") for r in t]), "last5_p50": med([g(r, "ticks", "trend", "last5_p50") for r in t]),
            "loss": [g(r, "ticks", "loss") for r in t], "sent": med([g(r, "ticks", "sent") for r in t]),
            "fps": med([g(r, "ticks", "fps") for r in t if g(r, "ticks", "fps") is not None]),
            "cpu_pct": med([g(r, "cpu", "cpu_pct") for r in t]), "cpu_nollvm_pct": med([g(r, "cpu", "cpu_nollvm_pct") for r in t]),
            "rss_end_mb": med([g(r, "mem_end", "rss_mb") for r in t]), "pss_end_mb": med([g(r, "mem_end", "pss_mb") for r in t]),
            "rss_growth_mb": med([g(r, "mem_end", "rss_mb") - g(r, "mem_start", "rss_mb") for r in t]),
            "wasm_end_mb": med([r.get("wasm_end_mb") for r in t if r.get("wasm_end_mb")]),
            "loadavg": [r["loadavg"].split()[0] for r in t]}
json.dump(S, open(f"{out}/summary.json", "w"), ensure_ascii=False, indent=1)
print(json.dumps(S, ensure_ascii=False, indent=1))
