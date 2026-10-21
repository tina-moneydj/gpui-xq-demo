#!/usr/bin/env bash
# 高解析多視窗壓測（2026-10-21）：wry-xq-demo vs gpui-xq-demo vs egui-xq-demo
#
# 畫面：獨立的 Xvfb（預設 :3，3840×2160）、程式縮放 2 倍（HiDPI：GPUI_X11_SCALE_FACTOR=2、WINIT_X11_SCALE_FACTOR=2、GDK_SCALE=2）。
#   XQ_WINDOWS=N 開 N 個看盤視窗（同一個程序），每個 1920×1080 實體像素（960×540 邏輯）、2×2 鋪滿螢幕不重疊；
#   每個視窗＝走勢圖（自選第 k 檔日線＋MA＋量）＋5 萬 33 列報價表＋資訊格。行情連線／解碼只有一份，三個 app 都是單一程序：
#   GPUI：多個視窗共用同一個 QuoteTable entity（同一份資料、只合併一次），每個視窗自己的走勢圖／資訊格 view；
#   egui：immediate viewport，共用同一個 QuoteTable（含 Mesh 快取），每個視窗自己的 ChartView；
#   wry：每個視窗一個 WebView（JS 各存一份報價表——WebView 架構無法共用），Rust 端同一包報價借用合併進每個視窗的批次。
# 行情：xq-tickgen 10k 筆/秒（同 bench-ticks.sh s10k：每 5ms 一包、快照 50,033 檔、暖身 3 秒、量 30 秒）。
# 量測（每輪）：整棵行程樹 RSS／PSS、CPU（含／扣 llvmpipe）、每個視窗的 FPS、延遲 tick→畫面 p50/p99（全部視窗合併＋各視窗）。
#   不點擊（切換延遲在 bench-ticks.sh 量）。沒有 GPU：FPS 是軟體光柵化的上限，不代表真機；記憶體／CPU 的趨勢才有參考價值。
# 輸出：$OUT/multi-results.json、$OUT/multi-summary.json、$OUT/multi-<app>-run.log、截圖。RESUME=1 跳過已完成的輪次。
# 用法：bench-multi.sh [runs=3] [windows=1,4] [apps=gpui,wry,egui]
set -euo pipefail
RUNS="${1:-3}"
WINS="${2:-1,4}"
APPS_SEL="${3:-gpui,wry,egui}"
export DISPLAY="${MULTI_DISPLAY:-:42}"  # 獨立的 4K Xvfb（:3 在這台機器被桌面管理程式回收，改用 :42）
WRY_DIR="${WRY_DIR:-/workspace/wry-xq-demo}"
GPUI_DIR="${GPUI_DIR:-/workspace/gpui-xq-demo}"
EGUI_DIR="${EGUI_DIR:-/workspace/egui-xq-demo}"
OUT="${OUT:-/workspace/bench/multi}"
TICK_ADDR="${TICK_ADDR:-127.0.0.1:47631}"
WRY_BIN="${WRY_BIN:-./target/release/wry-xq-demo}"
SCALE="${SCALE:-2}"
mkdir -p "$OUT"
export RUNS WINS APPS_SEL WRY_DIR GPUI_DIR EGUI_DIR OUT TICK_ADDR WRY_BIN SCALE
exec python3 - <<'PY'
import json, os, signal, statistics, subprocess, sys, threading, time

