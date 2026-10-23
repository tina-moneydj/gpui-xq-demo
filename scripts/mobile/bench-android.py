#!/usr/bin/env python3
"""手機版（Android）壓測：egui（glow／wgpu）vs GPUI（wgpu-Vulkan），在模擬器上跑同一組情境。

用法：bench-android.py --apps egui-glow,egui-wgpu,gpui --runs 3 --phases touch,heavy,ticks --out DIR
需求：adb 已連上模擬器（ANDROID_SERIAL）、adb root、/data/local/tmp/mt（tools/mt.c，多點觸控注入）、
      兩個 APK 已安裝（egui 用含 wgpu 的版本：XQ_RENDERER 切換）、xq-tickgen（--tickgen）、行情引擎 127.0.0.1:47631。
情境：
  touch：冷啟動（啟動→第一幀／第一筆報價）→ 5 萬列報價 idle 30 秒（CPU／PSS／FPS）→ 列表甩動 15 次
         → 點一列進走勢圖 → 平移 10 次 → 雙指縮放 6 次 → 長按十字線 3 次（每段 FRAMES fps／jank／最長幀、CPU）
  heavy：XQ_CHART_STRESS（1000 條畫線／100 層、日線、壓測指標組）＋ XQ_IND_BENCH=20（指標純計算）
  ticks：xq-tickgen 1k／10k／100k 筆/秒，各 20 秒；延遲 = 畫面 present − 送出（扣主機/模擬器時鐘差）、
         以及只用模擬器時鐘的 收到→present
CPU：app 行程 utime+stime（模擬器內）＋ 主機 emulator 行程（含 GL/Vulkan 轉譯與軟體光柵化）。
"""
import argparse, json, os, re, statistics, subprocess, sys, time

AP = argparse.ArgumentParser()
AP.add_argument("--apps", default="egui-glow,egui-wgpu,gpui")
AP.add_argument("--runs", type=int, default=3)
AP.add_argument("--phases", default="touch,heavy,ticks")
AP.add_argument("--rates", default="1000,10000,100000")
AP.add_argument("--out", required=True)
AP.add_argument("--tickgen", required=True)
AP.add_argument("--tick-port", type=int, default=47791)
AP.add_argument("--emu-pid", type=int, required=True)
A = AP.parse_args()
os.makedirs(A.out, exist_ok=True)
MT = "/data/local/tmp/mt /dev/input/event2"
APPS = {
    "egui-glow": ("tw.sysjust.xq.egui", "android.app.NativeActivity", {"XQ_RENDERER": "glow"}),
    "egui-wgpu": ("tw.sysjust.xq.egui", "android.app.NativeActivity", {"XQ_RENDERER": "wgpu"}),
    "gpui": ("tw.sysjust.xq.gpui", "dev.gpui.mobile.GpuiInputActivity", {}),
}
HZ = os.sysconf("SC_CLK_TCK")
RESULTS = os.path.join(A.out, "results.jsonl")

def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)

def adb(*args, timeout=120, check=False):
    for i in range(3):
        try:
            r = subprocess.run(["adb", *args], capture_output=True, text=True, timeout=timeout)
            if r.returncode == 0 or not check:
                return r.stdout
        except subprocess.TimeoutExpired:
            log("adb timeout", args[:3])
        time.sleep(2)
    return ""

def sh(cmd, timeout=180):
    return adb("shell", cmd, timeout=timeout)

def set_props(env):
    cmds = [f"setprop debug.xq.e{i} ''" for i in range(10)]
    for i, (k, v) in enumerate(env.items()):
        cmds.append(f"setprop debug.xq.e{i} {k}={v}")
    sh("; ".join(cmds))

def logcat():
    return adb("logcat", "-d", "-v", "epoch", "-s", "xq:*", timeout=180)

def lines(tag, text=None):
    text = logcat() if text is None else text
    out = []
    for ln in text.splitlines():
        m = re.match(r"\s*(\d+\.\d+)\s.*?xq\s*:\s(.*)$", ln)
        if m and m.group(2).startswith(tag):
            out.append((float(m.group(1)), m.group(2)))
    return out

def jl(s, tag):
    try:
        return json.loads(s[len(tag):])
    except Exception:
        return None

def wait_for(pat, timeout):
    t0 = time.time()
    while time.time() - t0 < timeout:
        txt = logcat()
        if re.search(pat, txt):
            return txt
        time.sleep(3)
    return None

def pid_of(pkg):
    return sh(f"pidof {pkg}").strip().split(" ")[0]

