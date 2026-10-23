#!/usr/bin/env python3
"""彙總交錯 3 輪（ilv/r<輪>-<before|after|egui>-<main|multi|ticks>/）→ 中位數表（summary.json＋印 markdown）。"""
import glob, json, os, statistics, sys
B = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "ilv")
def med(v):
    v = [x for x in v if x is not None]
    return round(statistics.median(v), 1) if v else None
def runs(ver, kind, f):
    out = []
    for d in sorted(glob.glob(f"{B}/r*-{ver}-{kind}")):
        p = os.path.join(d, f)
        if os.path.exists(p): out.append(json.load(open(p)))
    return out
S = {}
for v in ("before", "after", "egui"):
    m = runs(v, "main", "results.json")
    bl = [r for x in m for r in x.get("baseline", [])]; st = [r for x in m for r in x.get("stress", [])]
    S[f"{v}/main"] = {"n": len(bl), "first_quote_s": med([r["first_quote_s"] for r in bl]), "rss60": med([r["steady60"]["rss_mb"] for r in bl]),
                      "pss60": med([r["steady60"]["pss_mb"] for r in bl]), "cpu": med([r["cpu_pct"] for r in bl]), "fps": med([r["fps_avg"] for r in bl])}
    S[f"{v}/scroll"] = {"n": len(st), "scrollFps": med([r["done"]["scrollFps"] for r in st]), "jank": med([r["done"]["scrollJankCount"] for r in st]),
                        "cpu_scroll": med([r.get("cpu_scroll_pct") for r in st])}
    mu = [r for x in runs(v, "multi", "multi-results.json") for r in x]
    for w in (1, 4):
        rs = [r for r in mu if r["windows"] == w]
        S[f"{v}/multi{w}w"] = {"n": len(rs), "lat_p50": med([r["lat"]["p50"] for r in rs]), "lat_p99": med([r["lat"]["p99"] for r in rs]),
                               "fps_min_window": med([r["fpsMin"] for r in rs]), "cpu": med([r["cpu"] for r in rs]),
                               "pss": med([r["mem"]["end"]["pss_mb"] for r in rs]), "worst_window_p99": med([r["worstP99"] for r in rs]), "loss": med([r["ticks"]["loss"] for r in rs])}
    tk = [r for x in runs(v, "ticks", "ticks-results.json") for r in x]
    for sc in ("s1k", "s10k", "s50k", "s100k", "burst"):
        rs = [r for r in tk if r["scenario"] == sc]
        if not rs: continue
        S[f"{v}/ticks-{sc}"] = {"n": len(rs), "lat_p50": med([r["latOldest"]["p50"] for r in rs]), "lat_p99": med([r["latOldest"]["p99"] for r in rs]),
                                "fps": med([r["fps"] for r in rs]), "cpu": med([r["cpu"] for r in rs]), "loss": med([r["ticks"]["lossFinal"] for r in rs]),
                                "switch_p50": med([r["switch"]["p50"] for r in rs])}
json.dump(S, open(os.path.join(os.path.dirname(B), "summary.json"), "w"), ensure_ascii=False, indent=1)
for k, v in S.items(): print(k, v)
