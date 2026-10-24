#!/usr/bin/env python3
"""失焦但仍看得到：在 app 上面開一個半透明的系統對話框（設定的「網際網路」面板）25 秒，
app 收到 Pause／LostFocus 但還在畫面上（後面看得到）。比較對話框開著時 PACER frames 有沒有增加。"""
import subprocess, time, re, json, sys
PKG, ACT = "tw.sysjust.xq.gpui", "dev.gpui.mobile.GpuiInputActivity"
tag, n, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
def sh(c, t=240):
    try: return subprocess.run(["adb", "shell", c], capture_output=True, text=True, timeout=t).stdout
    except subprocess.TimeoutExpired: return ""
def lc():
    try: return subprocess.run(["adb", "logcat", "-d", "-v", "epoch", "-s", "gpui-android:*", "xq:*"], capture_output=True, text=True, timeout=240).stdout
    except subprocess.TimeoutExpired: return ""
def pacer(t):
    m = re.findall(r'PACER\|(\{.*\})', t); return json.loads(m[-1]) if m else None
for i in range(1, n + 1):
    sh(f"am force-stop {PKG}"); time.sleep(3)
    env = {"XQ_BENCH": "1", "XQ_METRICS": "1", "XQ_PACER_LOG": "1"}
    sh("; ".join([f"setprop debug.xq.e{k} ''" for k in range(10)] + [f"setprop debug.xq.e{k} {a}={b}" for k, (a, b) in enumerate(env.items())]))
    subprocess.run(["adb", "logcat", "-c"], timeout=60); sh(f"am start -n {PKG}/{ACT} >/dev/null")
    t0 = time.time()
    while time.time() - t0 < 300 and "BENCH first-quote" not in lc(): time.sleep(5)
    time.sleep(10); a = pacer(lc())
    sh("am start -a android.settings.panel.action.INTERNET_CONNECTIVITY >/dev/null"); time.sleep(10)
    win = sh(f"dumpsys window windows | grep -A40 'Window{{.*{PKG}'")
    vis = "isVisible=true" in win and "mHasSurface=true" in win
    top = sh("dumpsys window | grep mCurrentFocus").strip()[-70:]
    time.sleep(15); b = pacer(lc())
    sh("input keyevent KEYCODE_BACK"); time.sleep(15); c = pacer(lc()); t = lc()
    open(f"{out}/{tag}-i{i}.log", "w").write("\n".join(l for l in t.splitlines() if "PACER" in l or "MainEvent" in l or "deferred" in l))
    r = {"tag": tag, "iter": i, "framesBefore": a and a["frames"], "framesDuringDialog25s": (b and a) and b["frames"] - a["frames"],
         "canDrawDuringDialog": b and b["canDraw"], "appVisibleBehindDialog": vis, "focusDuringDialog": top,
         "framesAfter15s": (c and b) and c["frames"] - b["frames"]}
    print(json.dumps(r, ensure_ascii=False), flush=True); open(f"{out}/dialog.jsonl", "a").write(json.dumps(r, ensure_ascii=False) + "\n")
sh(f"am force-stop {PKG}")