def guest_cpu(pid):
    s = sh(f"cat /proc/{pid}/stat")
    try:
        f = s.rsplit(")", 1)[1].split()
        return (int(f[11]) + int(f[12])) / 100.0
    except Exception:
        return None

def host_cpu():
    try:
        f = open(f"/proc/{A.emu_pid}/stat").read().rsplit(")", 1)[1].split()
        return (int(f[11]) + int(f[12])) / HZ
    except Exception:
        return None

def pss_mb(pkg):
    s = sh(f"dumpsys meminfo {pkg}", timeout=240)
    m = re.search(r"TOTAL PSS:\s+(\d+)", s) or re.search(r"^\s*TOTAL\s+(\d+)", s, re.M)
    return round(int(m.group(1)) / 1024, 1) if m else None

def host_load():
    return float(open("/proc/loadavg").read().split()[0])

def launch(app, env):
    pkg, act, extra = APPS[app]
    sh(f"am force-stop {pkg}")
    time.sleep(2)
    set_props({**env, **extra})
    adb("logcat", "-c")
    out = sh(f"date +%s%N; am start -n {pkg}/{act} >/dev/null")
    try:
        return int(out.split()[0]) / 1e9
    except Exception:
        return time.time()

def frames_between(t_a, t_b, text=None):
    fr = []
    for ts, s in lines("FRAMES|", text):
        d = jl(s, "FRAMES|")
        if d and t_a <= ts <= t_b:
            fr.append(d)
    return fr

def summarize_frames(fr):
    if not fr:
        return {}
    busy = [f for f in fr if f["fps"] > 0]
    return {
        "secs": len(fr),
        "fpsMean": round(statistics.mean(f["fps"] for f in fr), 2),
        "fpsMedian": statistics.median(f["fps"] for f in fr),
        "jankPerSec": round(statistics.mean(f["jank"] for f in fr), 2),
        "maxFrameMs": max(f["maxMs"] for f in fr),
        "rssMbEnd": fr[-1].get("rssMb"),
        "indPatchUsMean": round(statistics.mean([f["indPatchUs"] for f in fr if f.get("indPatch")]), 2) if any(f.get("indPatch") for f in fr) else None,
        "indPatchPerSec": round(statistics.mean(f.get("indPatch", 0) for f in fr), 2),
        "indFullMsMean": round(statistics.mean([f["indFullMs"] for f in fr if f.get("indFull")]), 3) if any(f.get("indFull") for f in fr) else None,
    }

def guest_now():
    try:
        return int(sh("date +%s%N").split()[0]) / 1e9
    except Exception:
        return time.time()

def measured(app, name, pid, action, run):
    """執行 action（阻塞），量 CPU／FRAMES"""
    pkg = APPS[app][0]
    g0, h0, ta = guest_cpu(pid), host_cpu(), guest_now()
    w0 = time.time()
    action()
    wall = time.time() - w0
    g1, h1, tb = guest_cpu(pid), host_cpu(), guest_now()
    fr = frames_between(ta, tb)
    r = {"app": app, "run": run, "phase": name, "wallS": round(wall, 1), "load1": host_load(),
         "appCpuS": round(g1 - g0, 2) if None not in (g0, g1) else None,
         "appCpuPct": round((g1 - g0) / wall * 100, 1) if None not in (g0, g1) else None,
         "appCpuSPerMin": round((g1 - g0) / wall * 60, 2) if None not in (g0, g1) else None,
         "emuCpuPct": round((h1 - h0) / wall * 100, 1) if None not in (h0, h1) else None,
         **summarize_frames(fr)}
    return r

def save(r):
    log(json.dumps(r, ensure_ascii=False))
    with open(RESULTS, "a") as f:
        f.write(json.dumps(r, ensure_ascii=False) + "\n")

def shot(name):
    with open(os.path.join(A.out, name), "wb") as f:
        f.write(subprocess.run(["adb", "exec-out", "screencap", "-p"], capture_output=True, timeout=120).stdout)

