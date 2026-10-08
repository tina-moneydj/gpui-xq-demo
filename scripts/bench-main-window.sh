#!/usr/bin/env bash
# 主視窗成本比較：wry-xq-demo（tao + wry / WebKitGTK）vs gpui-xq-demo（GPUI 原生繪圖）
#
# 兩邊同畫面：一個主視窗 1200×720、同一份 33 檔自選、單一走勢圖分頁（2330 日線＋MA5/20/60＋成交量）、
# 右下空白／資訊格（wry 不建子 WebView）、不開壓力 5 萬。兩個程式輪流跑（WryFeedHost 一次只服務一個 client）。
#
# 量測：
#   啟動：launch → 視窗出現（xdotool 輪詢）→ 第一幀畫出（BENCH first-frame）→ 第一筆報價畫出（BENCH first-quote）
#         後兩者由程式在 XQ_BENCH=1 時印到 stderr，本腳本收到時打時間戳。
#   記憶體：主行程＋所有子行程（wry：WebKitWebProcess、WebKitNetworkProcess）的 RSS 與 PSS（/proc/*/smaps_rollup），
#           t=10s、t=60s 各取一次。
#   CPU：t=30s→60s 所有行程 utime+stime 加總 ÷ 30s（100% = 一顆核心）。
#   FPS：程式自己報的數字（wry：狀態列 rAF 計幀；GPUI：根 view 每秒 render 次數），取 t=30–60s 平均。
#   截圖：t≈60s（最後一輪）存到 $OUT/<app>.png
#
# 壓力情境（mode=stress）：同上畫面，再開 5 萬列合成表（6 欄）＋每秒 3000 筆合成跳價（30 Hz 分批、隨機列），
#   先只跳價 30 秒、再邊跳價邊上下正弦捲動 10 秒（wry：XQ_GROUP_STRESS 的 __benchGroupWatchlist；GPUI：XQ_GROUP_STRESS）。
#   量 RSS/PSS（跳價末、捲動中）、CPU（跳價 30 秒、捲動 10 秒）、程式回報的 FPS／可見格更新耗時（GROUPPERF|{json}）。
#
# 用法：bench-main-window.sh [runs=3] [mode=all|baseline|stress]
#   需求：DISPLAY（預設 :2）、xdotool、ffmpeg、WryFeedHost --demo-ticks 在 127.0.0.1:47631、兩個 release binary 已建好、
#         兩個程式都沒有在跑（否則會互搶 feed）。wry 的 state.json 會先備份、跑完還原。
set -euo pipefail
RUNS="${1:-3}"
MODE="${2:-all}"
export DISPLAY="${DISPLAY:-:2}"
WRY_DIR="${WRY_DIR:-/workspace/wry-xq-demo}"
GPUI_DIR="${GPUI_DIR:-/workspace/gpui-xq-demo}"
OUT="${OUT:-/workspace/bench}"
mkdir -p "$OUT"
export RUNS MODE WRY_DIR GPUI_DIR OUT
exec python3 - <<'PY'
import json, os, shutil, signal, statistics, subprocess, sys, threading, time

RUNS = int(os.environ["RUNS"]); OUT = os.environ["OUT"]; MODE = os.environ["MODE"]
WRY_DIR = os.environ["WRY_DIR"]; GPUI_DIR = os.environ["GPUI_DIR"]
W, H = 1200, 720  # 1280×800 Xvfb 扣掉 xfwm4 標題列後放得下的尺寸
T_IDLE, T_CPU0, T_END = 10.0, 30.0, 60.0
CLK = os.sysconf("SC_CLK_TCK")
STATE = os.path.expanduser("~/.local/share/wry-xq-demo/state.json")

# 與 gpui-xq-demo/src/names.rs 相同的自選清單
WATCH = ["2330","2317","2454","0050","2303","2881","2882","2308","2412","2382","3711","2891","2886","1301","1303",
         "2002","3008","2357","2603","2609","3231","6505","2884","2892","5880","2207","1216","2379","3034","2345",
         "AAPL","TSLA","NVDA"]

