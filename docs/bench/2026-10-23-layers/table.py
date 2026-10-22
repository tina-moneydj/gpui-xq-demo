import json,sys
def load(d): return json.load(open(f"/workspace/bench/layers-20261023/{d}/summary-heavy.json"))
for d in sys.argv[1:]:
    s=load(d); print("==",d,s.get("date"))
    for a in ("wry","gpui","egui"):
        h=s[a]["heavy"]; row=[]
        for ph in ("ticks","crosshair","pan","drag","select","reorder"):
            p=h[ph]; pm=p.get("perf_med",{})
            x=f"{ph}:{p['fps']}"
            if ph in("select","reorder"): x+=f" rb{pm.get('allRebuildP50')}/{pm.get('allRebuildP99')} ttf{pm.get('allTtfP50')}/{pm.get('allTtfP99')}"
            x+=f" cpuNo{p['cpu_nollvm_pct']:.0f}"
            row.append(x)
        print(a, f"frame={h['heavy_frame_ms']:.0f} rss={h['ticks']['rss_mb']:.0f}", " | ".join(row))