def phase_touch(app, run):
    pkg = APPS[app][0]
    t0 = launch(app, {"XQ_BENCH": "1", "XQ_METRICS": "1", "XQ_STRESS_LIST": "1"})
    txt = wait_for(r"BENCH first-quote", 240)
    if not txt:
        save({"app": app, "run": run, "phase": "startup", "error": "no first-quote in 240s"})
        open(os.path.join(A.out, f"{app}-r{run}-startup-fail.log"), "w").write(logcat())
        return
    ff = [ts for ts, s in lines("BENCH first-frame", txt)]
    fq = [ts for ts, s in lines("BENCH first-quote", txt)]
    pid = pid_of(pkg)
    save({"app": app, "run": run, "phase": "startup", "firstFrameMs": round((ff[0] - t0) * 1000) if ff else None,
          "firstQuoteMs": round((fq[0] - t0) * 1000) if fq else None, "load1": host_load()})
    time.sleep(10)
    r = measured(app, "idle-list", pid, lambda: time.sleep(30), run)
    r["pssMb"] = pss_mb(pkg)
    save(r)
    if run == 1: shot(f"{app}-list.png")
    save(measured(app, "fling", pid, lambda: sh(f"{MT} 15 1500 swipe 540 1900 540 700 120", 300), run))
    if run == 1: shot(f"{app}-fling.png")
    sh("input tap 200 300")
    time.sleep(10)
    save(measured(app, "chart-idle", pid, lambda: time.sleep(15), run))
    save(measured(app, "pan", pid, lambda: sh(f"{MT} 10 300 drag 250 1000 850 1000 600", 300), run))
    save(measured(app, "pinch", pid, lambda: sh("; ".join([f"{MT} 1 400 pinch 540 1100 200 700 500; {MT} 1 400 pinch 540 1100 700 200 500"] * 3), 300), run))
    if run == 1: shot(f"{app}-pinch.png")
    save(measured(app, "crosshair", pid, lambda: sh(f"{MT} 3 500 hold 540 1000 900 900 800 1500", 300), run))
    r = {"app": app, "run": run, "phase": "chart-mem", "pssMb": pss_mb(pkg)}
    save(r)
    open(os.path.join(A.out, f"{app}-r{run}-touch.log"), "w").write(logcat())
    sh(f"am force-stop {pkg}")

def phase_heavy(app, run):
    pkg = APPS[app][0]
    launch(app, {"XQ_BENCH": "1", "XQ_METRICS": "1", "XQ_CHART_STRESS": "1", "XQ_CHART_DRAWINGS": "1000",
                 "XQ_CHART_LAYERS": "100", "XQ_PERIOD": "D", "XQ_IND_BENCH": "20", "XQ_CHART_PHASE_MS": "8000"})
    txt = wait_for(r"BENCH first-frame", 240)
    pid = pid_of(pkg)
    g0, h0, w0 = guest_cpu(pid), host_cpu(), time.time()
    pss_peak = None
    t_end = time.time() + 900
    txt = ""
    while time.time() < t_end:
        txt = logcat()
        if '"phase":"done"' in txt:
            break
        if pss_peak is None and '"phase":"crosshair"' in txt:
            pss_peak = pss_mb(pkg)
        time.sleep(10)
    wall = time.time() - w0
    g1, h1 = guest_cpu(pid), host_cpu()
    open(os.path.join(A.out, f"{app}-r{run}-heavy.log"), "w").write(txt)
    if run == 1: shot(f"{app}-heavy.png")
    phases = [jl(s, "CHARTPERF|") for _, s in lines("CHARTPERF|", txt)]
    ind = [jl(s, "INDPERF|") for _, s in lines("INDPERF|", txt)]
    save({"app": app, "run": run, "phase": "heavy", "done": '"phase":"done"' in txt, "wallS": round(wall, 1),
          "appCpuPct": round((g1 - g0) / wall * 100, 1) if None not in (g0, g1) else None,
          "emuCpuPct": round((h1 - h0) / wall * 100, 1) if None not in (h0, h1) else None,
          "pssMb": pss_peak, "load1": host_load(), "indperf": ind[0] if ind else None,
          "chartperf": [p for p in phases if p]})
    sh(f"am force-stop {pkg}")

def clock_offset():
    best = None
    for _ in range(8):
        a = time.time()
        g = guest_now()
        b = time.time()
        if best is None or (b - a) < best[0]:
            best = (b - a, g - (a + b) / 2)
    return best  # (rtt s, guest − host s)