def wry_bench_state():
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
        # 左下用「組合」分頁（虛擬化表格，6 欄同 GPUI：代號／名稱／成交／漲跌／幅度／總量）
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
    "wry": dict(cwd=WRY_DIR, cmd=["./target/release/wry-xq-demo"], title=r"^XQ .*tao \+ wry",
                env={"WEBKIT_DISABLE_COMPOSITING_MODE": "1", "XQ_BENCH": "1"}),
    "gpui": dict(cwd=GPUI_DIR, cmd=["./target/release/gpui-xq-demo"], title="^XQ .*GPUI",
                 env={"XQ_BENCH": "1", "XQ_PERIOD": "D", "XQ_METRICS": "1"}),
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
    r = {"rss": 0, "pss": 0, "threads": 0}
    try:
        for l in open(f"/proc/{p}/status"):
            if l.startswith("VmRSS:"): r["rss"] = int(l.split()[1])
            elif l.startswith("Threads:"): r["threads"] = int(l.split()[1])
        for l in open(f"/proc/{p}/smaps_rollup"):
            if l.startswith("Pss:"): r["pss"] = int(l.split()[1])
    except Exception: pass
    return r

def cpu_ticks(p):
    try:
        f = open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()
        return int(f[11]) + int(f[12])
    except Exception: return 0

def thread_ticks(pids):
    """每個執行緒名稱（去掉尾端數字）的 CPU ticks，看 CPU 花在哪（例如 lavapipe 光柵化執行緒）"""
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

def thread_breakdown(a, b, secs, top=6):
    d = {k: (b.get(k, 0) - a.get(k, 0)) / CLK / secs * 100 for k in b}
    return {k: round(v, 1) for k, v in sorted(d.items(), key=lambda kv: -kv[1])[:top] if v >= 1}

def snapshot(root):
    procs = []
    for p in tree(root):
        m = mem(p); m.update(pid=p, comm=comm(p)); procs.append(m)
    return {"rss_mb": sum(x["rss"] for x in procs) / 1024, "pss_mb": sum(x["pss"] for x in procs) / 1024,
            "threads": sum(x["threads"] for x in procs), "nproc": len(procs),
            "procs": [{"comm": x["comm"], "rss_mb": round(x["rss"] / 1024, 1), "pss_mb": round(x["pss"] / 1024, 1),
                       "threads": x["threads"]} for x in procs]}

def find_window(title):
    r = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", title], capture_output=True, text=True)
    ids = r.stdout.split()
    return ids[0] if ids else None

def screenshot(wid, path):
    # 整個螢幕（視窗已移到左上角）；xfwm4 重新掛載後 getwindowgeometry 的座標不可靠
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-i", os.environ["DISPLAY"],
                    "-frames:v", "1", path], check=True)
    g = subprocess.run(["xdotool", "getwindowgeometry", "--shell", wid], capture_output=True, text=True).stdout
    d = dict(l.split("=", 1) for l in g.split() if "=" in l)
    return int(d.get("WIDTH", 0)), int(d.get("HEIGHT", 0))

STRESS_ENV = {"XQ_GROUP_STRESS": "1", "XQ_STRESS_COLS": "6", "XQ_STRESS_TICK_HZ": "3000",
              "XQ_STRESS_TICK_MS": "30000", "XQ_STRESS_SCROLL_MS": "10000"}

