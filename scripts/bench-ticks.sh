#!/usr/bin/env bash
# 高頻進價壓測：wry-xq-demo（tao + wry / WebKitGTK）vs gpui-xq-demo（GPUI 原生繪圖），2026-10-08
#
# 行情來源：gpui-xq-demo/crates/xq-feed 的 xq-tickgen（同 WryFeedHost 協議、同 127.0.0.1:47631），
#   每 5ms 一包、不合併；每包第一筆是走勢圖那檔 2330（探針），其餘 20% 落在 33 檔自選、80% 均勻落在 Z00000–Z49999。
#   每包尾端附 `XQTS + seq + sent_us`，程式在 XQ_TICK_BENCH=1 時才解析（預設關閉）。
# 畫面：兩邊同一個主視窗 1200×720；上＝2330 日線（MA5/20/60＋量）；左下＝5 萬 33 列報價表（33 檔自選＋5 萬 Z 列，6 欄），
#   engine 報價會對到每一列（wry：組合分頁暫時表格；GPUI：QuoteTable 5 萬列＋索引）。
# 情境：穩定 1k / 10k / 50k / 100k 筆/秒，以及開盤爆量 burst（rate = 5k + 150k·e^(−t/3s)，前 3 秒約 30 萬筆）。
#   每輪：重開產生器 → 開程式 → 等訂閱 3 秒 → 送 50,033 檔快照 → 暖身 3 秒 → 量 30 秒 → 收尾 4 秒；一次只跑一個程式。
# 量測（每輪）：
#   延遲 tick→畫面：present − sent。每個畫面幀取「這幀套用的最舊一包」（oldest）與「最新一包」（newest）；
#     present = 套用後下一個 frame 回呼（GPUI：window.on_next_frame；wry：兩次 rAF），是上界（最多多約 1 幀）。
#     兩邊時鐘都是同一台機器的 CLOCK_REALTIME（Rust SystemTime；JS performance.timeOrigin+now，WebKit 精度 1ms）。
#   積壓：前 5 秒 vs 最後 5 秒的延遲中位數；收尾時已收到的筆數是否等於送出（loss = 送出 − 收到，TCP 不會掉，看的是卡住沒讀）。
#   合併：收到（未合併）→ 交給 UI（合併後）→ 表格實際更新列數，差額是刻意合併（同代號只留最新），不是遺失。
#   CPU：程式整棵行程樹（wry 含 WebKitWebProcess）utime+stime ÷ 30 秒；另列扣掉 llvmpipe（軟體光柵化）執行緒。
#   記憶體：RSS／PSS（smaps_rollup）量測開始與結束、成長。FPS：程式自己每秒回報（wry＝rAF 次數；GPUI＝root render 次數）。
#   回應性：量測中每 6 秒用 xdotool 點報價表 2317 列、1 秒後點 2330 列，click → 切換後第一幀畫完（同上 present 定義）。
# 輸出：$OUT/ticks-results.json（每輪原始數據）、$OUT/ticks-summary.json（3 輪中位數）、$OUT/tick-<app>-run.log、截圖。
# 用法：bench-ticks.sh [runs=3] [scenarios=s1k,s10k,s50k,s100k,burst] [apps=gpui,wry]
#   需求：DISPLAY（預設 :2）、xdotool、ffmpeg、兩個 release binary＋xq-tickgen 已建好；
#         127.0.0.1:47631 沒有被佔用（先停 WryFeedHost）、兩個程式都沒在跑。wry 用獨立 XDG_DATA_HOME，不碰使用者 state.json。
set -euo pipefail
RUNS="${1:-3}"
SCEN="${2:-s1k,s10k,s50k,s100k,burst}"
APPS_SEL="${3:-gpui,wry}"
export DISPLAY="${DISPLAY:-:2}"
WRY_DIR="${WRY_DIR:-/workspace/wry-xq-demo}"
GPUI_DIR="${GPUI_DIR:-/workspace/gpui-xq-demo}"
OUT="${OUT:-/workspace/bench/ticks}"
TICK_ADDR="${TICK_ADDR:-127.0.0.1:47631}"
WRY_BIN="${WRY_BIN:-./target/release/wry-xq-demo}"
mkdir -p "$OUT"
export RUNS SCEN APPS_SEL WRY_DIR GPUI_DIR OUT TICK_ADDR WRY_BIN
exec python3 - <<'PY'
import json, os, signal, statistics, subprocess, sys, threading, time

