#!/usr/bin/env python3
"""design-tokens 分支驗證：截圖對照（main vs design-tokens）＋主視窗 FPS/RSS/CPU（交錯 N 輪取中位數）。
獨立的 Xvfb（DISPLAY=:73）與 tickgen 埠（47793），不碰別的工作。只殺自己啟動的 PID。
用法：tokbench.py shots OUTDIR [variant ...]   variant = name=bin_dir[:ENV=VAL,...]
      tokbench.py perf OUTDIR RUNS SECS variant ...
"""
import json, os, statistics, subprocess, sys, time, signal, socket

DISPLAY = ":73"
PORT = 47793
TICKGEN = "/workspace/tokens/bin/xq-tickgen"

def start_x():
    if os.path.exists(f"/tmp/.X11-unix/X{DISPLAY[1:]}"):
        return None
    p = subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "1280x800x24", "-nolisten", "tcp"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.5)
    return p

def start_feed(rate):
    args = [TICKGEN, "--addr", f"127.0.0.1:{PORT}", "--rate", str(rate), "--duration", "100000", "--rows", "0",
            "--settle", "0.5", "--warmup", "0.5", "--seed", "1"]
    p = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(100):
        try:
            socket.create_connection(("127.0.0.1", PORT), 0.2).close(); break
        except OSError: time.sleep(0.1)
    # 探測連線會被當成一個 client；重開一次確保乾淨
    p.terminate(); p.wait()
    p = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    return p

def parse_variant(s):
    name, rest = s.split("=", 1)
    path, _, envs = rest.partition(":")
    env = dict(kv.split("=", 1) for kv in envs.split(",") if kv)
    return name, path, env

def launch(path, env, log):
    e = dict(os.environ, DISPLAY=DISPLAY, XQ_FEED_ADDR=f"127.0.0.1:{PORT}", XQ_PERIOD="D", XQ_METRICS="1", XQ_BENCH="1",
             XQ_GPUI_STATE="/tmp/tok-gpui-state.json", XQ_EGUI_STATE="/tmp/tok-egui-state.json", XQ_RENDERER="glow")
    for k in ("XQ_TOKENS_DIR", "XQ_THEME", "XQ_TOKENS_WATCH"):
        e.pop(k, None)
    e.update(env)
    for f in ("/tmp/tok-gpui-state.json", "/tmp/tok-egui-state.json"):
        try: os.remove(f)
        except FileNotFoundError: pass
    return subprocess.Popen([path], env=e, stdout=log, stderr=subprocess.STDOUT, cwd=os.path.dirname(path))

def window_geom():
    r = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "XQ"], capture_output=True, text=True, env=dict(os.environ, DISPLAY=DISPLAY))
    for wid in r.stdout.split():
        g = subprocess.run(["xdotool", "getwindowgeometry", "--shell", wid], capture_output=True, text=True, env=dict(os.environ, DISPLAY=DISPLAY)).stdout
        d = dict(l.split("=", 1) for l in g.split() if "=" in l)
        if int(d.get("WIDTH", 0)) > 600:
            return int(d["X"]), int(d["Y"]), int(d["WIDTH"]), int(d["HEIGHT"])
    return None

def shot(path):
    g = window_geom()
    full = path + ".full.png"
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-video_size", "1280x800", "-i", DISPLAY, "-frames:v", "1", full], check=True)
    from PIL import Image
    im = Image.open(full)
    if g:
        x, y, w, h = g
        im = im.crop((x, y, x + w, y + h))
    im.save(path); os.remove(full)
    return g

def stop(p):
    if p and p.poll() is None:
        p.send_signal(signal.SIGTERM)
        try: p.wait(5)
        except subprocess.TimeoutExpired: p.kill(); p.wait()

def cpu_ticks(pid):
    try:
        f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
        return int(f[11]) + int(f[12])
    except Exception: return 0

def rss_mb(pid):
    for l in open(f"/proc/{pid}/status"):
        if l.startswith("VmRSS:"): return int(l.split()[1]) / 1024
    return 0.0

