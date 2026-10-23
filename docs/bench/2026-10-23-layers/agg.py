# 交錯重測彙整：每個 程式×層數 取各輪中位數 → ilv/summary-ilv.json ＋ 印表
import json,glob,statistics as st,re,ast,os
B=os.path.dirname(os.path.abspath(__file__)); I=B+"/ilv"
def med(xs):
    xs=[x for x in xs if isinstance(x,(int,float))]; return round(st.median(xs),3) if xs else None
out={}
for app in ("wry","gpui","egui"):
    for L in (100,1):
        runs=[]
        for d in sorted(glob.glob(f"{I}/r*-{app}-L{L}")):
            f=d+"/summary-heavy.json"
            if os.path.exists(f): runs.append(json.load(open(f))[app]["heavy"])
        if not runs: continue
        o={"runs":len(runs),"heavy_frame_ms":med([r["heavy_frame_ms"] for r in runs])}
        for ph in ("ticks","crosshair","pan","drag","select","reorder"):
            ps=[r[ph] for r in runs if ph in r]
            o[ph]={k:med([p.get(k) for p in ps]) for k in ("fps","paint_ms","cpu_pct","cpu_nollvm_pct","rss_mb","pss_mb")}
            for k in ("allRebuildP50","allRebuildP99","allTtfP50","allTtfP99","allOps","drawRebuilds","rebuildMsTotal"):
                o[ph][k]=med([p.get("perf_med",{}).get(k) for p in ps])
        out[f"{app}-L{L}"]=o
ab={"base":[],"new":[]}
if os.path.exists(I+"/ab.log"):
    for line in open(I+"/ab.log"):
        m=re.match(r"gpui-(base|new) (\{.*\})",line)
        if m: ab[m.group(1)].append(ast.literal_eval(m.group(2)))
out["gpui_ab"]={v:{ph:med([x[ph][0] for x in ab[v] if ph in x]) for ph in ("ticks","crosshair","pan")}|{"n":len(ab[v])} for v in ab}
json.dump(out,open(I+"/summary-ilv.json","w"),ensure_ascii=False,indent=1)
for k,o in out.items():
    if k=="gpui_ab": print(k,o); continue
    s=" | ".join(f"{ph} {o[ph]['fps']}fps cpuNo{o[ph]['cpu_nollvm_pct']}" + (f" rb{o[ph]['allRebuildP50']}/{o[ph]['allRebuildP99']} ttf{o[ph]['allTtfP50']}/{o[ph]['allTtfP99']}" if ph in("select","reorder") else "") for ph in ("ticks","crosshair","pan","drag","select","reorder"))
    print(f"{k} n={o['runs']} frame={o['heavy_frame_ms']} rss={o['ticks']['rss_mb']} :: {s}")
