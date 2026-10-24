#!/usr/bin/env python3
"""results.jsonl → summary.json（每個 app×情境×指標 取各輪中位數，附 n）＋ summary.md"""
import json, statistics, sys, os
d = sys.argv[1]
rows = [json.loads(l) for l in open(os.path.join(d, "results.jsonl")) if l.strip()]
# 有效性規則（每一列一視同仁）：
#  - 觸控各段 fpsMean < 0.5：畫面停住（資料照收、沒出幀）→ 排除
#  - 高頻進價 samples == 0：沒量到延遲 → 排除
#  - heavy 沒跑完（done=false）：只留跑完的段落 fps 與啟動時的指標計算，牆鐘／CPU／PSS 不算
TOUCH = {"idle-list", "fling", "chart-idle", "pan", "pinch", "crosshair"}
excluded = []
def valid(r):
    if r.get("phase") in TOUCH and (r.get("fpsMean") or 0) < 0.5:
        excluded.append((r["app"], r["run"], r["phase"], "畫面停住 fpsMean<0.5")); return False
    if str(r.get("phase", "")).startswith("tick") and not r.get("samples"):
        excluded.append((r["app"], r["run"], r["phase"], "沒有延遲樣本")); return False
    if r.get("phase") == "heavy" and r.get("done") is False:
        excluded.append((r["app"], r["run"], "heavy(部分)", "沒跑完：只留已完成段落 fps＋指標計算"))
        for k in ("wallS", "appCpuPct", "emuCpuPct", "pssMb"):
            r[k] = None
    return True
rows = [r for r in rows if "error" in r or valid(r)]
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
# 指標「補算最後一根」：從原始 touch logcat 取走勢圖畫面每秒的 indPatchUs，取每輪中位數（比平均不受單一離群秒影響）
import glob, re
for f in sorted(glob.glob(os.path.join(d, "*-r*-touch.log"))):
    m = re.match(r"(.+)-r(\d+)-touch\.log", os.path.basename(f))
    if not m:
        continue
    app, run = m.group(1), int(m.group(2))
    if not any(r["app"] == app and r["run"] == run and r["phase"] == "chart-mem" for r in rows):
        continue  # 只算完整的那幾輪
    v = []
    for ln in open(f, errors="replace"):
        i = ln.find("FRAMES|")
        if i < 0:
            continue
        try:
            j = json.loads(ln[i + 7:])
        except Exception:
            continue
        if j.get("screen") == "chart" and j.get("indPatch"):
            v.append(j["indPatchUs"])
    if v:
        a = agg.setdefault((app, "indicator"), {})
        a.setdefault("patchUsMedian", []).append(round(statistics.median(v), 2))
        a.setdefault("patchSamples", []).append(len(v))
out = {}
for (app, ph), m in sorted(agg.items()):
    o = out.setdefault(ph, {}).setdefault(app, {})
    for k, v in m.items():
        if k == "_errors":
            o["errors"] = v
        else:
            o[k] = {"median": round(statistics.median(v), 2), "n": len(v), "all": v}
# 指標計算：每筆 tick 只補最後一根（FRAMES indPatchUs），把所有觸控 log 的每秒值合在一起取中位數／p90
import glob, re
pooled = {}
for app in sorted({r["app"] for r in rows}):
    v = []
    for fn in sorted(glob.glob(os.path.join(d, f"{app}-r*-touch.log"))):
        for ln in open(fn, errors="replace"):
            m = re.search(r"FRAMES\|(\{.*\})", ln)
            if m:
                fr = json.loads(m.group(1))
                if fr.get("indPatch"):
                    v.append(fr["indPatchUs"])
    if v:
        v.sort()
        pooled[app] = {"n": len(v), "medianUs": round(statistics.median(v), 1), "p90Us": round(v[int(len(v) * 0.9)], 1)}
out["_indicator_patch_pooled"] = pooled
out["_excluded"] = excluded
json.dump(out, open(os.path.join(d, "summary.json"), "w"), ensure_ascii=False, indent=1)
with open(os.path.join(d, "summary.md"), "w") as f:
    for ph, apps in out.items():
        if ph == "_indicator_patch_pooled":
            f.write("\n### 指標每筆 tick 補最後一根（全部觸控 log 合併）\n\n" + "".join(f"- {a}: {v}\n" for a, v in apps.items())); continue
        if ph == "_excluded":
            f.write("\n### 排除的列\n\n" + "".join(f"- {e}\n" for e in apps)); continue
        keys = sorted({k for a in apps.values() for k in a if k not in ("errors",)})
        f.write(f"\n### {ph}\n\n| 指標 | " + " | ".join(apps) + " |\n|---|" + "---|" * len(apps) + "\n")
        for k in keys:
            f.write(f"| {k} | " + " | ".join(
                (f"{apps[a][k]['median']} (n={apps[a][k]['n']})" if k in apps[a] else "—") for a in apps) + " |\n")
        for a in apps:
            if "errors" in apps[a]:
                f.write(f"\n- {a} 錯誤：{apps[a]['errors']}\n")
print(open(os.path.join(d, "summary.md")).read())