def run_once(name, shot, stress=False):
    app = APPS[name]
    if name == "wry":
        json.dump(wry_bench_state(), open(STATE, "w"), ensure_ascii=False, indent=2)
    env = dict(os.environ, **app["env"], **(STRESS_ENV if stress else {}))
    events = {}; fps = []; log = []
    t0 = time.monotonic()
    proc = subprocess.Popen(app["cmd"], cwd=app["cwd"], env=env, stdin=subprocess.DEVNULL,
                            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, bufsize=1)
    def reader():
        for line in proc.stderr:
            t = time.monotonic() - t0; log.append(f"{t:8.3f} {line.rstrip()}")
            for ev in ("first-frame", "first-quote"):
                if f"BENCH {ev}" in line and ev not in events: events[ev] = t
            if "GROUPPERF|{" in line:
                try:
                    j = json.loads(line.split("GROUPPERF|", 1)[1])
                    events["gp:" + j.get("phase", "?")] = (t, j)
                except Exception: pass
            if "BENCH fps" in line or "[metrics] fps=" in line:
                try:
                    v = float(line.split("fps", 1)[1].strip(" =").split()[0])
                    fps.append((t, v))
                except Exception: pass
    threading.Thread(target=reader, daemon=True).start()
    try:
        return (measure_stress if stress else measure)(name, app, proc, t0, events, fps, log, shot)
    except BaseException:
        for p in reversed(tree(proc.pid)):
            try: os.kill(p, signal.SIGKILL)
            except ProcessLookupError: pass
        raise

def open_window(name, app, t0, events):
    wid = None
    while time.monotonic() - t0 < 20:
        wid = find_window(app["title"])
        if wid: events["window"] = time.monotonic() - t0; break
        time.sleep(0.02)
    if not wid: raise SystemExit(f"{name}: window not found")
    subprocess.run(["xdotool", "windowmove", "--sync", wid, "0", "0"])
    subprocess.run(["xdotool", "windowsize", "--sync", wid, str(W), str(H)])
    return wid

def stop(proc, name, log):
    # 結束：只殺自己啟動的 pid，再確認子行程跟著結束
    kids = tree(proc.pid)
    proc.send_signal(signal.SIGTERM)
    try: proc.wait(10)
    except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    alive = []
    for _ in range(50):
        alive = [p for p in kids if os.path.exists(f"/proc/{p}") and ppid(p) != -1 and p != proc.pid]
        if not alive: break
        time.sleep(0.1)
    for p in alive:
        try: os.kill(p, signal.SIGTERM)
        except ProcessLookupError: pass
    open(f"{OUT}/{name}-run.log", "a").write("\n".join(log) + "\n----\n")

def measure_stress(name, app, proc, t0, events, fps, log, shot):
    wid = open_window(name, app, t0, events)
    def wait_ev(key, timeout):
        end = time.monotonic() + timeout
        while key not in events:
            if time.monotonic() > end: raise SystemExit(f"{name}: 等不到 {key}")
            time.sleep(0.02)
        return events[key]
    wait_ev("gp:tick-start", 20)
    pids0 = tree(proc.pid); c0 = sum(cpu_ticks(p) for p in pids0); th0 = thread_ticks(pids0); ta = time.monotonic()
    shot_path = None
    if shot:
        while time.monotonic() - ta < 25: time.sleep(0.05)
        screenshot(wid, f"{OUT}/{name}-stress.png"); shot_path = f"{OUT}/{name}-stress.png"
    _, tick_end = wait_ev("gp:tick-end", 60)
    pids1 = tree(proc.pid); c1 = sum(cpu_ticks(p) for p in set(pids0) | set(pids1)); th1 = thread_ticks(pids1); tb = time.monotonic()
    mem_tick = snapshot(proc.pid)
    while time.monotonic() - tb < 8: time.sleep(0.05)
    mem_scroll = snapshot(proc.pid)
    _, done = wait_ev("gp:done", 30)
    pids2 = tree(proc.pid); c2 = sum(cpu_ticks(p) for p in set(pids1) | set(pids2)); tc = time.monotonic()
    # 捲動段的結束時間以收到 done 為準（最多晚 20ms 輪詢）
    stop(proc, name + "-stress", log)
    return {"app": name, "stress": True, "tick_end": tick_end, "done": done,
            "cpu_tick_pct": (c1 - c0) / CLK / (tb - ta) * 100, "cpu_tick_threads": thread_breakdown(th0, th1, tb - ta),
            "cpu_scroll_pct": (c2 - c1) / CLK / (tc - tb) * 100,
            "mem_tick": mem_tick, "mem_scroll": mem_scroll, "screenshot": shot_path}