RUNS = int(os.environ["RUNS"]); OUT = os.environ["OUT"]
SCEN = os.environ["SCEN"].split(","); APPS_SEL = os.environ["APPS_SEL"].split(",")
WRY_DIR = os.environ["WRY_DIR"]; GPUI_DIR = os.environ["GPUI_DIR"]; ADDR = os.environ["TICK_ADDR"]
W, H = 1200, 720
CLK = os.sysconf("SC_CLK_TCK")
XDG = os.path.join(OUT, "xdg")  # wry 專用 XDG_DATA_HOME（bench 狀態），不碰使用者的 state.json
DUR, SETTLE, WARMUP, TAIL = 30.0, 3.0, 3.0, 4.0
SNAP_ROWS = 50033
TICKGEN = os.path.join(GPUI_DIR, "target/release/xq-tickgen")
SCENARIOS = {
    "s1k": ["--rate", "1000"], "s10k": ["--rate", "10000"], "s50k": ["--rate", "50000"],
    "s100k": ["--rate", "100000"], "burst": ["--burst", "150000,3,5000"],
}
WATCH = ["2330","2317","2454","0050","2303","2881","2882","2308","2412","2382","3711","2891","2886","1301","1303",
         "2002","3008","2357","2603","2609","3231","6505","2884","2892","5880","2207","1216","2379","3034","2345",
         "AAPL","TSLA","NVDA"]
# 報價表列的點擊座標（視窗內座標，1200×720；用截圖校正）：row0 = 2330、row1 = 2317
CLICK = {
    "gpui": {"2330": (30, int(os.environ.get("GPUI_ROW0_Y", "462"))), "2317": (30, int(os.environ.get("GPUI_ROW1_Y", "484")))},
    "wry": {"2330": (30, int(os.environ.get("WRY_ROW0_Y", "501"))), "2317": (30, int(os.environ.get("WRY_ROW1_Y", "524")))},
}

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
                     "XQ_FEED_ADDR": ADDR, "XDG_DATA_HOME": XDG}),
    "gpui": dict(cwd=GPUI_DIR, cmd=["./target/release/gpui-xq-demo"], title="^XQ .*GPUI",
                 env={"XQ_BENCH": "1", "XQ_TICK_BENCH": "1", "XQ_PERIOD": "D", "XQ_FEED_ADDR": ADDR}),
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

def cpu_split(a, b, secs):
    d = {k: b.get(k, 0) - a.get(k, 0) for k in b}
    tot = sum(d.values()) / CLK / secs * 100
    llvm = sum(v for k, v in d.items() if "llvmpipe" in k) / CLK / secs * 100
    top = {k: round(v / CLK / secs * 100, 1) for k, v in sorted(d.items(), key=lambda kv: -kv[1])[:6] if v / CLK / secs * 100 >= 1}
    return round(tot, 1), round(tot - llvm, 1), top

def find_window(title):
    r = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", title], capture_output=True, text=True)
    ids = r.stdout.split()
    return ids[0] if ids else None

def open_window(name, app, t0):
    wid = None
    while time.monotonic() - t0 < 20:
        wid = find_window(app["title"])
        if wid: break
        time.sleep(0.02)
    if not wid: raise SystemExit(f"{name}: window not found")
    subprocess.run(["xdotool", "windowmove", "--sync", wid, "0", "0"])
    subprocess.run(["xdotool", "windowsize", "--sync", wid, str(W), str(H)])
    return wid