RUNS = int(os.environ["RUNS"]); OUT = os.environ["OUT"]; SCALE = os.environ["SCALE"]
WINS = [int(x) for x in os.environ["WINS"].split(",")]; APPS_SEL = os.environ["APPS_SEL"].split(",")
WRY_DIR = os.environ["WRY_DIR"]; GPUI_DIR = os.environ["GPUI_DIR"]; EGUI_DIR = os.environ["EGUI_DIR"]; ADDR = os.environ["TICK_ADDR"]
TW, TH = 1920, 1080  # 每個視窗的實體像素（2×2 鋪滿 3840×2160）
CLK = os.sysconf("SC_CLK_TCK")
XDG = os.path.join(OUT, "xdg")
DUR, SETTLE, WARMUP, TAIL = 30.0, 3.0, 3.0, 4.0
TICKGEN = os.path.join(GPUI_DIR, "target/release/xq-tickgen")
GEN_ARGS = ["--rate", "10000"]
WATCH = ["2330","2317","2454","0050","2303","2881","2882","2308","2412","2382","3711","2891","2886","1301","1303",
         "2002","3008","2357","2603","2609","3231","6505","2884","2892","5880","2207","1216","2379","3034","2345",
         "AAPL","TSLA","NVDA"]

def wry_bench_state():
    # 同 bench-main-window.sh：2330 日線＋MA5/20/60＋量、左下組合分頁 6 欄、右下空白
    off = lambda **kw: dict(on=False, **kw)
    cfg = {"atr": off(n=14), "bb": off(k=2, n=20), "dmi": off(n=14), "ema": off(p1=12, p2=26),
           "kd": off(d=3, k=3, n=14), "ma": {"on": True, "p1": 5, "p2": 20, "p3": 60},
           "macd": off(fast=12, sig=9, slow=26), "obv": off(), "rsi": off(n=14),
           "sar": off(max=0.2, step=0.02), "vol": {"on": True}, "volma": off(p1=5, p2=20), "wr": off(n=14)}
    key = {"cfg": cfg, "drawings": [], "overlays": [{"period": p, "type": "ma"} for p in (5, 20, 60)]}
    return {
        "v": 2,
        "ui": {"feed": "engine", "period": "D", "subLayout": {"left": 1, "right": 3}},
        "watchlist": WATCH,
        "keys": {"2330|D": key, "chart-1|2330|D": key},
        "groups": {"activeGroupId": "g-watch",
                   "columns": [{"field": f, "id": i, "label": l, "width": w} for f, i, l, w in (
                       ("symbol", "c-sym", "代號", 58), ("name", "c-name", "名稱", 92), ("price", "c-px", "成交", 76),
                       ("change", "c-ch", "漲跌", 70), ("pct", "c-pct", "漲幅%", 64), ("volume", "c-vol", "成交量", 92))],
                   "groups": [{"id": "g-watch", "name": "自選", "symbols": WATCH}]},
        "panes": {
            "top": {"active": "chart-1", "tabs": [{"id": "chart-1", "kind": "chart", "period": "D",
                                                   "symbol": "2330", "title": "2330 台積電"}]},
            "left": {"active": "group-1", "tabs": [{"id": "group-1", "kind": "group", "title": "組合"}]},
            "right": {"active": "web-home", "tabs": [{"id": "web-home", "kind": "web", "title": "空白網頁",
                                                      "url": "about:blank"}]},
        },
    }


APPS = {
    "wry": dict(cwd=WRY_DIR, cmd=[os.environ["WRY_BIN"]], title=r"^XQ .*tao \+ wry",
                env={"WEBKIT_DISABLE_COMPOSITING_MODE": "1", "XQ_BENCH": "1", "XQ_TICK_BENCH": "1",
                     "XQ_FEED_ADDR": ADDR, "XDG_DATA_HOME": XDG, "GDK_SCALE": SCALE}),
    "gpui": dict(cwd=GPUI_DIR, cmd=["./target/release/gpui-xq-demo"], title="^XQ .*GPUI",
                 env={"XQ_BENCH": "1", "XQ_TICK_BENCH": "1", "XQ_PERIOD": "D", "XQ_FEED_ADDR": ADDR,
                      "GPUI_X11_SCALE_FACTOR": SCALE}),
    "egui": dict(cwd=EGUI_DIR, cmd=["./target/release/egui-xq-demo"], title="^XQ .*egui",
                 env={"XQ_BENCH": "1", "XQ_TICK_BENCH": "1", "XQ_PERIOD": "D", "XQ_FEED_ADDR": ADDR,
                      "XQ_RENDERER": os.environ.get("EGUI_RENDERER", "glow"), "XQ_EGUI_STATE": "/tmp/egui-bench-state.json",
                      "WINIT_X11_SCALE_FACTOR": SCALE}),
}