def measure(name, app, proc, t0, events, fps, log, shot):
    wid = open_window(name, app, t0, events)
    def wait_until(t):
        while time.monotonic() - t0 < t: time.sleep(0.05)
    wait_until(T_IDLE); idle = snapshot(proc.pid)
    wait_until(T_CPU0)
    pids0 = tree(proc.pid); c0 = sum(cpu_ticks(p) for p in pids0); th0 = thread_ticks(pids0); tc0 = time.monotonic()
    wait_until(T_END)
    pids1 = tree(proc.pid); c1 = sum(cpu_ticks(p) for p in set(pids0) | set(pids1)); tc1 = time.monotonic()
    th1 = thread_ticks(pids1)
    steady = snapshot(proc.pid)
    geom = screenshot(wid, f"{OUT}/{name}.png") if shot else None
    cpu = (c1 - c0) / CLK / (tc1 - tc0) * 100
    fps_win = [v for t, v in fps if T_CPU0 <= t <= T_END]
    stop(proc, name, log)
    return {"app": name, "window_s": events.get("window"), "first_frame_s": events.get("first-frame"),
            "first_quote_s": events.get("first-quote"), "idle10": idle, "steady60": steady,
            "cpu_threads": thread_breakdown(th0, th1, tc1 - tc0), "cpu_pct": cpu, "fps_avg": statistics.mean(fps_win) if fps_win else None, "geom": geom}