def shots(out, variants):
    os.makedirs(out, exist_ok=True)
    x = start_x()
    try:
        for v in variants:
            name, path, env = parse_variant(v)
            feed = start_feed(0)  # 只送一次快照：畫面固定，可以逐像素比
            log = open(f"{out}/{name}.log", "w")
            p = launch(path, env, log)
            time.sleep(float(env.get("_WAIT", "8")))
            for k, cmd in enumerate(env.get("_KEYS", "").split("+") if env.get("_KEYS") else []):
                # 沒有視窗管理員：先把焦點給視窗（egui 沒焦點時收不到鍵盤），滑鼠移到中間點一下空白處不會改狀態，所以只 focus
                xe = dict(os.environ, DISPLAY=DISPLAY)
                wid = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "XQ"], capture_output=True, text=True, env=xe).stdout.split()
                if wid: subprocess.run(["xdotool", "windowfocus", "--sync", wid[0]], env=xe)
                time.sleep(0.3)
                subprocess.run(["xdotool", "key", cmd], env=xe); time.sleep(1.5)
            if env.get("_HOT"):
                # 熱更新測試：程式跑著的時候把改過的 token 檔蓋上去，等 watcher（300ms 輪詢）重載
                src, dst = env["_HOT"].split("@")
                import shutil; shutil.copyfile(src, dst); time.sleep(2.5)
            g = shot(f"{out}/{name}.png")
            print(name, "window", g, flush=True)
            stop(p); stop(feed); log.close(); time.sleep(0.5)
    finally:
        stop(x)

def perf(out, runs, secs, variants):
    os.makedirs(out, exist_ok=True)
    x = start_x()
    res = {parse_variant(v)[0]: [] for v in variants}
    clk = os.sysconf("SC_CLK_TCK")
    try:
        for r in range(runs):
            for v in variants:
                name, path, env = parse_variant(v)
                feed = start_feed(2000)
                log = open(f"{out}/{name}-run{r+1}.log", "w")
                p = launch(path, env, log)
                time.sleep(10)
                c0, t0 = cpu_ticks(p.pid), time.monotonic()
                time.sleep(secs)
                c1, t1 = cpu_ticks(p.pid), time.monotonic()
                rss = rss_mb(p.pid)
                stop(p); stop(feed); log.close()
                fps, paint = [], []
                for l in open(f"{out}/{name}-run{r+1}.log", errors="replace"):
                    if l.startswith("[metrics]"):
                        d = dict(kv.split("=", 1) for kv in l.split()[1:] if "=" in kv)
                        try: fps.append(float(d["fps"])); paint.append(float(d.get("chart_paint_us_avg", "nan")))
                        except Exception: pass
                # 只取量測窗內（第 11 秒起）的取樣
                fps_win = fps[10:10 + int(secs)] if len(fps) > 10 else fps
                paint_win = [x for x in (paint[10:10 + int(secs)] if len(paint) > 10 else paint) if x == x and x > 0]
                cpu = (c1 - c0) / clk / (t1 - t0) * 100
                fa = statistics.mean(fps_win) if fps_win else None
                row = {"fps_avg": round(fa, 1) if fa else None, "rss_mb": round(rss, 1), "cpu_pct": round(cpu, 1),
                       "cpu_ms_per_frame": round(cpu * 10 / fa, 2) if fa else None,
                       "chart_paint_us": round(statistics.median(paint_win)) if paint_win else None, "fps_samples": len(fps_win)}
                res[name].append(row)
                print(name, r + 1, row, flush=True)
                time.sleep(1)
    finally:
        stop(x)
    summ = {}
    for name, rows in res.items():
        summ[name] = {k: statistics.median([x[k] for x in rows if x.get(k) is not None]) for k in ("fps_avg", "rss_mb", "cpu_pct", "cpu_ms_per_frame", "chart_paint_us") if any(x.get(k) is not None for x in rows)}
    json.dump({"runs": res, "median": summ, "secs": secs, "tick_rate": 2000, "load_note": open("/proc/loadavg").read().strip()},
              open(f"{out}/perf.json", "w"), ensure_ascii=False, indent=1)
    print(json.dumps(summ, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    mode, out = sys.argv[1], sys.argv[2]
    if mode == "shots": shots(out, sys.argv[3:])
    else: perf(out, int(sys.argv[3]), float(sys.argv[4]), sys.argv[5:])
