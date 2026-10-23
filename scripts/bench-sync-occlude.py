#!/usr/bin/env python3
"""多視窗報價同步＋遮蔽驗證（2026-10-23）。
XQ_WINDOWS=4（4K Xvfb、縮放 2 倍、2×2 鋪滿）、xq-tickgen 10k 筆/秒、XQ_SYNC_CHECK=1。
時間軸（量測開始後）：0–8s 全開；8–13s 把主視窗（#1，持有行情連線）unmap；13–18s 全開；18–23s unmap #4；23–30s 全開。
每秒每個視窗一行 SYNC|{...}（幀數、與共用報價表不一致次數、落後超過一幀次數、最大落後 ms）。
輸出 JSON：每個視窗每一段的統計 + 重新顯示後第一幀的時間。"""
import json, os, re, signal, subprocess, sys, threading, time
GPUI = sys.argv[1]; OUT = sys.argv[2]; DISP = os.environ.get("D", ":62"); ADDR = os.environ.get("ADDR", "127.0.0.1:47781")
os.makedirs(os.path.dirname(OUT) or ".", exist_ok=True)
gen = subprocess.Popen([f"{GPUI}/target/release/xq-tickgen", "--addr", ADDR, "--rate", "10000", "--duration", "30", "--settle", "3", "--warmup", "3"],
                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
gev = []
threading.Thread(target=lambda: [gev.append((time.time(), l)) for l in gen.stdout], daemon=True).start()
time.sleep(0.5)
env = dict(os.environ, DISPLAY=DISP, XQ_BENCH="1", XQ_TICK_BENCH="1", XQ_PERIOD="D", XQ_FEED_ADDR=ADDR, GPUI_X11_SCALE_FACTOR="2",
           XQ_WINDOWS="4", XQ_SYNC_CHECK="1")
app = subprocess.Popen([f"{GPUI}/target/release/gpui-xq-demo"], cwd=GPUI, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
lines = []
threading.Thread(target=lambda: [lines.append((time.time(), l)) for l in app.stderr], daemon=True).start()
def xd(*a): return subprocess.run(["xdotool", *a], env=dict(os.environ, DISPLAY=DISP), capture_output=True, text=True).stdout.split()
wins = {}
end = time.time() + 30
while time.time() < end and len(wins) < 4:
    for w in xd("search", "--name", "XQ .*GPUI"):
        name = subprocess.run(["xdotool", "getwindowname", w], env=dict(os.environ, DISPLAY=DISP), capture_output=True, text=True).stdout.strip()
        m = re.search(r"#(\d)", name); k = int(m.group(1)) - 1 if m else 0
        wins[k] = w
    time.sleep(0.3)
for k, w in wins.items():
    x, y = (k % 2) * 1920, (k // 2) * 1080
    xd("windowmove", "--sync", w, str(x), str(y)); xd("windowsize", "--sync", w, "1920", "1080")
def wait_ev(ev, t=90):
    e = time.time() + t
    while time.time() < e:
        for ts, l in gev:
            if f'"ev":"{ev}"' in l: return ts
        time.sleep(0.02)
    raise SystemExit(f"no {ev}")
t0 = wait_ev("start")
events = []
def at(sec, f, label):
    while time.time() < t0 + sec: time.sleep(0.005)
    events.append((label, time.time())); f()
at(8, lambda: xd("windowunmap", wins[0]), "hide0")
at(13, lambda: xd("windowmap", wins[0]), "show0")
at(18, lambda: xd("windowunmap", wins[3]), "hide3")
at(23, lambda: xd("windowmap", wins[3]), "show3")
wait_ev("end", 30); time.sleep(1.5)
app.send_signal(signal.SIGTERM); app.wait(10); gen.wait(10)
# 解析
sync = []; tb = []
for ts, l in lines:
    if "SYNC|" in l: sync.append((ts, json.loads(l.split("SYNC|", 1)[1])))
    if "TICKBENCH|" in l: tb.append((ts, json.loads(l.split("TICKBENCH|", 1)[1])))
ev = dict(events)
segs = [("all-open-1", t0 + 1, ev["hide0"]), ("primary-hidden", ev["hide0"] + 1.0, ev["show0"]), ("all-open-2", ev["show0"] + 1.0, ev["hide3"]),
        ("w3-hidden", ev["hide3"] + 1.0, ev["show3"]), ("all-open-3", ev["show3"] + 1.0, t0 + 30)]
res = {"events": {k: round(v - t0, 3) for k, v in ev.items()}, "segments": {}}
for name, a, b in segs:
    per = {}
    for k in range(4):
        rows = [j for ts, j in sync if j["win"] == k and a <= ts <= b]
        lat = [(s[6] - s[1]) / 1000 for ts, j in tb if j["win"] == k for s in j["lat"] if a * 1e6 <= s[6] <= b * 1e6]
        lat.sort()
        per[k] = {"secs": len(rows), "frames": sum(r["frames"] for r in rows), "mismatch": sum(r["mismatch"] for r in rows),
                  "violations": sum(r["violations"] for r in rows), "maxStaleMs": max((r["maxStaleMs"] for r in rows), default=None),
                  "frameMsP50": sorted(r["frameMsP50"] for r in rows)[len(rows) // 2] if rows else None,
                  "latP50": lat[len(lat) // 2] if lat else None, "latP99": lat[int(len(lat) * .99)] if lat else None, "latN": len(lat),
                  "fallbackApplies": (rows[-1]["fallbackApplies"] - rows[0]["fallbackApplies"]) if rows and k == 0 else None}
    res["segments"][name] = per
# 重新顯示後第一幀：show 之後第一個 SYNC 行裡 frames>0 的時間只到秒級；改用 TICKBENCH 樣本 present 時間
for k, lab in ((0, "show0"), (3, "show3")):
    pres = sorted(s[6] for ts, j in tb if j["win"] == k for s in j["lat"] if s[6] >= ev[lab] * 1e6)
    res[f"firstPresentAfter_{lab}_ms"] = round(pres[0] / 1000 - ev[lab] * 1000, 1) if pres else None
res["violationsTotal"] = sum(j["violations"] for ts, j in sync if t0 + 1 <= ts <= t0 + 30)
json.dump(res, open(OUT, "w"), indent=1)
print(json.dumps(res, indent=1))
