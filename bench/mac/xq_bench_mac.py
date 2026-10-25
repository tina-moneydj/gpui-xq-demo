#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""XQ 壓測 macOS 版：GPUI vs egui（wry 已淘汰，不比）。

情境與量法對齊 Linux 版（scripts/bench-*.sh）與 Windows 版（bench/windows），3 輪取中位數：
  main    主視窗 1200×720（邏輯點）＋ xq-demofeed（= WryFeedHost --demo-ticks 的 Rust 版）：啟動、記憶體、CPU、FPS
  stress  5 萬列 × 6 欄報價表，每秒 3000 筆跳價 30 秒，再邊跳價邊捲動 10 秒
  heavy   重圖表：20 疊加＋8 副圖＋1000 筆畫線分 100 層（XQ_CHART_LAYERS），跳價／十字線／平移／拖曳／選取／換層順序／正確性
  ticks   高頻進價（xq-tickgen）：每秒 1k、10k、50k、100k 筆＋開盤爆量；延遲 p50/p99、遺失、積壓、CPU、FPS
  multi   1 個與 4 個看盤視窗（每秒 10k 筆），分到各螢幕（Retina／外接螢幕各自的縮放與更新率都記錄下來）

只用 macOS 內建工具：ps（RSS、累計 CPU 時間）、vmmap --summary（Physical footprint，即「活動監視器」的記憶體欄；
只在量測區間外取，因為 vmmap 會暫停目標程式一下）、ioreg（整機 GPU 使用率）、osascript JXA（螢幕資訊、視窗位置）、
sysctl／sw_vers／system_profiler（機器資訊）。不需要 sudo，也不需要「輔助使用」或「螢幕錄製」權限。

用法：python3 xq_bench_mac.py [--quick] [--runs 3] [--apps gpui,gpui-quads,egui-wgpu,egui] [--scenarios main,stress,heavy,ticks,multi]
                             [--ticks s1k,s10k,s50k,s100k,burst] [--windows 1,4] [--layers 100] [--bin-dir DIR] [--port 47631]
