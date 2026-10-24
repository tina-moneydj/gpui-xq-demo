#!/usr/bin/env python3
"""GPUI heavy 圖表凍結重現：重複 heavy 壓測，全量 logcat，停住時抓 debuggerd -b。"""
import subprocess, time, sys, os, re, json, argparse
P = argparse.ArgumentParser()
P.add_argument("--iters", type=int, default=5)
P.add_argument("--out", required=True)
P.add_argument("--tag", default="base")
P.add_argument("--extra", default="")  # KEY=VAL,KEY=VAL
P.add_argument("--cap", type=int, default=420)
P.add_argument("--stall", type=int, default=40)
P.add_argument("--pause-at", default="")  # CHARTPHASE 名稱：一開始就 SIGSTOP 模擬器
P.add_argument("--pause-s", type=int, default=0)
P.add_argument("--emu-pid", type=int, default=0)
P.add_argument("--kill-sysui", action="store_true")
P.add_argument("--clock-step", type=int, default=0)  # 在 pause-at 階段開始時把 guest 牆鐘撥 N 秒
A = P.parse_args()
os.makedirs(A.out, exist_ok=True)
PKG, ACT = "tw.sysjust.xq.gpui", "dev.gpui.mobile.GpuiInputActivity"
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def sh(c, t=180):
    try: return subprocess.run(["adb", "shell", c], capture_output=True, text=True, timeout=t).stdout
    except subprocess.TimeoutExpired: log("adb timeout", c[:40]); return ""
env = {"XQ_BENCH": "1", "XQ_METRICS": "1", "XQ_CHART_STRESS": "1", "XQ_CHART_DRAWINGS": "1000", "XQ_CHART_LAYERS": "100",
       "XQ_PERIOD": "D", "XQ_IND_BENCH": "20", "XQ_CHART_PHASE_MS": "8000", "XQ_PACER_LOG": "1"}
for kv in filter(None, A.extra.split(",")):
    k, v = kv.split("=", 1); env[k] = v
assert len(env) <= 10, env
summary = []
for it in range(1, A.iters + 1):
    sh(f"am force-stop {PKG}"); time.sleep(2)
    sh("; ".join([f"setprop debug.xq.e{i} ''" for i in range(10)] + [f"setprop debug.xq.e{i} {k}={v}" for i, (k, v) in enumerate(env.items())]))
    subprocess.run(["adb", "logcat", "-c"], timeout=60)
    lf = os.path.join(A.out, f"{A.tag}-i{it}.logcat")
    lc = subprocess.Popen(["adb", "logcat", "-v", "epoch"], stdout=open(lf, "w"), stderr=subprocess.STDOUT)
    sh(f"am start -n {PKG}/{ACT} >/dev/null")
    t0 = time.time(); res = "timeout"; last_frames = None; last_change = time.time(); dumped = False; pid = ""; paused = False
    while time.time() - t0 < A.cap:
        time.sleep(5)
        txt = open(lf, errors="replace").read()
        if not pid: pid = sh(f"pidof {PKG}").strip()
        if '"phase":"done"' in txt: res = "done"; break
        if A.pause_at and not paused and f"CHARTPHASE|{A.pause_at}|start" in txt:
            if A.clock_step:
                log(f"iter {it}: guest clock step {A.clock_step}s", sh(f"date @$(( $(date +%s) + ({A.clock_step}) ))").strip())
            if A.kill_sysui:
                sp = sh("pidof com.android.systemui").strip()
                log(f"iter {it}: kill systemui pid {sp}", sh(f"kill {sp}") if sp.isdigit() else "no pid")
            if A.pause_s:
                log(f"iter {it}: SIGSTOP emulator {A.emu_pid} for {A.pause_s}s"); os.kill(A.emu_pid, 19); time.sleep(A.pause_s); os.kill(A.emu_pid, 18)
            paused = True; last_change = time.time(); log("SIGCONT")
        m = re.findall(r'PACER\|(\{.*\})', txt)
        fr = json.loads(m[-1])["frames"] if m else None
        if fr != last_frames: last_frames, last_change = fr, time.time()
        if fr is not None and time.time() - last_change > A.stall and not dumped:
            res = "stall"
            log(f"iter {it}: STALL frames={fr} pid={pid}; dumping")
            open(os.path.join(A.out, f"{A.tag}-i{it}-debuggerd.txt"), "w").write(sh(f"debuggerd -b {pid}", 240))
            open(os.path.join(A.out, f"{A.tag}-i{it}-threads.txt"), "w").write(sh(f"for t in /proc/{pid}/task/*; do echo $t $(cat $t/comm) $(cat $t/stat | cut -d' ' -f3) $(cat $t/wchan); done"))
            open(os.path.join(A.out, f"{A.tag}-i{it}-window.txt"), "w").write(sh(f"dumpsys window windows | grep -A30 {PKG}") + sh("dumpsys SurfaceFlinger --list"))
            open(os.path.join(A.out, f"{A.tag}-i{it}-state.txt"), "w").write(sh(f"dumpsys activity processes | grep -B2 -A30 'ProcessRecord.*{PKG}'") + sh("dumpsys input | grep -A3 -i focused") + sh(f"cat /proc/{pid}/cgroup; cat /proc/{pid}/status | grep State"))
            dumped = True
            time.sleep(20); break
    lc.terminate()
    txt = open(lf, errors="replace").read()
    phases = re.findall(r"CHARTPHASE\|(\w+)\|start", txt)
    r = {"iter": it, "tag": A.tag, "result": res, "secs": round(time.time() - t0), "lastPhase": phases[-1] if phases else None, "frames": last_frames}
    log(json.dumps(r)); summary.append(r)
    with open(os.path.join(A.out, "summary.jsonl"), "a") as f: f.write(json.dumps(r) + "\n")
sh(f"am force-stop {PKG}")
log("DONE", sum(r["result"] == "stall" for r in summary), "stalls /", len(summary))
