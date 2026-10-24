#!/usr/bin/env python3
"""A3 v2 正確性：XQ_ZTEST=N 讓 app 輪流用 grouped／seq／paths 畫同一組隨機畫線，每個情境截一張圖；grouped 與 seq 要逐像素相同。"""
import json, os, subprocess, sys, time
import numpy as np
from PIL import Image
APP = sys.argv[1]; N = int(sys.argv[2]); OUT = sys.argv[3]; DISP = os.environ.get("ZDISP", ":73")
os.makedirs(OUT, exist_ok=True)
PLOT_Y1 = 850
xvfb = None
if subprocess.run(["xdpyinfo", "-display", DISP], capture_output=True).returncode != 0:
    xvfb = subprocess.Popen(["Xvfb", DISP, "-screen", "0", "1600x1000x24", "-ac", "+extension", "GLX", "+render", "-noreset"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); time.sleep(1.5)
env = dict(os.environ, DISPLAY=DISP, XQ_BENCH="1", XQ_CHART_STRESS="1", XQ_CHART_WAIT_MS="500", XQ_ZTEST=str(N),
           XQ_FEED_ADDR="127.0.0.1:1", XQ_GPUI_STATE=os.path.join(OUT, "state.json"), XQ_TOP_FRAC="0.8", XQ_PERIOD="D")
env.pop("XQ_DRAW_QUADS", None)
p = subprocess.Popen([APP], cwd=os.path.dirname(os.path.dirname(os.path.dirname(APP))), env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
rows = []; log = open(os.path.join(OUT, "app.log"), "w")
try:
    t0 = time.time()
    for line in p.stderr:
        log.write(line); log.flush()
        if line.startswith("ZTEST|done"): break
        if line.startswith("ZTEST|{\"case\""):
            j = json.loads(line.split("|", 1)[1]); rows.append(j)
            j["tag"] = ["grouped", "seq", "paths", "seq2"][j["case"] % 4]
            f = os.path.join(OUT, "s%02d-%s.png" % (j["seed"], j["tag"]))
            subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-i", DISP, "-frames:v", "1", f], check=True)
            print(line.strip(), flush=True)
        if time.time() - t0 > 60 + N * 12: print("timeout"); break
finally:
    p.kill(); p.wait()
    if xvfb: xvfb.kill()
res = []
for s in sorted({r["seed"] for r in rows}):
    im = {m: np.asarray(Image.open(os.path.join(OUT, "s%02d-%s.png" % (s, m))).convert("RGB")).astype(int) for m in ("grouped", "seq", "paths", "seq2")
          if os.path.exists(os.path.join(OUT, "s%02d-%s.png" % (s, m)))}
    def cmp(a, b):
        if a not in im or b not in im: return None
        d = np.abs(im[a] - im[b]).max(axis=2)[:PLOT_Y1]  # 底下狀態列（FPS、時間）每張不同，不比
        return {"diffPixels": int((d > 0).sum()), "diffOver8": int((d > 8).sum()), "maxDiff": int(d.max())}
    st = {r["tag"]: r for r in rows if r["seed"] == s}
    res.append({"seed": s, "drawings": st["grouped"]["drawings"], "layers": st["grouped"]["layers"], "selected": st["grouped"]["selected"],
                "grouped_vs_seq": cmp("grouped", "seq"), "seq_vs_seq2": cmp("seq", "seq2"), "grouped_vs_paths": cmp("grouped", "paths"),
                "stats": {m: {k: st[m][k] for k in ("elems", "groups", "paths", "pathBatches", "rebuildMs")} for m in st}})
json.dump({"rows": rows, "compare": res}, open(os.path.join(OUT, "ztest.json"), "w"), ensure_ascii=False, indent=1)
for r in res: print(r["seed"], r["drawings"], "g-vs-seq", r["grouped_vs_seq"], "seq-vs-seq2", r["seq_vs_seq2"], "g-vs-paths", r["grouped_vs_paths"], {m: (v["groups"], v["paths"], v["pathBatches"]) for m, v in r["stats"].items()})
