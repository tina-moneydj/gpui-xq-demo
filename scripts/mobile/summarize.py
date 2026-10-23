#!/usr/bin/env python3
"""results.jsonl → summary.json（每個 app×情境×指標 取各輪中位數，附 n）＋ summary.md"""
import json, statistics, sys, os
d = sys.argv[1]
rows = [json.loads(l) for l in open(os.path.join(d, "results.jsonl")) if l.strip()]
agg = {}
for r in rows:
    if "error" in r:
        agg.setdefault((r["app"], r["phase"]), {}).setdefault("_errors", []).append(r["error"])
        continue
    if r["phase"] == "heavy":
        flat = {k: r.get(k) for k in ("wallS", "appCpuPct", "emuCpuPct", "pssMb", "done")}
        if r.get("indperf"):
            flat["indMedianMs"] = r["indperf"]["medianMs"]; flat["indBars"] = r["indperf"]["bars"]
        for p in r.get("chartperf", []):
            ph = p.get("phase")
            if ph == "setup-begin":
                flat["indicatorSetupMs"] = p.get("indicatorSetupMs")
            elif ph == "setup":
                flat["setupRebuildMs"] = p.get("rebuildMs"); flat["setupSinceStartMs"] = p.get("sinceStartMs")
            elif "fps" in p:
                flat[f"{ph}.fps"] = p.get("fps"); flat[f"{ph}.avgPaintMs"] = p.get("avgPaintMs")
        r = {**r, **flat}
    a = agg.setdefault((r["app"], r["phase"]), {})
    for k, v in r.items():
        if k in ("app", "run", "phase", "chartperf", "indperf") or isinstance(v, (bool, str)) or v is None:
            continue
        a.setdefault(k, []).append(v)
out = {}
for (app, ph), m in sorted(agg.items()):
    o = out.setdefault(ph, {}).setdefault(app, {})
    for k, v in m.items():
        if k == "_errors":
            o["errors"] = v
        else:
            o[k] = {"median": round(statistics.median(v), 2), "n": len(v), "all": v}
json.dump(out, open(os.path.join(d, "summary.json"), "w"), ensure_ascii=False, indent=1)
with open(os.path.join(d, "summary.md"), "w") as f:
    for ph, apps in out.items():
        keys = sorted({k for a in apps.values() for k in a if k not in ("errors",)})
        f.write(f"\n### {ph}\n\n| 指標 | " + " | ".join(apps) + " |\n|---|" + "---|" * len(apps) + "\n")
        for k in keys:
            f.write(f"| {k} | " + " | ".join(
                (f"{apps[a][k]['median']} (n={apps[a][k]['n']})" if k in apps[a] else "—") for a in apps) + " |\n")
        for a in apps:
            if "errors" in apps[a]:
                f.write(f"\n- {a} 錯誤：{apps[a]['errors']}\n")
print(open(os.path.join(d, "summary.md")).read())