在 Linux 上也能跑（開發者自測流程用；記憶體/CPU 改讀 /proc，螢幕假設單一 1920×1080）。
"""
import argparse, datetime, json, os, platform, re, shutil, socket, statistics, subprocess, sys, tempfile, threading, time, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
IS_MAC = sys.platform == "darwin"

# ---------------------------------------------------------------- 參數
ap = argparse.ArgumentParser(description="XQ 壓測 macOS 版（GPUI vs egui）")
ap.add_argument("--runs", type=int, default=None)
ap.add_argument("--quick", action="store_true", help="1 輪；高頻只跑 10k、100k、爆量")
ap.add_argument("--apps", default="gpui,gpui-nofix,egui-wgpu,egui",
                help="gpui-nofix＝同一個 GPUI 執行檔關掉 10/25 的兩個修補（XQ_INACTIVE_THROTTLE=1、XQ_MAC_ACTIVITY=0），只跑高頻 s10k 與 4 視窗，A/B 對照用；"
                     "gpui-quads＝GPUI 開 XQ_DRAW_QUADS=1，只跑重圖表（10/24 實測沒有比較快、CPU 還多 6–9 點，預設不跑）；"
                     "gpui-norelief＝GPUI 關掉 10/25 下午的 malloc 還記憶體修補（XQ_MALLOC_RELIEF_MS=0），只跑重圖表，A/B 對照用")
ap.add_argument("--scenarios", default="main,stress,heavy,ticks,multi")
ap.add_argument("--ticks", default=None)
ap.add_argument("--windows", default="1,4")
ap.add_argument("--layers", default="100", help="重圖表畫線分幾層（1＝不分層對照）")
ap.add_argument("--bin-dir", default=os.path.join(HERE, "bin"))
ap.add_argument("--out", default=None)
ap.add_argument("--port", type=int, default=47631)
ap.add_argument("--no-footprint", action="store_true", help="不跑 vmmap（只記 RSS）")
ap.add_argument("--no-zip", action="store_true")
ap.add_argument("--resummarize", default=None, metavar="results.json",
                help="不跑測試，只用舊的 results.json 重新算勝負、重寫 summary.md／results.csv（寫到 --out，預設同資料夾）")
A = ap.parse_args()
RUNS = A.runs if A.runs is not None else (1 if A.quick else 3)
APPS = [a.strip() for a in A.apps.split(",") if a.strip()]
SCEN = [s.strip() for s in A.scenarios.split(",") if s.strip()]
TICKS = [s.strip() for s in (A.ticks or ("s10k,s100k,burst" if A.quick else "s1k,s10k,s50k,s100k,burst")).split(",") if s.strip()]
NWIN = [int(x) for x in A.windows.split(",") if x.strip()]
ADDR = "127.0.0.1:%d" % A.port

# ---------------------------------------------------------------- 常數（同 Linux／Windows 版）
W, H = 1200, 720
T_IDLE, T_CPU0, T_END = 10.0, 30.0, 60.0
DUR, SETTLE, WARMUP, TAIL = 30, 3, 3, 4
HEAVY_PHASES = ("ticks", "crosshair", "pan", "drag", "select", "reorder", "zshot")
TICK_ARGS = {"s1k": ["--rate", "1000"], "s10k": ["--rate", "10000"], "s50k": ["--rate", "50000"],
             "s100k": ["--rate", "100000"], "burst": ["--burst", "150000,3,5000"]}
STRESS_ENV = {"XQ_GROUP_STRESS": "1", "XQ_STRESS_COLS": "6", "XQ_STRESS_TICK_HZ": "3000",
              "XQ_STRESS_TICK_MS": "30000", "XQ_STRESS_SCROLL_MS": "10000"}
HEAVY_ENV = {"XQ_CHART_STRESS": "1", "XQ_TOP_FRAC": "0.8", "XQ_CHART_WARM_MS": "3000", "XQ_CHART_PHASE_MS": "10000",
             "XQ_CHART_DRAWINGS": "1000", "XQ_CHART_LAYERS": A.layers}
TITLE_BAR = 28.0  # macOS 標準標題列高度（點）；視窗內容區往下讓這麼多
APPDEF = {
    "gpui":      {"bin": "gpui-xq-demo", "label": "GPUI（Metal）", "env": {"XQ_DRAW_QUADS": "0"}},
    # 同一個 GPUI 執行檔打開 XQ_DRAW_QUADS=1（畫線層橫平豎直的圖形改用 quad 畫）；只影響畫線，所以只跑重圖表
    "gpui-quads": {"bin": "gpui-xq-demo", "label": "GPUI quad 畫線（XQ_DRAW_QUADS=1）", "env": {"XQ_DRAW_QUADS": "1"}, "only": ("heavy",)},
    # 2026-10-25：同一個 GPUI 執行檔，關掉「全部視窗不節流」與「macOS latency-critical（關 App Nap）」兩個修補，A/B 看停頓有沒有變
    "gpui-nofix": {"bin": "gpui-xq-demo", "label": "GPUI 未修補（對照）", "env": {"XQ_DRAW_QUADS": "0", "XQ_INACTIVE_THROTTLE": "1", "XQ_MAC_ACTIVITY": "0"},
                   "only": ("ticks", "multi"), "ticks": ("s10k",), "windows": (4,)},
    # 2026-10-25 下午：同一個 GPUI 執行檔關掉「每 0.5 秒 malloc_zone_pressure_relief」（src/memrelief.rs），看重圖表多的 ~200 MB 有沒有還回去
    "gpui-norelief": {"bin": "gpui-xq-demo", "label": "GPUI 未還記憶體（對照）", "env": {"XQ_DRAW_QUADS": "0", "XQ_MALLOC_RELIEF_MS": "0"}, "only": ("heavy",)},
    "gpui-r100": {"bin": "gpui-xq-demo", "label": "GPUI relief 100ms", "env": {"XQ_DRAW_QUADS": "0", "XQ_MALLOC_RELIEF_MS": "100"}, "only": ("heavy",)},
    "egui-wgpu": {"bin": "egui-xq-demo", "label": "egui（wgpu／Metal）", "env": {"XQ_RENDERER": "wgpu"}},
    "egui":      {"bin": "egui-xq-demo", "label": "egui（glow／OpenGL）", "env": {"XQ_RENDERER": "glow"}},
}
if not IS_MAC:  # Linux 自測：沒有 Metal
    APPDEF["gpui"]["label"] = "GPUI"; APPDEF["gpui-quads"]["label"] = "GPUI quad（XQ_DRAW_QUADS=1）"; APPDEF["egui-wgpu"]["label"] = "egui（wgpu）"; APPDEF["egui"]["label"] = "egui（glow）"
def runs_in(app, scen):
    o = APPDEF.get(app, {}).get("only"); return o is None or scen in o
def runs_sub(app, sub):
    """高頻情境（s10k…）／視窗數（4）的限制；只有 gpui-nofix 有"""
    d = APPDEF.get(app, {})
    if isinstance(sub, int): return sub in d.get("windows", (sub,))
    return sub in d.get("ticks", (sub,))
def apps_in(gid):
    out = []
    for a in APPS:
        if not runs_in(a, gid.split("-")[0]): continue
        if gid.startswith("ticks-") and not runs_sub(a, gid[6:]): continue
        if gid.startswith("multi-") and not runs_sub(a, int(gid[6:-1])): continue
        out.append(a)
    return out
for a in APPS:
    if a not in APPDEF: sys.exit("不認得的 app：%s（可用 %s）" % (a, ",".join(APPDEF)))

STAMP = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
# 不用電腦名稱（常含使用者名字；結果會放進公開 repo）
HOST = "mac"
OUT = A.out or (os.path.dirname(os.path.abspath(A.resummarize)) if A.resummarize else os.path.join(HERE, "results", "%s-%s" % (STAMP, HOST)))
os.makedirs(OUT, exist_ok=True)
WORK = tempfile.mkdtemp(prefix="xq-bench-")
LOGLINES = []

# 匿名化：結果檔不留家目錄路徑／電腦名稱／帳號（會放進公開 repo）
_HOME = os.path.expanduser("~"); _NODE = (platform.node() or "").split(".")[0]
try: _USER = os.path.basename(_HOME)
except Exception: _USER = ""
def anon(text):
    if _HOME and len(_HOME) > 1: text = text.replace(_HOME, "~")
    if _NODE and len(_NODE) > 2: text = text.replace(_NODE, "mac")
    if _USER and len(_USER) > 2: text = re.sub(r"(?<![A-Za-z0-9_])%s(?![A-Za-z0-9_])" % re.escape(_USER), "user", text)
    return text

def say(msg):
    line = "[%s] %s" % (time.strftime("%H:%M:%S"), msg)
    print(line, flush=True); LOGLINES.append(line)

def r1(v):
    return None if v is None else round(float(v), 1)

def med(xs):
    v = [float(x) for x in xs if x is not None]
    return statistics.median(v) if v else None

def pct(xs, q):
    v = sorted(float(x) for x in xs if x is not None)
    if not v: return None
    i = int(round(q / 100.0 * (len(v) - 1)))
    return v[max(0, min(len(v) - 1, i))]

def sh(cmd, timeout=20):
    try: return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout).stdout
    except Exception: return ""

# ---------------------------------------------------------------- 機器／螢幕
JXA_SCREENS = r'''
ObjC.import('AppKit');
var s = $.NSScreen.screens, out = [];
for (var i = 0; i < s.count; i++) {
  var sc = s.objectAtIndex(i), f = sc.frame, v = sc.visibleFrame, o = {};
  o.x = f.origin.x; o.y = f.origin.y; o.w = f.size.width; o.h = f.size.height;
  o.vx = v.origin.x; o.vy = v.origin.y; o.vw = v.size.width; o.vh = v.size.height;
  o.scale = sc.backingScaleFactor;
  try { o.id = sc.deviceDescription.objectForKey('NSScreenNumber').unsignedIntValue; } catch (e) { o.id = null; }
  try { o.name = ObjC.unwrap(sc.localizedName); } catch (e) { o.name = null; }
  try { o.maxFps = sc.maximumFramesPerSecond; } catch (e) { o.maxFps = null; }
  out.push(o);
}
JSON.stringify(out);
'''

def screens():
    """各螢幕（全域座標，左上角原點＝主螢幕左上，單位：點）"""
    if not IS_MAC:
        return [{"index": 0, "id": None, "name": "linux-test", "x": 0, "y": 0, "w": 1920, "h": 1080,
                 "vx": 0, "vy": 0, "vw": 1920, "vh": 1080, "scale": 1.0, "maxFps": None}]
    raw = sh(["osascript", "-l", "JavaScript", "-e", JXA_SCREENS])
    try: arr = json.loads(raw)
    except Exception: arr = []
    if not arr:
        say("！讀不到螢幕資訊，假設單一 1440×900 螢幕")
        return [{"index": 0, "id": None, "name": "?", "x": 0, "y": 0, "w": 1440, "h": 900,
                 "vx": 0, "vy": 25, "vw": 1440, "vh": 875, "scale": 2.0, "maxFps": None}]
    H0 = arr[0]["h"]
    out = []
    for i, o in enumerate(arr):  # Cocoa 座標（左下原點、y 往上）→ 左上原點
        out.append({"index": i, "id": o.get("id"), "name": o.get("name"), "scale": o.get("scale"), "maxFps": o.get("maxFps"),
                    "x": o["x"], "y": H0 - (o["y"] + o["h"]), "w": o["w"], "h": o["h"],
                    "vx": o["vx"], "vy": H0 - (o["vy"] + o["vh"]), "vw": o["vw"], "vh": o["vh"],
                    "pixels": "%dx%d" % (round(o["w"] * (o.get("scale") or 1)), round(o["h"] * (o.get("scale") or 1)))})
    return out

def machine_info(scr):
    info = {"host": HOST, "python": platform.python_version(), "platform": platform.platform()}
    if IS_MAC:
        g = lambda k: sh(["sysctl", "-n", k]).strip()
        info.update({
            "macos": " ".join(sh(["sw_vers", "-productVersion"]).split() + ["(" + sh(["sw_vers", "-buildVersion"]).strip() + ")"]),
            "model": g("hw.model"), "cpu": g("machdep.cpu.brand_string"), "arch": platform.machine(),
            "rosetta": g("sysctl.proc_translated") == "1",
            "cores": g("hw.ncpu"), "perfCores": g("hw.perflevel0.logicalcpu"), "effCores": g("hw.perflevel1.logicalcpu"),
            "ramGB": round(int(g("hw.memsize") or 0) / 2**30, 1),
            "power": sh(["pmset", "-g", "batt"]).strip().splitlines()[:2],
            "lowPowerMode": "lowpowermode 1" in sh(["pmset", "-g"]).replace("\t", " ").replace("  ", " "),
        })
        try:
            d = json.loads(sh(["system_profiler", "SPDisplaysDataType", "-json"], 60) or "{}")
            gpus = []
            for gp in d.get("SPDisplaysDataType", []):
                gpus.append({"name": gp.get("sppci_model") or gp.get("_name"), "cores": gp.get("sppci_cores"),
                             "metal": gp.get("spdisplays_mtlgpufamilysupport"),
                             "displays": [{"name": x.get("_name"), "res": x.get("_spdisplays_resolution") or x.get("spdisplays_resolution"),
                                           "pixels": x.get("_spdisplays_pixels"), "retina": x.get("spdisplays_retina"),
                                           "main": x.get("spdisplays_main")} for x in gp.get("spdisplays_ndrvs", [])]})
            info["gpus"] = gpus
        except Exception as e:
            info["gpus_error"] = str(e)
    else:
        info["cpu"] = platform.processor()
    info["screens"] = scr
    return info

# ---------------------------------------------------------------- 程序量測
def ppid_map():
    m = {}
    for line in sh(["ps", "-A", "-o", "pid=,ppid="]).splitlines():
        p = line.split()
        if len(p) == 2: m[int(p[0])] = int(p[1])
    return m

def tree(root):
    m = ppid_map(); s = {root}; changed = True
    while changed:
        changed = False
        for p, pp in m.items():
            if pp in s and p not in s: s.add(p); changed = True
    return sorted(s)

def parse_cputime(t):
    # macOS ps time：[[dd-]hh:]mm:ss.cc
    t = t.strip()
    if not t: return 0.0
    days = 0
    if "-" in t: d, t = t.split("-", 1); days = int(d)
    secs = 0.0
    for part in t.split(":"): secs = secs * 60 + float(part)
    return days * 86400 + secs

def cpu_secs(pids):
    tot = 0.0
    for p in pids:
        if not IS_MAC and os.path.exists("/proc/%d/stat" % p):
            try:
                f = open("/proc/%d/stat" % p).read().rsplit(")", 1)[1].split()
                tot += (int(f[11]) + int(f[12])) / os.sysconf("SC_CLK_TCK"); continue
            except Exception: pass
        tot += parse_cputime(sh(["ps", "-o", "time=", "-p", str(p)]))
    return tot

def rss_mb(pids):
    tot = 0
    for p in pids:
        v = sh(["ps", "-o", "rss=", "-p", str(p)]).strip()
        if v.isdigit(): tot += int(v)
    return round(tot / 1024.0, 1)

def footprint_mb(pid):
    """vmmap --summary 的 Physical footprint（≈ 活動監視器「記憶體」欄）；會短暫暫停目標，只在量測區間外呼叫。
    2026-10-25：前後在程式的 log 記 MARK vmmap-begin／end（10/24 實測：GPUI 每輪那一次約 0.5 秒的停頓就是這裡造成的，
    egui 同一秒也掉了一半的幀，只是 egui 沒有停頓偵測；都在量測區間外，不影響表格裡的數字）"""
    if not IS_MAC or A.no_footprint: return None
    pr = PROCS.get(pid)
    if pr: pr.mark("vmmap-begin")
    out = sh(["vmmap", "--summary", str(pid)], 30)
    if pr: pr.mark("vmmap-end")
    # 2026-10-25：記憶體拆解——把 vmmap 的分區表（REGION TYPE：MALLOC_*、IOAccelerator／IOSurface＝GPU、__TEXT＝程式碼、
    # mapped file＝字型等）存到 OUT/vmmap/，看 GPUI 重圖表多出來的 ~200 MB 是哪一類。只存表格（已去掉使用者名稱／路徑）。
    try:
        i = out.find("REGION TYPE")
        if i >= 0 and pr:
            d = os.path.join(OUT, "vmmap"); os.makedirs(d, exist_ok=True)
            fn = re.sub(r"[^A-Za-z0-9_.-]+", "_", "%s-%s-%.0fs" % (pr.tag, pr.scen or "x", time.monotonic() - pr.t0))
            open(os.path.join(d, fn + ".txt"), "w", encoding="utf-8").write(anon(out[i:]))
    except Exception as ex:
        say("vmmap 分區表存檔失敗：%s" % ex)
    m = re.search(r"Physical footprint:\s+([0-9.]+)([KMG])", out)
    if not m: return None
    v = float(m.group(1)); u = m.group(2)
    return round(v / 1024 if u == "K" else v * 1024 if u == "G" else v, 1)

def mem(pid, footprint=False):
    pids = tree(pid)
    o = {"rss_mb": rss_mb(pids), "nproc": len(pids)}
    if footprint: o["footprint_mb"] = footprint_mb(pid)
    return o

class GpuSampler:
    """整機 GPU 使用率（ioreg IOAccelerator PerformanceStatistics "Device Utilization %"），每秒取樣"""
    def __init__(self):
        self.v = []; self.on = False; self.t = None
    def start(self):
        self.v = []; self.on = IS_MAC
        if not self.on: return
        def loop():
            while self.on:
                out = sh(["ioreg", "-r", "-d", "1", "-w", "0", "-c", "IOAccelerator"], 5)
                vals = [int(x) for x in re.findall(r'"Device Utilization %"\s*=\s*(\d+)', out)]
                if vals: self.v.append(max(vals))
                time.sleep(1.0)
        self.t = threading.Thread(target=loop, daemon=True); self.t.start()
    def stop(self):
        self.on = False
        return r1(statistics.mean(self.v)) if self.v else None

GPU = GpuSampler()

JXA_WINDOWS = r'''
ObjC.import('CoreGraphics');
var a = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo(1, 0)));
var pid = %d, out = [];
for (var i = 0; i < a.length; i++) { var w = a[i]; if (w.kCGWindowOwnerPID == pid && w.kCGWindowLayer == 0) out.push(w.kCGWindowBounds); }
JSON.stringify(out);
'''

def window_bounds(pid):
    """實際視窗外框（全域座標，點）；不需要權限（只讀位置、不讀標題）"""
    if not IS_MAC: return None
    try: return json.loads(sh(["osascript", "-l", "JavaScript", "-e", JXA_WINDOWS % pid]) or "null")
    except Exception: return None

# ---------------------------------------------------------------- 程式啟動／輸出解析
PROCS = {}       # pid → Proc（footprint_mb 用來記 MARK）
LAST_DIAG = [None]  # 最近一次 app 程序的卡頓診斷（Proc.stop 時算好，主流程接到結果上）
STALL_TMP = None    # XQ_STALL_SAMPLE_DIR（GPUI 卡住時自動 sample 的輸出），main() 裡設定
def _dict_json(l, tag):
    i = l.find(tag)
    if i < 0: return None
    try: return json.loads(l[i + len(tag):])
    except Exception: return None

class Proc:
    def __init__(self, cmd, env, tag):
        self.tag = tag; self.t0 = time.monotonic(); self.lines = []; self.ev = {}; self.fps = []; self.tb = []; self.gen = []
        self.stalls = []; self.longf = []; self.gpulog = []; self.marks = []; self.scen = ""
        self.p = subprocess.Popen(cmd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                  text=True, bufsize=1, errors="replace", cwd=WORK)
        self.pid = self.p.pid; PROCS[self.pid] = self
        self.th = threading.Thread(target=self._read, daemon=True); self.th.start()
    def mark(self, name):
        t = time.monotonic() - self.t0; self.marks.append((round(t, 3), name)); self.lines.append("%8.3f MARK %s" % (t, name))
    def _read(self):
        for line in self.p.stdout:
            now = time.monotonic(); t = now - self.t0; l = line.rstrip("\n")
            i = l.find("TICKBENCH|")
            if i >= 0:
                try: self.tb.append((now, json.loads(l[i + 10:])))
                except Exception: self.lines.append("%8.3f BAD %s" % (t, l))
                continue
            self.lines.append("%8.3f %s" % (t, l))
            j = _dict_json(l, "STALL|")
            if j is not None: j["tBench"] = round(t, 3); self.stalls.append(j)
            j = _dict_json(l, "LONGFRAME|")
            if j is not None: j["tBench"] = round(t, 3); self.longf.append(j)
            if l.startswith("GPUILOG|") and len(self.gpulog) < 40: self.gpulog.append(l[8:300])
            if l.startswith("{"):
                try: self.gen.append((now, json.loads(l))); continue
                except Exception: pass
            for ev in ("first-frame", "first-quote", "heavy-frame"):
                if ("BENCH " + ev) in l and ev not in self.ev: self.ev[ev] = t
            i = l.find("CHARTPHASE|")
            if i >= 0:
                p = l[i + 11:].split("|")
                if len(p) >= 2: self.ev["ph:%s:%s" % (p[0], p[1].strip().split(" ")[0])] = t
            for tag, pre in (("CHARTPERF|{", "cp"), ("GROUPPERF|{", "gp")):
                i = l.find(tag)
                if i >= 0:
                    try:
                        j = json.loads(l[i + len(tag) - 1:]); self.ev["%s:%s" % (pre, j.get("phase", "?"))] = (t, j)
                    except Exception: pass
            if "BENCH fps" in l or "[metrics] fps=" in l:
                m = re.search(r"fps[ =]+([0-9.]+)", l)
                if m: self.fps.append((t, float(m.group(1))))
    def alive(self): return self.p.poll() is None
    def wait_ev(self, key, timeout):
        end = time.monotonic() + timeout
        while key not in self.ev:
            if not self.alive(): raise RuntimeError("%s：程式結束了（exit=%s，等 %s）" % (self.tag, self.p.returncode, key))
            if time.monotonic() > end: raise RuntimeError("%s：等不到 %s" % (self.tag, key))
            time.sleep(0.02)
        return self.ev[key]
    def wait_gen(self, ev, timeout):
        end = time.monotonic() + timeout
        while True:
            for t, j in self.gen:
                if j.get("ev") == ev: return t, j
                if j.get("ev") == "error": raise RuntimeError("xq-tickgen error: %s" % j)
            if not self.alive() and ev != "end": raise RuntimeError("xq-tickgen 結束了（等 %s）" % ev)
            if time.monotonic() > end: raise RuntimeError("產生器等不到 %s" % ev)
            time.sleep(0.01)
    def stop(self, logname=None, header=""):
        # 只結束這次自己啟動的程序（確切 PID）
        if self.alive():
            self.p.terminate()
            try: self.p.wait(8)
            except subprocess.TimeoutExpired: self.p.kill(); self.p.wait()
        self.th.join(2)
        PROCS.pop(self.pid, None)
        if self.tag in APPDEF: LAST_DIAG[0] = self.diag(header)
        if logname:
            with open(os.path.join(OUT, logname), "a", encoding="utf-8") as f:
                f.write(anon("==== %s %s\n%s\n" % (header, datetime.datetime.now().isoformat(timespec="seconds"), "\n".join(self.lines))))

    def diag(self, header):
        """卡頓診斷：STALL／LONGFRAME 行、vmmap 時間、GPUI log、自動 sample 的檔案（搬進結果資料夾、匿名化）"""
        vm = []; b = None
        for t, n in self.marks:
            if n == "vmmap-begin": b = t
            elif n == "vmmap-end" and b is not None: vm.append([b, t]); b = None
        for st in self.stalls:
            # STALL 的 t 是程式自己的時鐘（程式啟動起算），tBench 是壓測腳本收到那一行的時間（Proc 啟動起算），差不到 0.2 秒
            tb = st.get("tBench", 0) - st.get("ms", 0) / 1000.0
            st["duringVmmap"] = any(x - 0.3 <= tb <= y + 0.3 for x, y in vm)
        files = []
        if STALL_TMP and os.path.isdir(STALL_TMP):
            dst = os.path.join(OUT, "stalls"); os.makedirs(dst, exist_ok=True)
            for f in sorted(os.listdir(STALL_TMP)):
                if not f.endswith(".txt"): continue
                name = "%s-%s" % (re.sub(r"[^A-Za-z0-9_.-]+", "_", header or self.tag), f)
                try:
                    txt = open(os.path.join(STALL_TMP, f), encoding="utf-8", errors="replace").read()
                    open(os.path.join(dst, name), "w", encoding="utf-8").write(anon(txt)); files.append("stalls/" + name)
                except Exception: pass
                try: os.remove(os.path.join(STALL_TMP, f))
                except Exception: pass
        return {"stalls": self.stalls, "longframes": self.longf, "vmmap": vm, "gpuilog": self.gpulog, "samples": files}

def bin_path(name):
    p = os.path.join(A.bin_dir, name)
    if not os.path.exists(p): sys.exit("找不到 %s（先跑 ./build-mac.sh，或用 --bin-dir 指定）" % p)
    return p

def app_env(app, extra, rects=None):
    e = dict(os.environ)
    for k in list(e):
        if k.startswith("XQ_"): del e[k]
    e.update({"XQ_BENCH": "1", "XQ_PERIOD": "D", "XQ_FEED_ADDR": ADDR, "XDG_DATA_HOME": os.path.join(WORK, "data"),
              "XQ_GPUI_STATE": os.path.join(WORK, "gpui-state.json"), "XQ_EGUI_STATE": os.path.join(WORK, "egui-state.json"),
              "XQ_TAPE_STATE": os.path.join(WORK, "tape-state.json")})
    e.update(APPDEF[app]["env"]); e.update(extra)
    if STALL_TMP and APPDEF[app]["bin"].startswith("gpui"): e["XQ_STALL_SAMPLE_DIR"] = STALL_TMP
    if rects: e["XQ_WIN_RECTS"] = ";".join(rects)
    return e

def reset_state():
    for f in ("gpui-state.json", "egui-state.json", "tape-state.json"):
        try: os.remove(os.path.join(WORK, f))
        except FileNotFoundError: pass
    shutil.rmtree(os.path.join(WORK, "data"), ignore_errors=True)

def port_busy():
    try: socket.create_connection(("127.0.0.1", A.port), 0.3).close(); return True
    except OSError: return False

def wait_port(timeout):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if port_busy(): return True
        time.sleep(0.1)
    return False

# ---------------------------------------------------------------- 視窗位置
def rect_str(scr, x, y, w, h):
    s = "%d,%d,%d,%d" % (x, y, w, h)
    if scr.get("id") is not None: s += ",%d,%d,%d" % (scr["id"], scr["x"], scr["y"])
    return s

def main_rect(SCR):
    s = SCR[0]
    return [rect_str(s, s["vx"] + 20, s["vy"] + TITLE_BAR + 12, W, H)]

def multi_rects(SCR, n):
    """n 個視窗：螢幕 ≥ n 時一個螢幕一個（佔滿可用區）；不夠時輪流分到各螢幕，同一螢幕內格狀鋪滿（同 Windows 版）"""
    per = {i: [] for i in range(len(SCR))}
    for k in range(n): per[k % len(SCR)].append(k)
    rects = [None] * n; place = []
    for si, ks in per.items():
        if not ks: continue
        s = SCR[si]; m = len(ks)
        cols = 1
        while cols * cols < m: cols += 1
        rows = (m + cols - 1) // cols
        cw, ch = s["vw"] / cols, s["vh"] / rows
        for j, k in enumerate(ks):
            r, c = divmod(j, cols)
            x = s["vx"] + c * cw; y = s["vy"] + r * ch + TITLE_BAR
            w = cw; h = ch - TITLE_BAR
            rects[k] = rect_str(s, x, y, w, h)
            place.append({"win": k, "screen": si, "screenName": s.get("name"), "scale": s.get("scale"),
                          "content_pt": "%dx%d" % (w, h), "content_px": "%dx%d" % (w * (s.get("scale") or 1), h * (s.get("scale") or 1))})
    return rects, place

# ---------------------------------------------------------------- feed
FEED = None
def restart_feed():
    global FEED
    if FEED: FEED.stop("demofeed.log", "feed"); time.sleep(0.5)
    FEED = Proc([bin_path("xq-demofeed"), "--addr", ADDR], dict(os.environ), "xq-demofeed")
    if not wait_port(20): raise RuntimeError("xq-demofeed 起不來（%s）" % ADDR)
    time.sleep(1.0)

def stop_feed():
    global FEED
    if FEED: FEED.stop("demofeed.log", "feed"); FEED = None

# ---------------------------------------------------------------- 情境
def run_main(app, SCR):
    restart_feed(); reset_state()
    st = Proc([bin_path(APPDEF[app]["bin"])], app_env(app, {"XQ_METRICS": "1"}, main_rect(SCR)), app); st.scen = "main"
    try:
        st.wait_ev("first-frame", 30)
        wb = None
        while time.monotonic() - st.t0 < T_IDLE:
            if wb is None and time.monotonic() - st.t0 > 3: wb = window_bounds(st.pid)
            time.sleep(0.05)
        idle = mem(st.pid, footprint=True)
        while time.monotonic() - st.t0 < T_CPU0: time.sleep(0.05)
        pids = tree(st.pid); c0 = cpu_secs(pids); ta = time.monotonic(); GPU.start()
        while time.monotonic() - st.t0 < T_END: time.sleep(0.05)
        c1 = cpu_secs(pids); tb = time.monotonic(); g = GPU.stop()
        steady = mem(st.pid, footprint=True)
        f = [v for t, v in st.fps if T_CPU0 <= t <= T_END]
        if not st.alive(): raise RuntimeError("%s 中途結束" % app)
        return {"app": app, "first_frame_ms": r1(st.ev.get("first-frame", 0) * 1000) if "first-frame" in st.ev else None,
                "first_quote_ms": r1(st.ev["first-quote"] * 1000) if "first-quote" in st.ev else None,
                "idle10": idle, "steady60": steady, "cpu_pct": r1((c1 - c0) / (tb - ta) * 100), "gpu_util": g,
                "fps": r1(statistics.mean(f)) if f else None, "window": wb}
    finally:
        st.stop("%s-main.log" % app, "main")

def run_stress(app, SCR):
    restart_feed(); reset_state()
    st = Proc([bin_path(APPDEF[app]["bin"])], app_env(app, dict(STRESS_ENV), main_rect(SCR)), app); st.scen = "stress"
    try:
        st.wait_ev("gp:tick-start", 40)
        pids = tree(st.pid); c0 = cpu_secs(pids); ta = time.monotonic(); GPU.start()
        _, te = st.wait_ev("gp:tick-end", 60)
        c1 = cpu_secs(pids); tb = time.monotonic(); g1 = GPU.stop(); GPU.start()
        m_tick = mem(st.pid)
        while time.monotonic() - tb < 8: time.sleep(0.05)
        m_scroll = mem(st.pid)
        _, done = st.wait_ev("gp:done", 30)
        c2 = cpu_secs(pids); tc = time.monotonic(); g2 = GPU.stop()
        m_end = mem(st.pid, footprint=True)
        return {"app": app, "tick_end": te, "done": done, "cpu_tick_pct": r1((c1 - c0) / (tb - ta) * 100),
                "cpu_scroll_pct": r1((c2 - c1) / (tc - tb) * 100), "gpu_tick": g1, "gpu_scroll": g2,
                "mem_tick": m_tick, "mem_scroll": m_scroll, "mem_end": m_end}
    finally:
        st.stop("%s-stress.log" % app, "stress")

def run_heavy(app, SCR):
    restart_feed(); reset_state()
    st = Proc([bin_path(APPDEF[app]["bin"])], app_env(app, dict(HEAVY_ENV), main_rect(SCR)), app); st.scen = "heavy"
    try:
        _, setup = st.wait_ev("cp:setup", 60)
        st.wait_ev("heavy-frame", 30)
        out = {"app": app, "heavy_frame_ms": r1(st.ev["heavy-frame"] * 1000), "setup": setup, "phases": {}}
        for ph in HEAVY_PHASES:
            st.wait_ev("ph:%s:start" % ph, 30)
            pids = tree(st.pid); c0 = cpu_secs(pids); ta = time.monotonic(); GPU.start()
            while time.monotonic() - ta < (1.5 if ph == "zshot" else 5): time.sleep(0.02)
            m = mem(st.pid)
            st.wait_ev("ph:%s:end" % ph, 30)
            c1 = cpu_secs(pids); tb = time.monotonic(); g = GPU.stop()
            _, perf = st.wait_ev("cp:%s" % ph, 5)
            out["phases"][ph] = {"cpu_pct": r1((c1 - c0) / (tb - ta) * 100), "gpu_util": g, "mem": m, "perf": perf}
        st.wait_ev("cp:done", 10)
        out["mem_end"] = mem(st.pid, footprint=True)
        return out
    finally:
        st.stop("%s-heavy.log" % app, "heavy")

def counters(j):
    return {"recv": float(j.get("rowsIn", 0)), "toUi": float(j.get("inboxRows", 0)),
            "applied": float(j.get("applied", 0)), "batches": float(j.get("applies", 0))}

def run_feedgen(app, scen, nwin, SCR, tag):
    stop_feed()  # 主視窗／5 萬列／重圖表用的 xq-demofeed 先關掉，同一個埠換 xq-tickgen
    time.sleep(0.5)
    if port_busy(): raise RuntimeError("%s 已被佔用" % ADDR)
    gen = Proc([bin_path("xq-tickgen"), "--addr", ADDR] + TICK_ARGS[scen] +
               ["--duration", str(DUR), "--settle", str(SETTLE), "--warmup", str(WARMUP)], dict(os.environ), "xq-tickgen")
    st = None
    try:
        gen.wait_gen("listen", 10)
        reset_state()
        x = {"XQ_TICK_BENCH": "1"}
        place = None
        if nwin > 0:
            x["XQ_WINDOWS"] = str(nwin); rects, place = multi_rects(SCR, nwin)
        else:
            rects = main_rect(SCR)
        st = Proc([bin_path(APPDEF[app]["bin"])], app_env(app, x, rects), app); st.scen = ("multi%dw" % nwin) if nwin else scen
        tstart, gs = gen.wait_gen("start", 60)
        start_us = float(gs["unixUs"])
        pids = tree(st.pid); c0 = cpu_secs(pids); g0 = cpu_secs([gen.pid]); GPU.start()
        rss = [(0.0, rss_mb(pids))]; nxt = 2.0; wb = None
        while True:
            el = time.monotonic() - tstart
            if any(j.get("ev") == "end" for _, j in gen.gen) or el > DUR + 30: break
            if not st.alive(): raise RuntimeError("%s：程式結束了（exit=%s）" % (app, st.p.returncode))
            if el >= nxt: rss.append((round(el, 1), rss_mb(pids))); nxt += 2.0
            if wb is None and el > 3 and nwin > 0: wb = window_bounds(st.pid)
            time.sleep(0.02)
        tend, je = gen.wait_gen("end", 5)
        secs = tend - tstart
        c1 = cpu_secs(pids); g1 = cpu_secs([gen.pid]); gpu = GPU.stop()
        m_end = mem(st.pid); rss.append((round(secs, 1), m_end["rss_mb"]))
        tw = time.monotonic() + TAIL
        while time.monotonic() < tw: time.sleep(0.05)
        m_tail = mem(st.pid, footprint=True)
        return {"st": st, "tStart": tstart, "tEnd": tend, "startUs": start_us, "jEnd": je, "secs": secs,
                "cpu": r1((c1 - c0) / secs * 100), "genCpu": r1((g1 - g0) / secs * 100), "gpu": gpu,
                "rss": rss, "mEnd": m_end, "mTail": m_tail, "placement": place, "window": wb}
    finally:
        if st: st.stop("%s-%s.log" % (app, "multi" if nwin else "ticks"), tag)
        gen.stop("tickgen.log", tag)

def stats(v):
    return {"p50": r1(pct(v, 50)), "p95": r1(pct(v, 95)), "p99": r1(pct(v, 99)), "max": r1(max(v) if v else None), "n": len(v)}

def run_ticks(app, scen, SCR):
    r = run_feedgen(app, scen, 0, SCR, "tick-%s-%s" % (app, scen))
    tb = r["st"].tb; end_us = float(r["jEnd"]["unixUs"]); sent = float(r["jEnd"]["sent"])
    base = [j for t, j in tb if t < r["tStart"]]
    if not base: raise RuntimeError("%s/%s：沒有開始前的 TICKBENCH 行" % (app, scen))
    c0 = counters(base[-1])
    after = [j for t, j in tb if t >= r["tEnd"]]
    c_end = counters(after[0] if after else tb[-1][1]); c_fin = counters(tb[-1][1])
    samples = [s for _, j in tb for s in (j.get("lat") or []) if s and s[1] >= r["startUs"] and s[3] <= end_us]
    oldest = [(s[6] - s[1]) / 1000 for s in samples]; newest = [(s[6] - s[3]) / 1000 for s in samples]
    first5 = [(s[6] - s[1]) / 1000 for s in samples if s[1] < r["startUs"] + 5e6]
    last5 = [(s[6] - s[1]) / 1000 for s in samples if s[1] >= end_us - 5e6]
    pres = sorted(s[6] for s in samples)
    gaps = [(b - a) / 1000 for a, b in zip(pres, pres[1:])]
    fps = [float(j.get("fps", 0)) for t, j in tb if r["tStart"] + 1 <= t <= r["tEnd"]]
    f5, l5 = pct(first5, 50), pct(last5, 50)
    mx = max(samples, key=lambda s: s[6] - s[1]) if samples else None
    return {"app": app, "scenario": scen, "secs": r1(r["secs"]),
            "gen": {"sent": sent, "target": r["jEnd"].get("target"), "maxDue": r["jEnd"].get("maxDue"),
                    "behindFrames": r["jEnd"].get("behindFrames"), "cpu": r["genCpu"]},
            "latOldest": stats(oldest), "latNewest": stats(newest),
            "breakdown_p50": {"transport": r1(pct([(s[4] - s[3]) / 1000 for s in samples], 50)),
                              "recvToApply": r1(pct([(s[5] - s[4]) / 1000 for s in samples], 50)),
                              "applyToPresent": r1(pct([(s[6] - s[5]) / 1000 for s in samples], 50))},
            "trend": {"first5_p50": r1(f5), "last5_p50": r1(l5), "backlog_ms": r1(l5 - f5) if f5 is not None and l5 is not None else None},
            "maxSampleGapMs": r1(max(gaps) if gaps else None),
            # 2026-10-25：最慢那一筆在第幾秒、卡在哪一段（transport＝送出→背景收到；recvToApply＝收到→主執行緒套用；applyToPresent＝套用→下一幀開始）
            "maxLat": ({"atS": r1((mx[1] - r["startUs"]) / 1e6), "transport": r1((mx[4] - mx[3]) / 1000), "recvToApply": r1((mx[5] - mx[4]) / 1000),
                        "applyToPresent": r1((mx[6] - mx[5]) / 1000)} if mx else None),
            "maxGapAtS": (r1((pres[gaps.index(max(gaps))] - r["startUs"]) / 1e6) if gaps else None),
            "ticks": {"sent": sent, "recvFinal": c_fin["recv"] - c0["recv"], "applied": c_fin["applied"] - c0["applied"],
                      "lossFinal": sent - (c_fin["recv"] - c0["recv"]), "backlogAtEnd": sent - (c_end["recv"] - c0["recv"])},
            "cpu": r["cpu"], "gpu_util": r["gpu"], "rss": r["rss"], "mem_end": r["mEnd"], "mem_tail": r["mTail"],
            "fps": r1(med(fps)), "fpsMin": r1(min(fps) if fps else None)}

def run_multi(app, n, SCR):
    r = run_feedgen(app, "s10k", n, SCR, "multi-%s-%dw" % (app, n))
    end_us = float(r["jEnd"]["unixUs"]); sent = float(r["jEnd"]["sent"])
    by = {}
    for t, j in r["st"].tb: by.setdefault(int(j.get("win", 0) or 0), []).append((t, j))
    if 0 not in by: raise RuntimeError("%s/%dw：沒有主視窗的 TICKBENCH" % (app, n))
    base = [j for t, j in by[0] if t < r["tStart"]]
    if not base: raise RuntimeError("%s/%dw：沒有開始前的 TICKBENCH 行" % (app, n))
    recv = counters(by[0][-1][1])["recv"] - counters(base[-1])["recv"]
    per = {}; pooled = []; worst = None
    for k in range(n):
        lines = by.get(k, []); old = []
        for t, j in lines:
            for s in (j.get("lat") or []):
                if s and s[1] >= r["startUs"] and s[3] <= end_us:
                    v = (s[6] - s[1]) / 1000; old.append(v); pooled.append(v)
                    if worst is None or v > worst[0]: worst = (v, k, s)
        fps = [float(j.get("fps", 0)) for t, j in lines if r["tStart"] + 1 <= t <= r["tEnd"]]
        per[str(k)] = {"fps": r1(med(fps)), "fpsMin": r1(min(fps) if fps else None), "p50": r1(pct(old, 50)),
                       "p99": r1(pct(old, 99)), "n": len(old)}
    fv = [p["fps"] for p in per.values() if p["fps"] is not None]
    return {"app": app, "windows": n, "secs": r1(r["secs"]), "ticks": {"sent": sent, "recv": recv, "loss": sent - recv},
            "lat": stats(pooled), "perWindow": per, "fpsMin": r1(min(fv) if fv else None),
            "maxLat": ({"win": worst[1], "atS": r1((worst[2][1] - r["startUs"]) / 1e6), "transport": r1((worst[2][4] - worst[2][3]) / 1000),
                        "recvToApply": r1((worst[2][5] - worst[2][4]) / 1000), "applyToPresent": r1((worst[2][6] - worst[2][5]) / 1000)} if worst else None),
            "fpsMean": r1(statistics.mean(fv) if fv else None),
            "worstP99": r1(max([p["p99"] for p in per.values() if p["p99"] is not None] or [0]) if per else None),
            "cpu": r["cpu"], "gpu_util": r["gpu"], "mem_end": r["mEnd"], "mem_tail": r["mTail"],
            "placement": r["placement"], "window": r["window"]}

def attempt(fn, *a):
    """失敗自動重跑一次；仍失敗就記錄並跳過"""
    for k in (1, 2):
        try: return fn(*a)
        except Exception as e:
            say("  ！%s 失敗（第 %d 次）：%s" % (fn.__name__, k, e))
            FAILS.append({"fn": fn.__name__, "args": [str(x) for x in a[:2]], "try": k, "err": str(e)})
            time.sleep(3)
    return None

# ---------------------------------------------------------------- 彙整
def Q(o, path):
    for k in path.split("."):
        if not isinstance(o, dict): return None
        o = o.get(k)
        if o is None: return None
    return o

# (情境, 指標 key, 路徑, 單位, 越大越好?, 中文)
METRICS = {
    "main": [("first_frame_ms", "first_frame_ms", "ms", False, "啟動到第一幀"), ("first_quote_ms", "first_quote_ms", "ms", False, "啟動到第一筆報價"),
             ("rss10_mb", "idle10.rss_mb", "MB", False, "RSS 第 10 秒"), ("rss60_mb", "steady60.rss_mb", "MB", False, "RSS 第 60 秒"),
             ("footprint60_mb", "steady60.footprint_mb", "MB", False, "實體記憶體（活動監視器）第 60 秒"),
             ("cpu_pct", "cpu_pct", "%", False, "CPU（第 30–60 秒）"), ("fps", "fps", "fps", True, "FPS（第 30–60 秒）"),
             ("gpu_util", "gpu_util", "%", False, "整機 GPU 使用率")],
    "stress": [("cpu_tick_pct", "cpu_tick_pct", "%", False, "CPU（跳價）"), ("cpu_scroll_pct", "cpu_scroll_pct", "%", False, "CPU（跳價＋捲動）"),
               ("tick_fps", "tick_end.fps", "fps", True, "FPS（跳價）"), ("scroll_fps", "done.scrollFps", "fps", True, "FPS（捲動）"),
               ("tick_paint_ms", "tick_end.avgPaintMs", "ms", False, "每次重畫表格耗時（跳價）"),
               ("scroll_paint_ms", "done.avgPaintMs", "ms", False, "每次重畫表格耗時（捲動）"),
               ("scroll_jank", "done.scrollJankCount", "次", False, "捲動卡頓次數"),
               ("rss_tick_mb", "mem_tick.rss_mb", "MB", False, "RSS（跳價末）"), ("footprint_mb", "mem_end.footprint_mb", "MB", False, "實體記憶體（結束時）"),
               ("gpu_tick", "gpu_tick", "%", False, "整機 GPU（跳價）")],
}
def heavy_metrics():
    m = [("heavy_frame_ms", "heavy_frame_ms", "ms", False, "啟動到第一個重圖表幀")]
    for ph in HEAVY_PHASES:
        m += [("%s_fps" % ph, "phases.%s.perf.fps" % ph, "fps", True, "%s：FPS" % ph),
              ("%s_paint_ms" % ph, "phases.%s.perf.avgPaintMs" % ph, "ms", False, "%s：每幀圖表繪製" % ph),
              ("%s_cpu" % ph, "phases.%s.cpu_pct" % ph, "%", False, "%s：CPU" % ph),
              ("%s_rss_mb" % ph, "phases.%s.mem.rss_mb" % ph, "MB", False, "%s：RSS" % ph)]
    m.append(("footprint_mb", "mem_end.footprint_mb", "MB", False, "實體記憶體（結束時）"))
    return m
METRICS["heavy"] = heavy_metrics()
TICK_METRICS = [("lat_p50", "latOldest.p50", "ms", False, "延遲 p50"), ("lat_p99", "latOldest.p99", "ms", False, "延遲 p99"),
                ("lat_max", "latOldest.max", "ms", False, "延遲最大"), ("loss", "ticks.lossFinal", "筆", False, "遺失"),
                ("backlog_ms", "trend.backlog_ms", "ms", False, "積壓（後 5 秒−前 5 秒延遲）"), ("cpu", "cpu", "%", False, "CPU"),
                ("fps", "fps", "fps", True, "FPS 中位數"), ("fpsMin", "fpsMin", "fps", True, "FPS 最低"),
                ("rss_end_mb", "mem_end.rss_mb", "MB", False, "RSS（結束）"), ("footprint_mb", "mem_tail.footprint_mb", "MB", False, "實體記憶體（結束後）"),
                ("gpu_util", "gpu_util", "%", False, "整機 GPU")]
MULTI_METRICS = [("lat_p50", "lat.p50", "ms", False, "延遲 p50（全部視窗）"), ("lat_p99", "lat.p99", "ms", False, "延遲 p99（全部視窗）"),
                 ("worst_p99", "worstP99", "ms", False, "最慢視窗 p99"), ("loss", "ticks.loss", "筆", False, "遺失"),
                 ("fps_min", "fpsMin", "fps", True, "最慢視窗 FPS"), ("fps_mean", "fpsMean", "fps", True, "平均 FPS"),
                 ("cpu", "cpu", "%", False, "CPU"), ("rss_end_mb", "mem_end.rss_mb", "MB", False, "RSS（結束）"),
                 ("footprint_mb", "mem_tail.footprint_mb", "MB", False, "實體記憶體（結束後）"), ("gpu_util", "gpu_util", "%", False, "整機 GPU")]

def groups():
    """(群組 id, 中文標題, 結果清單 key, 過濾, 指標)"""
    g = []
    if "main" in SCEN: g.append(("main", "主視窗（1200×720 點、xq-demofeed 約 40Hz）", "main", None, METRICS["main"]))
    if "stress" in SCEN: g.append(("stress", "5 萬列報價表（每秒 3000 筆跳價＋捲動）", "stress", None, METRICS["stress"]))
    if "heavy" in SCEN: g.append(("heavy", "重圖表（20 疊加＋8 副圖＋1000 筆畫線／%s 層）" % A.layers, "heavy", None, METRICS["heavy"]))
    if "ticks" in SCEN:
        for sc in TICKS: g.append(("ticks-" + sc, "高頻進價 %s" % sc, "ticks", ("scenario", sc), TICK_METRICS))
    if "multi" in SCEN:
        for n in NWIN: g.append(("multi-%dw" % n, "多視窗 ×%d（每秒 1 萬筆）" % n, "multi", ("windows", n), MULTI_METRICS))
    return g

# 勝負判定（2026-10-24 修正）：舊版只拿第一名和第二名比，3 個 app 時
# egui-wgpu 與 egui-glow 差 5% 內就整列標「平手」，GPUI 差很多也看不出來。
# 新版：每個 app 都和最好的那個比，差距在容許範圍內的都算贏（可以多個）；
# 全部都在範圍內才是「平手」。FPS 全部到螢幕更新率 95% 以上也算平手。
SCREEN_MAX_FPS = None
TOL_ABS = {"fps": 1.0, "%": 1.0, "筆": 1.0, "次": 1.0, "ms": 1.0, "MB": 2.0}
def _num(v):
    try: return float(v)
    except (TypeError, ValueError): return None
def judge(meds, hib, unit):
    """meds: {app: 中位數}。回傳 None（不到 2 個 app 有值）、"tie"、或贏家 app 名稱以 "+" 連接（可多個）"""
    if len(meds) < 2: return None
    if unit == "fps" and SCREEN_MAX_FPS and min(meds.values()) >= 0.95 * SCREEN_MAX_FPS: return "tie"
    best_v = (max if hib else min)(meds.values())
    tol = lambda v: max(0.05 * max(abs(v), abs(best_v)), TOL_ABS.get(unit, 0.0))
    winners = [a for a in APPS if a in meds and abs(meds[a] - best_v) <= tol(meds[a])]
    if len(winners) == len(meds): return "tie"
    return "+".join(winners)
def winners_of(w):
    return [] if w in (None, "tie") else w.split("+")

def summarize(ALL):
    rows = []; S = {}
    for gid, title, key, flt, mets in groups():
        S[gid] = {"title": title, "metrics": []}
        for mk, path, unit, hib, zh in mets:
            row = {"group": gid, "metric": mk, "label": zh, "unit": unit, "higher_is_better": hib, "apps": {}}
            for app in apps_in(gid):
                rs = [r for r in ALL[key] if r and r["app"] == app and (flt is None or r.get(flt[0]) == flt[1])]
                vals = [Q(r, path) for r in rs]
                row["apps"][app] = {"median": r1(med(vals)), "runs": [r1(v) if v is not None else None for v in vals]}
            meds = {a: v["median"] for a, v in row["apps"].items() if v["median"] is not None}
            row["winner"] = judge(meds, hib, unit)
            rows.append(row); S[gid]["metrics"].append(row)
    return rows, S

def write_outputs(ALL, rows, S, info, build, t_start):
    res = {"tool": "gpui-xq-demo/bench/mac/xq_bench_mac.py", "version": 1, "started": t_start, "finished": datetime.datetime.now().isoformat(timespec="seconds"),
           "args": vars(A), "runs": RUNS, "apps": APPS, "machine": info, "build": build, "summary": S, "raw": ALL, "failures": FAILS}
    open(os.path.join(OUT, "results.json"), "w", encoding="utf-8").write(anon(json.dumps(res, ensure_ascii=False, indent=1, default=str)))
    import csv
    with open(os.path.join(OUT, "results.csv"), "w", newline="", encoding="utf-8-sig") as f:
        w = csv.writer(f)
        w.writerow(["group", "metric", "label", "unit", "higher_is_better", "app", "median"] + ["run%d" % (i + 1) for i in range(RUNS)] + ["winner"])
        for r in rows:
            for app, v in r["apps"].items():
                w.writerow([r["group"], r["metric"], r["label"], r["unit"], r["higher_is_better"], app, v["median"]] +
                           (v["runs"] + [None] * RUNS)[:RUNS] + [r["winner"]])
    # summary.md：結論在前
    lab = lambda a: APPDEF[a]["label"]
    wins = {a: 0 for a in APPS}; ties = 0; total = 0
    for r in rows:
        if r["winner"] is None: continue
        total += 1
        if r["winner"] == "tie": ties += 1
        else:
            for a in winners_of(r["winner"]): wins[a] += 1
    md = ["# XQ 壓測結果（macOS）：GPUI vs egui", "",
          "> 這份是**在這台 Mac 上實際量到的數字**（%d 輪取中位數）。🏆＝該項最好，或和最好的差距在 5%% 以內（可以多個 🏆）；全部 app 都在 5%% 內才算平手（＝）。勝負項數：每個 🏆 各算一項。" % RUNS, "",
          "## 結論", ""]
    if total:
        order = sorted(APPS, key=lambda a: -wins[a])
        md.append("- 共比較 %d 項：" % total + "、".join("%s 勝 %d 項" % (lab(a), wins[a]) for a in order) + "、平手 %d 項。" % ties)
        for gid, g in S.items():
            ws = [a for m in g["metrics"] for a in winners_of(m["winner"])]
            if ws:
                cnt = {a: ws.count(a) for a in apps_in(gid)}
                top = max(cnt, key=lambda a: cnt[a])
                md.append("- **%s**：%s 較好（%d/%d 項）" % (g["title"], lab(top), cnt[top], len([m for m in g["metrics"] if m["winner"]])))
    else:
        md.append("- 只量到一個 app，沒有可比較的項目。")
    if FAILS: md.append("- ⚠️ 有 %d 次失敗／重跑，細節在 results.json 的 failures。" % len(FAILS))
    maxfps = [s.get("maxFps") for s in info.get("screens", []) if s.get("maxFps")]
    if maxfps: md.append("- 註：螢幕最高更新率 %s Hz；FPS 到這個數字就是頂了（兩邊都到頂＝平手）。" % "/".join(str(x) for x in maxfps))
    md += ["", "## 這台機器", ""]
    for k, zh in (("model", "機型"), ("cpu", "CPU"), ("cores", "邏輯核心"), ("ramGB", "記憶體 GB"), ("macos", "macOS"), ("arch", "架構"),
                  ("rosetta", "Rosetta 轉譯中"), ("lowPowerMode", "低耗電模式"), ("power", "電源")):
        if k in info: md.append("- %s：%s" % (zh, info[k]))
    for gp in info.get("gpus", []) or []:
        md.append("- GPU：%s %s 核心，Metal %s" % (gp.get("name"), gp.get("cores") or "", gp.get("metal") or ""))
    for s in info.get("screens", []):
        md.append("- 螢幕 %d：%s，%s 點 ×%s 倍＝%s 像素，最高 %s Hz%s" % (
            s["index"] + 1, s.get("name"), "%dx%d" % (s["w"], s["h"]), s.get("scale"), s.get("pixels"), s.get("maxFps"), "（主螢幕）" if s["index"] == 0 else ""))
    md.append("- 編譯：%s" % json.dumps(build, ensure_ascii=False))
    for gid, g in S.items():
        cols = apps_in(gid)
        md += ["", "## " + g["title"], "", "| 項目 | " + " | ".join(lab(a) for a in cols) + " |", "|---|" + "---|" * len(cols)]
        for m in g["metrics"]:
            cells = []
            for a in cols:
                v = m["apps"][a]["median"]
                c = "—" if v is None else ("%g %s" % (v, m["unit"]))
                if a in winners_of(m["winner"]): c += " 🏆"
                elif m["winner"] == "tie" and v is not None: c += " ＝"
                cells.append(c)
            md.append("| %s%s | %s |" % (m["label"], "（越高越好）" if m["higher_is_better"] else "", " | ".join(cells)))
    md += stall_md(ALL, lab)
    md += ["", "## 量法", "",
           "- 情境同 Linux／Windows 版（scripts/bench-*.sh、bench/windows）。主視窗／5 萬列／重圖表的行情來自 xq-demofeed（WryFeedHost --demo-ticks 的 Rust 移植，合成資料）；高頻與多視窗來自 xq-tickgen。",
           "- RSS＝ps 的常駐記憶體；實體記憶體＝vmmap 的 Physical footprint（≈ 活動監視器「記憶體」欄，只在量測區間外取）。CPU：100%＝一顆核心。",
           "- GPU＝ioreg 的整機 GPU 使用率（含其他程式），只當參考。FPS 是程式自己回報的每秒畫面數。",
           "- 高頻情境沒有做「點報價列切換」的反應時間（Mac 上要自動點擊得開「輔助使用」權限，這版先不做）。",
           "- 視窗位置由程式自己依 XQ_WIN_RECTS 開在指定螢幕，不需要任何權限；多視窗 ×4：螢幕夠就一個螢幕一個，不夠就平均分配後格狀鋪滿。"]
    open(os.path.join(OUT, "summary.md"), "w", encoding="utf-8").write(anon("\n".join(md) + "\n"))
    if LOGLINES: open(os.path.join(OUT, "bench.log"), "w", encoding="utf-8").write(anon("\n".join(LOGLINES) + "\n"))

def stall_md(ALL, lab):
    """2026-10-25：停頓診斷（GPUI 才有 STALL 行；egui 沒有偵測，不代表沒停）"""
    md = ["", "## 停頓診斷（GPUI；2026-10-25 新增）", "",
          "- GPUI 版有一個背景看門狗：主執行緒超過 120 ms 沒動就記一筆 STALL。**整個程序被暫停**＝看門狗自己也停了（通常是壓測包自己的 vmmap 量記憶體，"
          "在量測區間外、不影響表格數字）；**只有主執行緒卡住**＝背景收行情照常、畫面停住（這才是要找的問題）。",
          "- 只有主執行緒卡住時，會自動用 macOS 內建的 sample 記下卡住當下的呼叫堆疊（stalls 資料夾），請一起傳回。", "",
          "| 情境 | app | 停頓次數（3 輪合計） | 壓測包 vmmap 造成 | 只有主執行緒卡住 | 最長 | 卡住前的階段 |", "|---|---|---|---|---|---|---|"]
    names = {"main": "主視窗", "stress": "5 萬列", "heavy": "重圖表", "ticks": "高頻", "multi": "多視窗"}
    any_row = False
    for key in ("main", "stress", "heavy", "ticks", "multi"):
        for app in APPS:
            rs = [r for r in ALL.get(key, []) if r and r.get("app") == app and r.get("diag")]
            if not rs or not APPDEF[app]["bin"].startswith("gpui"): continue
            subs = sorted({(r.get("scenario") or (("%d 視窗" % r["windows"]) if r.get("windows") else "")) for r in rs})
            for sub in subs:
                rr = [r for r in rs if (r.get("scenario") or (("%d 視窗" % r["windows"]) if r.get("windows") else "")) == sub]
                st = [x for r in rr for x in r["diag"].get("stalls", [])]
                susp = [x for x in st if x.get("procSuspended") or x.get("duringVmmap")]
                main_only = [x for x in st if x not in susp]
                ph = {}
                for x in main_only: ph[x.get("phase")] = ph.get(x.get("phase"), 0) + 1
                md.append("| %s%s | %s | %d | %d | %d | %s | %s |" % (names[key], (" " + sub) if sub else "", lab(app), len(st), len(susp), len(main_only),
                          ("%g ms" % max(x.get("ms", 0) for x in st)) if st else "—", "、".join("%s×%d" % kv for kv in ph.items()) or "—"))
                any_row = True
    if not any_row: md.append("| — | — | — | — | — | — | — |")
    rows = []
    for key in ("ticks", "multi"):
        for r in ALL.get(key, []):
            if r and r.get("maxLat"):
                m = r["maxLat"]
                rows.append("| %s | %s | %s | %s ms | 第 %s 秒 | %s | %s | %s |" % (
                    r.get("scenario") or ("%d 視窗" % r.get("windows", 0)), lab(r["app"]), r.get("run"),
                    (r.get("latOldest") or r.get("lat") or {}).get("max"), m.get("atS"), m.get("transport"), m.get("recvToApply"), m.get("applyToPresent")))
    if rows:
        md += ["", "**高頻／多視窗：最慢那一筆卡在哪一段**（ms；送出→收到＝網路與背景執行緒，收到→套用＝等主執行緒，套用→畫面＝畫這一幀到下一幀開始）", "",
               "| 情境 | app | 輪 | 最慢一筆 | 在開始後 | 送出→收到 | 收到→套用 | 套用→畫面 |", "|---|---|---|---|---|---|---|---|"] + rows
    files = [f for k in ALL for r in ALL[k] if r and r.get("diag") for f in r["diag"].get("samples", [])]
    if files: md += ["", "自動記下的呼叫堆疊：" + "、".join(files)]
    return md

# ---------------------------------------------------------------- 主流程
FAILS = []
def main():
    global STALL_TMP
    t_start = datetime.datetime.now().isoformat(timespec="seconds")
    STALL_TMP = os.path.join(WORK, "stalls"); os.makedirs(STALL_TMP, exist_ok=True)
    for b in ["xq-demofeed", "xq-tickgen"] + sorted(set(APPDEF[a]["bin"] for a in APPS)): bin_path(b)
    if port_busy(): sys.exit("%s 已被佔用：請先關掉 XQ／上次沒結束的測試程式（或用 --port 換埠）" % ADDR)
    mine = set(os.path.basename(APPDEF[a]["bin"]) for a in APPS) | {"xq-demofeed", "xq-tickgen"}
    running = [l for l in sh(["ps", "-A", "-o", "pid=,comm="]).splitlines() if os.path.basename(l.split(None, 1)[-1].strip()) in mine]
    if running and IS_MAC: sys.exit("這些程式正在執行，請先關掉：\n" + "\n".join(running))
    SCR = screens()
    global SCREEN_MAX_FPS
    SCREEN_MAX_FPS = _num(SCR[0].get("maxFps")) if SCR else None
    info = machine_info(SCR)
    build = {}
    try: build = json.load(open(os.path.join(A.bin_dir, "build-info.json")))
    except Exception: pass
    if info.get("rosetta"): say("！目前的終端機在 Rosetta（Intel 轉譯）下執行，數字會偏慢。請用原生終端機重跑。")
    per_app = (62 if "main" in SCEN else 0) + (45 if "stress" in SCEN else 0) + (70 if "heavy" in SCEN else 0) + \
              (42 * len(TICKS) if "ticks" in SCEN else 0) + (42 * len(NWIN) if "multi" in SCEN else 0)
    say("XQ 壓測 macOS 版：%s；情境 %s；%d 輪。預估約 %d 分鐘。" % (",".join(APPS), ",".join(SCEN), RUNS, round((per_app * len([x for x in APPS if "only" not in APPDEF[x]]) + (70 if "heavy" in SCEN else 0) * len([x for x in APPS if x in ("gpui-quads", "gpui-norelief")]) + 85 * len([x for x in APPS if x == "gpui-nofix"])) * RUNS * 1.1 / 60)))
    say("螢幕：" + "；".join("%s %dx%d 點 ×%s（%s Hz）" % (s.get("name"), s["w"], s["h"], s.get("scale"), s.get("maxFps")) for s in SCR))
    say("測試期間請不要動滑鼠鍵盤、不要把終端機蓋在測試視窗上面。結果：%s" % OUT)
    ALL = {"main": [], "stress": [], "heavy": [], "ticks": [], "multi": []}
    try:
        for run in range(1, RUNS + 1):
            for app in APPS:  # 每一輪 app 交錯跑，降低背景負載變化的影響
                if "main" in SCEN and runs_in(app, "main"):
                    say("第 %d/%d 輪 %s 主視窗…" % (run, RUNS, app)); r = attempt(run_main, app, SCR)
                    if r: r["diag"] = LAST_DIAG[0]
                    if r: r["run"] = run; ALL["main"].append(r); say("  第一幀 %s ms、報價 %s ms、RSS %s MB、CPU %s%%、FPS %s" % (r["first_frame_ms"], r["first_quote_ms"], r["steady60"]["rss_mb"], r["cpu_pct"], r["fps"]))
                if "stress" in SCEN and runs_in(app, "stress"):
                    say("第 %d/%d 輪 %s 5 萬列…" % (run, RUNS, app)); r = attempt(run_stress, app, SCR)
                    if r: r["diag"] = LAST_DIAG[0]
                    if r: r["run"] = run; ALL["stress"].append(r); say("  CPU 跳價 %s%%／捲動 %s%%，FPS 捲動 %s" % (r["cpu_tick_pct"], r["cpu_scroll_pct"], Q(r, "done.scrollFps")))
                if "heavy" in SCEN and runs_in(app, "heavy"):
                    say("第 %d/%d 輪 %s 重圖表（%s 層）…" % (run, RUNS, app, A.layers)); r = attempt(run_heavy, app, SCR)
                    if r: r["diag"] = LAST_DIAG[0]
                    if r: r["run"] = run; ALL["heavy"].append(r); say("  " + "、".join("%s %s fps" % (p, Q(r, "phases.%s.perf.fps" % p)) for p in HEAVY_PHASES))
                if "ticks" in SCEN and runs_in(app, "ticks"):
                    for sc in TICKS:
                        if not runs_sub(app, sc): continue
                        say("第 %d/%d 輪 %s 高頻 %s…" % (run, RUNS, app, sc)); r = attempt(run_ticks, app, sc, SCR)
                        if r: r["diag"] = LAST_DIAG[0]
                        if r: r["run"] = run; ALL["ticks"].append(r); say("  延遲 p50 %s／p99 %s ms、遺失 %s、CPU %s%%、FPS %s" % (r["latOldest"]["p50"], r["latOldest"]["p99"], r["ticks"]["lossFinal"], r["cpu"], r["fps"]))
                        time.sleep(1)
                if "multi" in SCEN and runs_in(app, "multi"):
                    for n in NWIN:
                        if not runs_sub(app, n): continue
                        say("第 %d/%d 輪 %s 多視窗 ×%d…" % (run, RUNS, app, n)); r = attempt(run_multi, app, n, SCR)
                        if r: r["diag"] = LAST_DIAG[0]
                        if r: r["run"] = run; ALL["multi"].append(r); say("  延遲 p99 %s ms、最慢視窗 FPS %s、CPU %s%%" % (r["lat"]["p99"], r["fpsMin"], r["cpu"]))
                        time.sleep(1)
            stop_feed()
            rows, S = summarize(ALL); write_outputs(ALL, rows, S, info, build, t_start)  # 每輪存一次，中斷也有資料
    except KeyboardInterrupt:
        say("中斷了；已完成的部分照樣存檔")
    finally:
        stop_feed(); GPU.stop()
    rows, S = summarize(ALL); write_outputs(ALL, rows, S, info, build, t_start)
    shutil.rmtree(WORK, ignore_errors=True)
    if not A.no_zip:
        z = OUT.rstrip("/") + ".zip"
        with zipfile.ZipFile(z, "w", zipfile.ZIP_DEFLATED) as zf:
            for root, _, files in os.walk(OUT):
                for f in files:
                    p = os.path.join(root, f); zf.write(p, os.path.relpath(p, os.path.dirname(OUT)))
        say("完成。請把這個檔案傳回來：%s" % z)
        if IS_MAC: subprocess.run(["open", "-R", z])
    else:
        say("完成：%s" % OUT)

def resummarize(path):
    """用舊的 results.json 重新算勝負（例如 2026-10-23 那份 summary.md 的平手判定有錯）"""
    global APPS, RUNS, SCEN, TICKS, NWIN, SCREEN_MAX_FPS, FAILS
    res = json.load(open(path, encoding="utf-8"))
    a = res.get("args") or {}
    APPS = list(res["apps"]); RUNS = res["runs"]
    SCEN = [x for x in (a.get("scenarios") or "main,stress,heavy,ticks,multi").split(",") if x]
    TICKS = sorted({r.get("scenario") for r in res["raw"].get("ticks", []) if r}, key=lambda x: (len(x), x)) or TICKS
    NWIN = sorted({r.get("windows") for r in res["raw"].get("multi", []) if r}) or NWIN
    A.layers = a.get("layers", A.layers)
    scr = (res.get("machine") or {}).get("screens") or []
    SCREEN_MAX_FPS = _num(scr[0].get("maxFps")) if scr else None
    FAILS = res.get("failures") or []
    rows, S = summarize(res["raw"])
    write_outputs(res["raw"], rows, S, res.get("machine") or {}, res.get("build") or {}, res.get("started"))
    shutil.rmtree(WORK, ignore_errors=True)
    say("重新計算完成：%s" % OUT)

if __name__ == "__main__":
    if A.resummarize:
        if not A.out: OUT = os.path.dirname(os.path.abspath(A.resummarize))
        resummarize(A.resummarize)
    else:
        main()