def phase_ticks(app, run):
    pkg = APPS[app][0]
    for rate in [int(x) for x in A.rates.split(",")]:
        tg_log = open(os.path.join(A.out, f"{app}-r{run}-tick{rate}-gen.jsonl"), "w")
        tg = subprocess.Popen([A.tickgen, "--addr", f"127.0.0.1:{A.tick_port}", "--rate", str(rate), "--duration", "20",
                               "--settle", "3", "--warmup", "3"], stdout=tg_log, stderr=subprocess.STDOUT)
        try:
            adb("reverse", "tcp:47631", f"tcp:{A.tick_port}")
            rtt0, off0 = clock_offset()
            launch(app, {"XQ_BENCH": "1", "XQ_METRICS": "1", "XQ_TICK_BENCH": "1", "XQ_PERIOD": "D"})
            pid = None
            t_end = time.time() + 240
            g0 = h0 = w0 = None
            while time.time() < t_end:
                tg_log.flush()
                gen = open(tg_log.name).read()
                if g0 is None and '"ev":"start"' in gen:
                    pid = pid_of(pkg); g0, h0, w0 = guest_cpu(pid), host_cpu(), time.time()
                if '"ev":"end"' in gen:
                    break
                if tg.poll() is not None:
                    break
                time.sleep(2)
            time.sleep(4)
            wall = time.time() - w0 if w0 else None
            g1, h1 = (guest_cpu(pid), host_cpu()) if pid else (None, None)
            rtt1, off1 = clock_offset()
            txt = logcat()
            open(os.path.join(A.out, f"{app}-r{run}-tick{rate}.log"), "w").write(txt)
            gen = [json.loads(l) for l in open(tg_log.name) if l.startswith("{")]
            start = next((g["unixUs"] for g in gen if g.get("ev") == "start"), None)
            end = next((g for g in gen if g.get("ev") == "end"), None)
            off_us = (off0 + off1) / 2 * 1e6
            tb = [jl(s, "TICKBENCH|") for _, s in lines("TICKBENCH|", txt)]
            tb = [t for t in tb if t]
            # 時鐘差：adb date 在 TCG 下來回近 1 秒、不準；改用「所有樣本 recv−sent 的最小值」當差值
            # （假設最快那包傳輸≈0，所以下面 e2e 是下界）；adb 量的留著當參考
            diffs = [s[4] - s[3] for t in tb for s in t["lat"]]
            off_adb_us = off_us
            if diffs:
                off_us = min(diffs)
            lat_new, lat_old, g_only = [], [], []
            for t in tb:
                for s in t["lat"]:
                    fseq, fsent, lseq, lsent, recv, applied, present = s
                    if start and lsent < start + 3e6:  # 開始後前 3 秒當暖身
                        continue
                    lat_new.append((present - off_us - lsent) / 1000)
                    lat_old.append((present - off_us - fsent) / 1000)
                    g_only.append((present - recv) / 1000)
            def pct(v, q):
                v = sorted(v); return round(v[min(len(v) - 1, int(len(v) * q))], 1) if v else None
            last = tb[-1] if tb else {}
            fps = [t["fps"] for t in tb[3:]] if len(tb) > 3 else []
            save({"app": app, "run": run, "phase": f"tick{rate}", "sent": end.get("sent") if end else None,
                  "rowsIn": last.get("rowsIn"), "applied": last.get("applied"), "samples": len(lat_new),
                  "latNewestP50": pct(lat_new, .5), "latNewestP95": pct(lat_new, .95),
                  "latOldestP50": pct(lat_old, .5), "latOldestP95": pct(lat_old, .95),
                  "recvToPresentP50": pct(g_only, .5), "recvToPresentP95": pct(g_only, .95),
                  "fpsMean": round(statistics.mean(fps), 2) if fps else None, "rssMbEnd": last.get("rssMb"),
                  "appCpuPct": round((g1 - g0) / wall * 100, 1) if None not in (g0, g1, wall) else None,
                  "emuCpuPct": round((h1 - h0) / wall * 100, 1) if None not in (h0, h1, wall) else None,
                  "clockOffsetMs": round(off_us / 1000, 1), "clockOffsetAdbMs": round(off_adb_us / 1000, 1), "clockDriftMs": round((off1 - off0) * 1000, 1),
                  "clockRttMs": round(max(rtt0, rtt1) * 1000, 1), "load1": host_load()})
        finally:
            sh(f"am force-stop {pkg}")
            if tg.poll() is None:
                tg.terminate()
                try: tg.wait(10)
                except Exception: tg.kill()
            tg_log.close()
            adb("reverse", "tcp:47631", "tcp:47631")

for run in range(1, A.runs + 1):
    for app in A.apps.split(","):
        for ph in A.phases.split(","):
            log(f"=== run {run} {app} {ph}")
            try:
                {"touch": phase_touch, "heavy": phase_heavy, "ticks": phase_ticks}[ph](app, run)
            except Exception as e:
                save({"app": app, "run": run, "phase": ph, "error": repr(e)})
log("ALL DONE")