def screenshot(path):
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-i", os.environ["DISPLAY"],
                    "-frames:v", "1", path], check=True)

def unix_us(): return int(time.time() * 1e6)

def pct(v, q):
    if not v: return None
    v = sorted(v); i = min(len(v) - 1, max(0, int(round(q / 100 * (len(v) - 1)))))
    return v[i]

def kill_tree(root):
    for p in reversed(tree(root)):
        try: os.kill(p, signal.SIGKILL)
        except ProcessLookupError: pass

def run_once(name, scen, shot):
    app = APPS[name]
    gev = []  # (mono, json)
    gen = subprocess.Popen([TICKGEN, "--addr", ADDR, *SCENARIOS[scen], "--duration", str(DUR), "--settle", str(SETTLE),
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
            if time.monotonic() > end: raise SystemExit(f"{name}/{scen}: 產生器等不到 {ev}: {gev[-3:]}")
            time.sleep(0.01)
    gwait("listen", 5)
    if name == "wry":
        os.makedirs(f"{XDG}/wry-xq-demo", exist_ok=True)
        json.dump(wry_bench_state(), open(f"{XDG}/wry-xq-demo/state.json", "w"), ensure_ascii=False, indent=2)
    env = dict(os.environ, **app["env"])
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
        wid = open_window(name, app, t0)
        t_start, j_start = gwait("start", 60)
        start_us = j_start["unixUs"]
        pids = tree(proc.pid)
        th0 = thread_ticks(pids); g0 = thread_ticks([gen.pid]); m0 = mem_tree(proc.pid)
        mems = [(0.0, m0)]
        clicks = []
        plan = []
        for k in range(int(DUR // 6)):
            plan += [(5 + 6 * k, "2317"), (6 + 6 * k, "2330")]
        plan = [p for p in plan if p[0] < DUR - 1]
        shot_path = None; next_mem = 2.0
        while True:
            el = time.monotonic() - t_start
            if any(j.get("ev") == "end" for _, j in gev) or el > DUR + 30: break
            if plan and el >= plan[0][0]:
                _, sym = plan.pop(0)
                x, y = CLICK[name][sym]
                if y > 0:
                    clicks.append((sym, unix_us()))
                    subprocess.run(["xdotool", "mousemove", "--window", wid, str(x), str(y), "click", "1"])
            if el >= next_mem:
                mems.append((round(el, 1), mem_tree(proc.pid))); next_mem += 2.0
            if shot and shot_path is None and el >= 15:
                shot_path = f"{OUT}/tick-{name}-{scen}.png"; screenshot(shot_path)
            time.sleep(0.02)
        t_end, j_end = gwait("end", 5)
        secs = t_end - t_start
        pids = tree(proc.pid)
        th1 = thread_ticks(pids); g1 = thread_ticks([gen.pid]); m1 = mem_tree(proc.pid)
        mems.append((round(secs, 1), m1))
        time.sleep(TAIL)
        m_tail = mem_tree(proc.pid)
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
        open(f"{OUT}/tick-{name}-run.log", "a").write(f"==== {scen} {time.strftime('%F %T')}\n" + "\n".join(log) + "\n")
    return analyze(name, scen, tb, gev, t_start, t_end, start_us, j_end, th0, th1, g0, g1, secs, mems, m_tail, clicks, shot_path)

def counters(name, j):
    if name == "gpui":
        return {"recv": j["rowsIn"], "toUi": j["inboxRows"], "applied": j["applied"], "batches": j["applies"]}
    return {"recv": j["raw"], "toUi": j["rows"], "applied": j["applied"], "batches": j["pushes"]}

def analyze(name, scen, tb, gev, t_start, t_end, start_us, j_end, th0, th1, g0, g1, secs, mems, m_tail, clicks, shot):
    end_us = j_end["unixUs"]
    sent = j_end["sent"]
    base = [j for t, j in tb if t < t_start]
    if not base: raise SystemExit(f"{name}/{scen}: 沒有開始前的 TICKBENCH 行")
    c0 = counters(name, base[-1])
    after_end = [j for t, j in tb if t >= t_end]
    c_end = counters(name, after_end[0]) if after_end else counters(name, tb[-1][1])
    c_fin = counters(name, tb[-1][1])
    d = lambda c, k: c[k] - c0[k]
    samples = [s for _, j in tb for s in j["lat"] if s[1] >= start_us and s[3] <= end_us]
    oldest = [(s[6] - s[1]) / 1000 for s in samples]
    newest = [(s[6] - s[3]) / 1000 for s in samples]
    transport = [(s[4] - s[3]) / 1000 for s in samples]
    to_apply = [(s[5] - s[4]) / 1000 for s in samples]
    to_present = [(s[6] - s[5]) / 1000 for s in samples]
    first5 = [(s[6] - s[1]) / 1000 for s in samples if s[1] < start_us + 5e6]
    last5 = [(s[6] - s[1]) / 1000 for s in samples if s[1] >= end_us - 5e6]
    presents = sorted(s[6] for s in samples)
    gaps = [(b - a) / 1000 for a, b in zip(presents, presents[1:])]
    # 窗口內最後一個樣本離結束多久（UI 卡住時樣本會停）
    stall_end = (end_us - presents[-1]) / 1000 if presents else None
    fps = [j["fps"] for t, j in tb if t_start + 1 <= t <= t_end]
    cpu, cpu_x, top = cpu_split(th0, th1, secs)
    gcpu, _, _ = cpu_split(g0, g1, secs)
    sw = [s for _, j in tb for s in j["sw"]]
    sw_raw = [list(x) for x in sw]
    sw_lat = []; sw_dispatch = []; missed = 0
    for k, (sym, cu) in enumerate(clicks):
        nxt = clicks[k + 1][1] if k + 1 < len(clicks) else float("inf")
        m = next((s for s in sw if s[0] == sym and cu - 2000 <= s[1] < nxt), None)
        if m is None: missed += 1; continue
        sw.remove(m)
        sw_lat.append((m[2] - cu) / 1000); sw_dispatch.append((m[1] - cu) / 1000)
    r = lambda v: None if v is None else round(v, 1)
    ps = lambda v: {"p50": r(pct(v, 50)), "p95": r(pct(v, 95)), "p99": r(pct(v, 99)), "max": r(max(v) if v else None), "n": len(v)}
    gsec = [j for _, j in gev if j.get("ev") == "sec"]
    mem_peak = max((m["rss_mb"] for _, m in mems), default=None)
    res = {
        "app": name, "scenario": scen, "secs": round(secs, 2),
        "gen": {"sent": sent, "target": j_end["target"], "frames": j_end["frames"], "bytes": j_end["bytes"],
                "maxDue": j_end["maxDue"], "behindFrames": j_end["behindFrames"], "maxWriteMs": j_end["maxWriteMs"],
                "cpu": gcpu, "perSec": [g["secSent"] for g in gsec]},
        "latOldest": ps(oldest), "latNewest": ps(newest),
        "breakdown_p50": {"transport": r(pct(transport, 50)), "recvToApply": r(pct(to_apply, 50)),
                          "applyToPresent": r(pct(to_present, 50))},
        "breakdown_p99": {"transport": r(pct(transport, 99)), "recvToApply": r(pct(to_apply, 99)),
                          "applyToPresent": r(pct(to_present, 99))},
        "trend": {"first5_p50": r(pct(first5, 50)), "last5_p50": r(pct(last5, 50)),
                  "first5_max": r(max(first5) if first5 else None), "last5_max": r(max(last5) if last5 else None)},
        "maxSampleGapMs": r(max(gaps) if gaps else None), "stallAtEndMs": r(stall_end),
        "ticks": {"sent": sent, "recvAtEnd": d(c_end, "recv"), "recvFinal": d(c_fin, "recv"),
                  "toUi": d(c_fin, "toUi"), "applied": d(c_fin, "applied"), "batches": d(c_fin, "batches"),
                  "lossFinal": sent - d(c_fin, "recv"), "backlogAtEnd": sent - d(c_end, "recv")},
        "cpu": cpu, "cpuExLlvmpipe": cpu_x, "cpuTop": top,
        "mem": {"start": mems[0][1], "end": mems[-1][1], "tail": m_tail, "peakRss": mem_peak,
                "rssGrowth": round(mems[-1][1]["rss_mb"] - mems[0][1]["rss_mb"], 1),
                "pssGrowth": round(mems[-1][1]["pss_mb"] - mems[0][1]["pss_mb"], 1),
                "series": [[t, m["rss_mb"], m["pss_mb"]] for t, m in mems]},
        "fps": r(statistics.median(fps)) if fps else None, "fpsMin": min(fps) if fps else None,
        "switch": {"p50": r(pct(sw_lat, 50)), "max": r(max(sw_lat) if sw_lat else None), "n": len(sw_lat), "missed": missed,
                   "dispatch_p50": r(pct(sw_dispatch, 50)), "all": [r(x) for x in sw_lat],
                   "clicks": clicks, "raw": sw_raw},
        "shot": shot,
    }
    if name == "wry":
        res["wry"] = {"evaluates": d(c_fin, "batches"), "nev": tb[-1][1]["nev"] - base[-1]["nev"],
                      "pushMs": round(tb[-1][1]["pushMs"] - base[-1]["pushMs"], 1), "quotesLen": tb[-1][1]["quotes"],
                      "evalToApply_p50": r(pct([(s[5] - s[7]) / 1000 for s in samples if len(s) > 7 and s[7]], 50)),
                      "recvToEval_p50": r(pct([(s[7] - s[4]) / 1000 for s in samples if len(s) > 7 and s[7]], 50))}
    else:
        res["gpui"] = {"applyUs": tb[-1][1]["applyUs"] - base[-1]["applyUs"]}
    return res

def med(vals):
    v = [x for x in vals if x is not None]
    return round(statistics.median(v), 1) if v else None

def summarize(results):
    out = {}
    for name in APPS_SEL:
        for scen in SCEN:
            rs = [x for x in results if x["app"] == name and x["scenario"] == scen]
            if not rs: continue
            g = lambda f: med([f(x) for x in rs])
            out[f"{name}/{scen}"] = {
                "runs": len(rs),
                "sent": g(lambda x: x["ticks"]["sent"]),
                "lat_oldest_p50": g(lambda x: x["latOldest"]["p50"]), "lat_oldest_p95": g(lambda x: x["latOldest"]["p95"]),
                "lat_oldest_p99": g(lambda x: x["latOldest"]["p99"]), "lat_oldest_max": g(lambda x: x["latOldest"]["max"]),
                "lat_newest_p50": g(lambda x: x["latNewest"]["p50"]), "lat_newest_p99": g(lambda x: x["latNewest"]["p99"]),
                "samples": g(lambda x: x["latOldest"]["n"]),
                "first5_p50": g(lambda x: x["trend"]["first5_p50"]), "last5_p50": g(lambda x: x["trend"]["last5_p50"]),
                "maxSampleGapMs": g(lambda x: x["maxSampleGapMs"]), "stallAtEndMs": g(lambda x: x["stallAtEndMs"]),
                "transport_p50": g(lambda x: x["breakdown_p50"]["transport"]),
                "recvToApply_p50": g(lambda x: x["breakdown_p50"]["recvToApply"]),
                "applyToPresent_p50": g(lambda x: x["breakdown_p50"]["applyToPresent"]),
                "recvFinal": g(lambda x: x["ticks"]["recvFinal"]), "lossFinal": g(lambda x: x["ticks"]["lossFinal"]),
                "backlogAtEnd": g(lambda x: x["ticks"]["backlogAtEnd"]),
                "toUi": g(lambda x: x["ticks"]["toUi"]), "applied": g(lambda x: x["ticks"]["applied"]),
                "batches": g(lambda x: x["ticks"]["batches"]),
                "cpu": g(lambda x: x["cpu"]), "cpuExLlvmpipe": g(lambda x: x["cpuExLlvmpipe"]), "genCpu": g(lambda x: x["gen"]["cpu"]),
                "rssStart": g(lambda x: x["mem"]["start"]["rss_mb"]), "rssEnd": g(lambda x: x["mem"]["end"]["rss_mb"]),
                "pssStart": g(lambda x: x["mem"]["start"]["pss_mb"]), "pssEnd": g(lambda x: x["mem"]["end"]["pss_mb"]),
                "rssGrowth": g(lambda x: x["mem"]["rssGrowth"]), "pssGrowth": g(lambda x: x["mem"]["pssGrowth"]),
                "fps": g(lambda x: x["fps"]), "fpsMin": g(lambda x: x["fpsMin"]),
                "switch_p50": med([v for x in rs for v in x["switch"]["all"]]),
                "switch_max": max([v for x in rs for v in x["switch"]["all"]], default=None),
                "switch_n": sum(x["switch"]["n"] for x in rs), "switch_missed": sum(x["switch"]["missed"] for x in rs),
                "genMaxDue": g(lambda x: x["gen"]["maxDue"]), "genBehindFrames": g(lambda x: x["gen"]["behindFrames"]),
            }
    return out

def main():
    for p in os.listdir("/proc"):
        if p.isdigit() and comm(int(p)) in ("wry-xq-demo", "gpui-xq-demo", "xq-tickgen"):
            raise SystemExit(f"有程式在跑（pid {p} {comm(int(p))}），先關掉再量")
    import socket
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    host, port = ADDR.rsplit(":", 1)
    try: s.bind((host, int(port)))
    except OSError: raise SystemExit(f"{ADDR} 被佔用（先停 WryFeedHost）")
    s.close()
    path = f"{OUT}/ticks-results.json"
    results = json.load(open(path)) if os.environ.get("RESUME") and os.path.exists(path) else []
    for scen in SCEN:
        for i in range(RUNS):
            for name in APPS_SEL:
                if sum(1 for x in results if x["app"] == name and x["scenario"] == scen) > i: continue
                shot = (i == RUNS - 1) and scen in ("s50k", "burst")
                t = time.strftime("%H:%M:%S")
                r = run_once(name, scen, shot)
                results.append(r)
                json.dump(results, open(path, "w"), ensure_ascii=False, indent=1)
                print(f"[{t}] {name:4s} {scen:5s} #{i+1} sent={r['ticks']['sent']} recv={r['ticks']['recvFinal']} "
                      f"lat p50/p99/max={r['latOldest']['p50']}/{r['latOldest']['p99']}/{r['latOldest']['max']}ms "
                      f"first5/last5={r['trend']['first5_p50']}/{r['trend']['last5_p50']} cpu={r['cpu']}({r['cpuExLlvmpipe']}) "
                      f"fps={r['fps']} rss={r['mem']['start']['rss_mb']}→{r['mem']['end']['rss_mb']} sw={r['switch']['p50']}ms"
                      f" gap={r['maxSampleGapMs']}", flush=True)
                time.sleep(1.0)
    summary = {"date": time.strftime("%Y-%m-%d %H:%M %z"), "runs": RUNS, "durationSec": DUR, "scenarios": SCENARIOS,
               "medians": summarize(results)}
    json.dump(summary, open(f"{OUT}/ticks-summary.json", "w"), ensure_ascii=False, indent=1)
    print(json.dumps(summary["medians"], ensure_ascii=False, indent=1))

main()
PY