def ppid(p):
    try: return int(open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()[1])
    except Exception: return -1

def tree(root):
    pids = [int(e) for e in os.listdir("/proc") if e.isdigit()]
    s = {root}; changed = True
    while changed:
        changed = False
        for p in pids:
            if p not in s and ppid(p) in s: s.add(p); changed = True
    return sorted(s)

def comm(p):
    try: return open(f"/proc/{p}/comm").read().strip()
    except Exception: return "?"

def mem(p):
    r = {"rss": 0, "pss": 0}
    try:
        for l in open(f"/proc/{p}/status"):
            if l.startswith("VmRSS:"): r["rss"] = int(l.split()[1])
        for l in open(f"/proc/{p}/smaps_rollup"):
            if l.startswith("Pss:"): r["pss"] = int(l.split()[1])
    except Exception: pass
    return r

def mem_tree(root):
    ms = [mem(p) for p in tree(root)]
    return {"rss_mb": round(sum(m["rss"] for m in ms) / 1024, 1), "pss_mb": round(sum(m["pss"] for m in ms) / 1024, 1)}

def thread_ticks(pids):
    out = {}
    for p in pids:
        try: tids = os.listdir(f"/proc/{p}/task")
        except Exception: continue
        pc = comm(p)
        for t in tids:
            try:
                st = open(f"/proc/{p}/task/{t}/stat").read()
                name = st[st.index("(") + 1:st.rindex(")")].rstrip("0123456789:-_ ") or "?"
                f = st.rsplit(")", 1)[1].split()
                key = f"{pc}/{name}"
                out[key] = out.get(key, 0) + int(f[11]) + int(f[12])
            except Exception: pass
    return out

def proc_ticks(pids):
    # 程序層級 utime+stime（含已結束的執行緒；llvmpipe 執行緒會開開關關，只加總執行緒會算出負值）
    out = {}
    for p in pids:
        try:
            f = open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()
            out[p] = int(f[11]) + int(f[12])
        except Exception: pass
    return out

def cpu_split(a, b, secs, pa=None, pb=None):
    d = {k: max(0, b.get(k, 0) - a.get(k, 0)) for k in b}
    if pa is not None:
        tot = sum(max(0, pb[p] - pa.get(p, 0)) for p in pb) / CLK / secs * 100
    else:
        tot = sum(d.values()) / CLK / secs * 100
    llvm = sum(v for k, v in d.items() if "llvmpipe" in k) / CLK / secs * 100
    top = {k: round(v / CLK / secs * 100, 1) for k, v in sorted(d.items(), key=lambda kv: -kv[1])[:6] if v / CLK / secs * 100 >= 1}
    return round(tot, 1), round(max(0.0, tot - llvm), 1), top

def screenshot(path):
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-video_size", os.environ.get("SCREEN", "3840x2160"),
                    "-i", os.environ["DISPLAY"], "-frames:v", "1", path], check=True)

def unix_us(): return int(time.time() * 1e6)

def pct(v, q):
    if not v: return None
    v = sorted(v); i = min(len(v) - 1, max(0, int(round(q / 100 * (len(v) - 1)))))
    return v[i]

def find_windows(title):
    r = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", title], capture_output=True, text=True)
    return r.stdout.split()