def main():
    for name, app in APPS.items():
        if subprocess.run(["pgrep", "-f", "-x", ".*" + app["cmd"][0].split("/")[-1] + "$"],
                          capture_output=True).returncode == 0:
            raise SystemExit(f"{name} 已在執行，請先關掉（會互搶 feed）")
    backup = STATE + ".bench-backup"
    had_state = os.path.exists(STATE)
    if had_state: shutil.copy2(STATE, backup)
    for f in ("wry-run.log", "gpui-run.log", "wry-stress-run.log", "gpui-stress-run.log"):
        try: os.remove(f"{OUT}/{f}")
        except FileNotFoundError: pass
    results = []; stress = []
    try:
        if MODE in ("all", "baseline"):
            for i in range(RUNS):
                for name in ("wry", "gpui"):
                    print(f"baseline {i+1}/{RUNS} {name} …", flush=True)
                    r = run_once(name, shot=(i == RUNS - 1)); r["run"] = i + 1; results.append(r)
                    print(f"  window={r['window_s']:.3f}s frame={r['first_frame_s']} quote={r['first_quote_s']} "
                          f"rss10={r['idle10']['rss_mb']:.1f} rss60={r['steady60']['rss_mb']:.1f} "
                          f"pss60={r['steady60']['pss_mb']:.1f} cpu={r['cpu_pct']:.1f}% fps={r['fps_avg']} "
                          f"threads={r['cpu_threads']}", flush=True)
                    time.sleep(3)
        if MODE in ("all", "stress"):
            for i in range(RUNS):
                for name in ("wry", "gpui"):
                    print(f"stress {i+1}/{RUNS} {name} …", flush=True)
                    r = run_once(name, shot=(i == RUNS - 1), stress=True); r["run"] = i + 1; stress.append(r)
                    print(f"  cpu_tick={r['cpu_tick_pct']:.1f}% cpu_scroll={r['cpu_scroll_pct']:.1f}% "
                          f"rss_tick={r['mem_tick']['rss_mb']:.1f} pss_tick={r['mem_tick']['pss_mb']:.1f} "
                          f"rss_scroll={r['mem_scroll']['rss_mb']:.1f}\n  tick_end={r['tick_end']}\n  done={r['done']}\n"
                          f"  threads={r['cpu_tick_threads']}", flush=True)
                    time.sleep(3)
    finally:
        time.sleep(1)
        if had_state: shutil.copy2(backup, STATE); os.remove(backup)
    med = lambda xs: statistics.median([x for x in xs if x is not None]) if any(x is not None for x in xs) else None
    def bin_size(p): return os.path.getsize(p) / 1024 / 1024
    summary = {"date": time.strftime("%Y-%m-%d %H:%M %z"), "runs": RUNS, "mode": MODE}
    for name in ("wry", "gpui"):
        out = {"binary_mb": bin_size(os.path.join(APPS[name]["cwd"], APPS[name]["cmd"][0]))}
        rs = [r for r in results if r["app"] == name]
        if rs:
            out["baseline"] = {
                "window_ms": med([r["window_s"] * 1000 for r in rs]),
                "first_frame_ms": med([r["first_frame_s"] * 1000 if r["first_frame_s"] else None for r in rs]),
                "first_quote_ms": med([r["first_quote_s"] * 1000 if r["first_quote_s"] else None for r in rs]),
                "rss10_mb": med([r["idle10"]["rss_mb"] for r in rs]), "pss10_mb": med([r["idle10"]["pss_mb"] for r in rs]),
                "rss60_mb": med([r["steady60"]["rss_mb"] for r in rs]), "pss60_mb": med([r["steady60"]["pss_mb"] for r in rs]),
                "cpu_pct": med([r["cpu_pct"] for r in rs]), "fps": med([r["fps_avg"] for r in rs]),
                "nproc": med([r["steady60"]["nproc"] for r in rs]), "threads": med([r["steady60"]["threads"] for r in rs]),
                "procs60_last": rs[-1]["steady60"]["procs"], "cpu_threads_last": rs[-1]["cpu_threads"],
            }
        ss = [r for r in stress if r["app"] == name]
        if ss:
            g = lambda ph, k: med([r[ph].get(k) for r in ss])
            out["stress"] = {
                "rss_tick_mb": med([r["mem_tick"]["rss_mb"] for r in ss]), "pss_tick_mb": med([r["mem_tick"]["pss_mb"] for r in ss]),
                "rss_scroll_mb": med([r["mem_scroll"]["rss_mb"] for r in ss]), "pss_scroll_mb": med([r["mem_scroll"]["pss_mb"] for r in ss]),
                "cpu_tick_pct": med([r["cpu_tick_pct"] for r in ss]), "cpu_scroll_pct": med([r["cpu_scroll_pct"] for r in ss]),
                "tick_fps": g("tick_end", "fps"), "scroll_fps": g("done", "scrollFps"),
                "tick_avg_paint_ms": g("tick_end", "avgPaintMs"), "tick_cells_per_paint": g("tick_end", "avgCellsPerPaint"),
                "tick_paints": g("tick_end", "paints"),
                "scroll_avg_paint_ms": g("done", "avgPaintMs"), "scroll_cells_per_paint": g("done", "avgCellsPerPaint"),
                "scroll_paints": g("done", "paints"),
                "scroll_jank_count": g("done", "scrollJankCount"), "scroll_jank_avg_ms": g("done", "scrollJankAvgMs"),
                "rows": ss[-1]["done"].get("rows"), "cols": ss[-1]["done"].get("cols"),
                "cpu_tick_threads_last": ss[-1]["cpu_tick_threads"],
            }
        summary[name] = out
    json.dump({"baseline": results, "stress": stress}, open(f"{OUT}/results.json", "w"), ensure_ascii=False, indent=1)
    json.dump(summary, open(f"{OUT}/summary.json", "w"), ensure_ascii=False, indent=1)
    print(json.dumps(summary, ensure_ascii=False, indent=1))

main()
PY
