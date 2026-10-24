#!/usr/bin/env python3
"""GPUI Android 記憶體 A/B：同一個 APK，用環境變數切字型載入方式；每種 N 次，量 dumpsys meminfo。"""
import subprocess, time, re, json, os, sys, argparse
P = argparse.ArgumentParser(); P.add_argument("--n", type=int, default=3); P.add_argument("--out", required=True)
P.add_argument("--modes", default="read,mmap,minimal"); A = P.parse_args(); os.makedirs(A.out, exist_ok=True)
PKG, ACT = "tw.sysjust.xq.gpui", "dev.gpui.mobile.GpuiInputActivity"
MIN = "Roboto-Regular.ttf,RobotoMono-Regular.ttf,DroidSansMono.ttf,NotoSansCJK-Regular.ttc,NotoColorEmojiFlags.ttf"
MODES = {"read": {"GPUI_ANDROID_FONT_LOAD": "read"}, "mmap": {}, "minimal": {"GPUI_ANDROID_FONTS": MIN}}
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def sh(c, t=240):
    try: return subprocess.run(["adb", "shell", c], capture_output=True, text=True, timeout=t).stdout
    except subprocess.TimeoutExpired: return ""
def meminfo():
    for _ in range(3):
        s = sh(f"dumpsys meminfo {PKG}")
        m = re.search(r"TOTAL PSS:\s+(\d+)", s)
        if m:
            g = lambda k: (re.search(rf"^\s*{k}\s+(\d+)", s, re.M) or [0, 0])[1]
            return {"pssMb": round(int(m.group(1)) / 1024, 1), "nativeHeapMb": round(int(g("Native Heap")) / 1024, 1),
                    "ttfMmapMb": round(int(g(r"\.ttf mmap")) / 1024, 1), "otherMmapMb": round(int(g("Other mmap")) / 1024, 1),
                    "glMtrackMb": round(int(g("GL mtrack")) / 1024, 1), "eglMtrackMb": round(int(g("EGL mtrack")) / 1024, 1),
                    "rssMb": round(int((re.search(r"TOTAL RSS:\s+(\d+)", s) or [0, 0])[1]) / 1024, 1)}, s
    return None, ""
def wait(pat, to):
    t0 = time.time()
    while time.time() - t0 < to:
        if re.search(pat, subprocess.run(["adb", "logcat", "-d", "-s", "xq:*", "gpui-android:*"], capture_output=True, text=True, timeout=120).stdout): return True
        time.sleep(3)
    return False
for i in range(1, A.n + 1):
    for mode in A.modes.split(","):
        env = {"XQ_BENCH": "1", "XQ_METRICS": "1", **MODES[mode]}
        sh(f"am force-stop {PKG}"); time.sleep(3)
        sh("; ".join([f"setprop debug.xq.e{k} ''" for k in range(10)] + [f"setprop debug.xq.e{k} {a}={b}" for k, (a, b) in enumerate(env.items())]))
        subprocess.run(["adb", "logcat", "-c"], timeout=60)
        sh(f"am start -n {PKG}/{ACT} >/dev/null")
        ok = wait("BENCH first-quote", 240); time.sleep(20)
        lst, raw1 = meminfo()
        if i == 1:
            open(f"{A.out}/{mode}-list.png", "wb").write(subprocess.run(["adb", "exec-out", "screencap", "-p"], capture_output=True, timeout=120).stdout)
        sh("input tap 200 300"); time.sleep(25)
        cht, raw2 = meminfo()
        lc = subprocess.run(["adb", "logcat", "-d", "-s", "gpui-android:*"], capture_output=True, text=True, timeout=120).stdout
        fonts = re.findall(r"system fonts: .*", lc)
        if i == 1:
            open(f"{A.out}/{mode}-chart.png", "wb").write(subprocess.run(["adb", "exec-out", "screencap", "-p"], capture_output=True, timeout=120).stdout)
            open(f"{A.out}/{mode}-meminfo-chart.txt", "w").write(raw2)
        r = {"iter": i, "mode": mode, "ok": ok, "list": lst, "chart": cht, "fonts": fonts[-1] if fonts else None}
        log(json.dumps(r, ensure_ascii=False)); open(f"{A.out}/mem.jsonl", "a").write(json.dumps(r, ensure_ascii=False) + "\n")
sh(f"am force-stop {PKG}")
log("DONE")