def open_windows(name, app, n, t0):
    ids = []
    while time.monotonic() - t0 < 40:
        ids = find_windows(app["title"])
        if len(ids) >= n: break
        time.sleep(0.05)
    if len(ids) < n: raise SystemExit(f"{name}: 只找到 {len(ids)}/{n} 個視窗")
    # 依標題排序：主視窗（沒有 #k）在前
    titled = []
    for w in ids:
        t = subprocess.run(["xdotool", "getwindowname", w], capture_output=True, text=True).stdout.strip()
        k = int(t.rsplit("#", 1)[1]) - 1 if "#" in t else 0
        titled.append((k, w))
    titled.sort()
    for k, w in titled[:n]:
        x, y = (k % 2) * TW, (k // 2) * TH
        subprocess.run(["xdotool", "windowmove", "--sync", w, str(x), str(y)])
        subprocess.run(["xdotool", "windowsize", "--sync", w, str(TW), str(TH)])
    return [w for _, w in titled[:n]]

def run_once(name, n, shot):
    app = APPS[name]
    gev = []
    gen = subprocess.Popen([TICKGEN, "--addr", ADDR, *GEN_ARGS, "--duration", str(DUR), "--settle", str(SETTLE),
                            "--warmup", str(WARMUP)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    def gen_reader():
        for line in gen.stdout:
            try: gev.append((time.monotonic(), json.loads(line)))
            except Exception: gev.append((time.monotonic(), {"ev": "raw", "line": line.strip()}))
    threading.Thread(target=gen_reader, daemon=True).start()
    def gwait(ev, timeout):
        end = time.monotonic() + timeout
        while True:
            for t, j in gev:
                if j.get("ev") == ev: return t, j
                if j.get("ev") == "error": raise SystemExit(f"tickgen error: {j}")
            if time.monotonic() > end: raise SystemExit(f"{name}/{n}w: 產生器等不到 {ev}: {gev[-3:]}")
            time.sleep(0.01)
    gwait("listen", 5)
    if name == "wry":
        os.makedirs(f"{XDG}/wry-xq-demo", exist_ok=True)
        json.dump(wry_bench_state(), open(f"{XDG}/wry-xq-demo/state.json", "w"), ensure_ascii=False, indent=2)
    env = dict(os.environ, **app["env"], XQ_WINDOWS=str(n))
    tb = []; log = []
    t0 = time.monotonic()
    proc = subprocess.Popen(app["cmd"], cwd=app["cwd"], env=env, stdin=subprocess.DEVNULL,
                            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, bufsize=1)
    def reader():
        for line in proc.stderr:
            t = time.monotonic()
            if "TICKBENCH|" in line:
                try: tb.append((t, json.loads(line.split("TICKBENCH|", 1)[1])))
                except Exception: log.append(f"{t - t0:8.3f} BAD {line[:200]}")
                continue
            log.append(f"{t - t0:8.3f} {line.rstrip()}")
    threading.Thread(target=reader, daemon=True).start()
    try:
        open_windows(name, app, n, t0)
        t_start, j_start = gwait("start", 90)
        start_us = j_start["unixUs"]
        pids = tree(proc.pid)
        th0 = thread_ticks(pids); p0 = proc_ticks(pids); g0 = thread_ticks([gen.pid]); m0 = mem_tree(proc.pid)
        mems = [(0.0, m0)]
        shot_path = None; next_mem = 2.0
        while True:
            el = time.monotonic() - t_start
            if any(j.get("ev") == "end" for _, j in gev) or el > DUR + 30: break
            if el >= next_mem:
                mems.append((round(el, 1), mem_tree(proc.pid))); next_mem += 2.0
            if shot and shot_path is None and el >= 15:
                shot_path = f"{OUT}/multi-{name}-{n}w.png"; screenshot(shot_path)
            time.sleep(0.05)
        t_end, j_end = gwait("end", 5)
        secs = t_end - t_start
        pids = tree(proc.pid)
        th1 = thread_ticks(pids); p1 = proc_ticks(pids); g1 = thread_ticks([gen.pid]); m1 = mem_tree(proc.pid)
        mems.append((round(secs, 1), m1))
        time.sleep(TAIL)
        m_tail = mem_tree(proc.pid)
        procs = sorted({comm(p) for p in tree(proc.pid)})
    finally:
        kids = tree(proc.pid)
        proc.send_signal(signal.SIGTERM)
        try: proc.wait(10)
        except subprocess.TimeoutExpired: proc.kill(); proc.wait()
        time.sleep(0.5)
        for p in kids:
            if p != proc.pid and os.path.exists(f"/proc/{p}"):
                try: os.kill(p, signal.SIGTERM)
                except ProcessLookupError: pass
        gen.terminate()
        try: gen.wait(5)
        except subprocess.TimeoutExpired: gen.kill(); gen.wait()
        open(f"{OUT}/multi-{name}-run.log", "a").write(f"==== {n}w {time.strftime('%F %T')}\n" + "\n".join(log) + "\n")
    return analyze(name, n, tb, t_start, t_end, start_us, j_end, (th0, p0), (th1, p1), g0, g1, secs, mems, m_tail, shot_path, procs)

def recv_of(name, j):
    return j["rowsIn"] if name in ("gpui", "egui") else j["raw"]

def analyze(name, n, tb, t_start, t_end, start_us, j_end, th0, th1, g0, g1, secs, mems, m_tail, shot, procs):
    end_us = j_end["unixUs"]; sent = j_end["sent"]
    r = lambda v: None if v is None else round(v, 1)
    wins = {}
    for t, j in tb:
        wins.setdefault(j.get("win", 0), []).append((t, j))
    if 0 not in wins: raise SystemExit(f"{name}/{n}w: 沒有主視窗的 TICKBENCH")
    base = [j for t, j in wins[0] if t < t_start]
    if not base: raise SystemExit(f"{name}/{n}w: 沒有開始前的 TICKBENCH 行")
    recv = recv_of(name, wins[0][-1][1]) - recv_of(name, base[-1])
    per = {}; pooled = []
    for k in range(n):
        lines = wins.get(k, [])
        samples = [s for _, j in lines for s in j["lat"] if s[1] >= start_us and s[3] <= end_us]
        oldest = [(s[6] - s[1]) / 1000 for s in samples]
        pooled += oldest
        fps = [j["fps"] for t, j in lines if t_start + 1 <= t <= t_end]
        per[k] = {"fps": r(statistics.median(fps)) if fps else None, "fpsMin": min(fps) if fps else None,
                  "p50": r(pct(oldest, 50)), "p99": r(pct(oldest, 99)), "max": r(max(oldest) if oldest else None),
                  "n": len(oldest)}
    cpu, cpu_x, top = cpu_split(th0[0], th1[0], secs, th0[1], th1[1])
    gcpu, _, _ = cpu_split(g0, g1, secs)
    fpsv = [per[k]["fps"] for k in per if per[k]["fps"] is not None]
    return {
        "app": name, "windows": n, "secs": round(secs, 2), "scale": SCALE, "procs": procs,
        "gen": {"sent": sent, "cpu": gcpu, "maxDue": j_end["maxDue"], "behindFrames": j_end["behindFrames"]},
        "ticks": {"sent": sent, "recv": recv, "loss": sent - recv},
        "lat": {"p50": r(pct(pooled, 50)), "p99": r(pct(pooled, 99)), "max": r(max(pooled) if pooled else None), "n": len(pooled)},
        "perWindow": per,
        "fpsMin": min(fpsv) if fpsv else None, "fpsMean": r(sum(fpsv) / len(fpsv)) if fpsv else None,
        "worstP99": max((per[k]["p99"] for k in per if per[k]["p99"] is not None), default=None),
        "cpu": cpu, "cpuExLlvmpipe": cpu_x, "cpuTop": top,
        "mem": {"start": mems[0][1], "end": mems[-1][1], "tail": m_tail,
                "peakRss": max(m["rss_mb"] for _, m in mems), "peakPss": max(m["pss_mb"] for _, m in mems),
                "series": [[t, m["rss_mb"], m["pss_mb"]] for t, m in mems]},
        "shot": shot,
    }

def med(vals):
    v = [x for x in vals if x is not None]
    return round(statistics.median(v), 1) if v else None

def summarize(results):
    out = {}
    for name in APPS_SEL:
        for n in WINS:
            rs = [x for x in results if x["app"] == name and x["windows"] == n]
            if not rs: continue
            g = lambda f: med([f(x) for x in rs])
            out[f"{name}/{n}w"] = {
                "runs": len(rs), "sent": g(lambda x: x["ticks"]["sent"]), "loss": g(lambda x: x["ticks"]["loss"]),
                "lat_p50": g(lambda x: x["lat"]["p50"]), "lat_p99": g(lambda x: x["lat"]["p99"]),
                "lat_max": g(lambda x: x["lat"]["max"]), "worst_window_p99": g(lambda x: x["worstP99"]),
                "fps_per_window": [g(lambda x, k=k: (x["perWindow"].get(str(k)) or x["perWindow"].get(k) or {}).get("fps")) for k in range(n)],
                "fps_min_window": g(lambda x: x["fpsMin"]), "fps_mean": g(lambda x: x["fpsMean"]),
                "cpu": g(lambda x: x["cpu"]), "cpuExLlvmpipe": g(lambda x: x["cpuExLlvmpipe"]),
                "rssEnd": g(lambda x: x["mem"]["end"]["rss_mb"]), "pssEnd": g(lambda x: x["mem"]["end"]["pss_mb"]),
                "rssPeak": g(lambda x: x["mem"]["peakRss"]), "pssPeak": g(lambda x: x["mem"]["peakPss"]),
                "rssStart": g(lambda x: x["mem"]["start"]["rss_mb"]), "pssStart": g(lambda x: x["mem"]["start"]["pss_mb"]),
                "procs": rs[-1]["procs"],
            }
    return out

def main():
    for p in os.listdir("/proc"):
        if p.isdigit() and comm(int(p)) in ("wry-xq-demo", "gpui-xq-demo", "egui-xq-demo", "xq-tickgen"):
            raise SystemExit(f"有程式在跑（pid {p} {comm(int(p))}），先關掉再量")
    import socket
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    host, port = ADDR.rsplit(":", 1)
    try: s.bind((host, int(port)))
    except OSError: raise SystemExit(f"{ADDR} 被佔用（先停 WryFeedHost）")
    s.close()
    path = f"{OUT}/multi-results.json"
    results = json.load(open(path)) if os.environ.get("RESUME") and os.path.exists(path) else []
    for n in WINS:
        for i in range(RUNS):
            for name in APPS_SEL:
                if sum(1 for x in results if x["app"] == name and x["windows"] == n) > i: continue
                shot = i == RUNS - 1
                t = time.strftime("%H:%M:%S")
                try: res = run_once(name, n, shot)
                except SystemExit as e:
                    print(f"  ！{name}/{n}w: {e}；重跑一次", flush=True); time.sleep(3)
                    res = run_once(name, n, shot)
                results.append(res)
                json.dump(results, open(path, "w"), ensure_ascii=False, indent=1)
                pw = " ".join(f"w{k}:{v['fps']}fps/{v['p50']}/{v['p99']}ms" for k, v in res["perWindow"].items())
                print(f"[{t}] {name:4s} {n}w #{i+1} sent={res['ticks']['sent']} loss={res['ticks']['loss']} "
                      f"lat p50/p99={res['lat']['p50']}/{res['lat']['p99']}ms cpu={res['cpu']}({res['cpuExLlvmpipe']}) "
                      f"rss/pss={res['mem']['end']['rss_mb']}/{res['mem']['end']['pss_mb']} {pw}", flush=True)
                time.sleep(1.0)
    summary = {"date": time.strftime("%Y-%m-%d %H:%M %z"), "runs": RUNS, "durationSec": DUR, "rate": 10000,
               "screen": os.environ.get("SCREEN", "3840x2160"), "scale": SCALE, "tile": [TW, TH],
               "medians": summarize(results)}
    json.dump(summary, open(f"{OUT}/multi-summary.json", "w"), ensure_ascii=False, indent=1)
    print(json.dumps(summary["medians"], ensure_ascii=False, indent=1))

main()
PY
