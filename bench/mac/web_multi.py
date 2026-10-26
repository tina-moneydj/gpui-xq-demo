#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""網頁版「一頁嵌多個元件」情境（壓測包的 web 情境；也可以單獨跑：python3 web_multi.py）。

工具包的 web/egui、web/gpui 是 egui-xq-demo／gpui-xq-demo 的 examples/web-multi 建好的網頁（wasm＋index.html＋page.js）。
這支在本機開一個小網頁伺服器（只聽 127.0.0.1、會 gzip、收 /__bench 回傳的結果），用一個全新的瀏覽器設定檔開頁面，
頁面自己照固定流程量完（暖機 3 秒 → 5 秒幀數 → 記憶體 → 移除一格 → 讓報價表 panic），把結果 POST 回來，再關掉瀏覽器。
這裡另外量：頁面總下載量（gzip 後實際傳輸的位元組）、瀏覽器全部行程的 RSS 與 5 秒量測區間的 CPU。

版面（每個框架都跑）：
  mixed    穿插：左上報價表、右上網頁、左下網頁、右下 K 線（canvas 元件 2 個＋一般 HTML 2 格）
  canvas4  4 個 canvas 元件：報價表、K 線、兩個計數器
  single   只有報價表 1 個元件（記憶體基準）
egui 共用一個 wasm instance；GPUI（gpui_web 一個 instance 只能一張 canvas）每個元件各自一個 instance。
瀏覽器：Chrome 優先，沒有就 Edge（Mac 上 Safari 不能用新設定檔自動開關，不支援）。
"""
import gzip, http.server, json, os, shutil, socket, statistics, subprocess, sys, tempfile, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
IS_MAC = sys.platform == "darwin"
IS_WIN = os.name == "nt"
FRAMEWORKS = ("egui", "gpui")
CONFIGS = (("mixed", ""), ("canvas4", "layout=canvas4"), ("single", "only=table&steps=basic"))
CONFIG_ZH = {"mixed": "穿插（2 個元件＋2 個網頁格）", "canvas4": "4 個元件", "single": "1 個元件（報價表）"}
FW_ZH = {"egui": "egui（WebGL2）", "gpui": "GPUI（WebGPU 優先）"}
TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".wasm": "application/wasm"}


def find_browser():
    cands = []
    if IS_MAC:
        cands = ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                 os.path.expanduser("~/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                 "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"]
    elif IS_WIN:
        for base in (os.environ.get("ProgramFiles"), os.environ.get("ProgramFiles(x86)"), os.environ.get("LOCALAPPDATA")):
            if base:
                cands += [os.path.join(base, "Google", "Chrome", "Application", "chrome.exe"),
                          os.path.join(base, "Microsoft", "Edge", "Application", "msedge.exe")]
    else:
        cands = [shutil.which(x) or "" for x in ("google-chrome", "chromium", "chromium-browser")]
        cands.append(os.environ.get("XQ_BROWSER", ""))
    env = os.environ.get("XQ_BROWSER")
    if env: cands.insert(0, env)
    for c in cands:
        if c and os.path.exists(c): return c
    return None


class Server:
    """127.0.0.1 上的靜態檔案伺服器：gzip（快取壓好的內容）、計算送出的位元組、收頁面 POST 的結果"""
    def __init__(self, root):
        self.root = os.path.abspath(root); self.cache = {}; self.lock = threading.Lock()
        self.reset()
        srv = self
        class H(http.server.BaseHTTPRequestHandler):
            def log_message(self, *a): pass
            def do_GET(self):
                path = self.path.split("?", 1)[0]
                f = os.path.abspath(os.path.join(srv.root, path.lstrip("/") or "index.html"))
                if not f.startswith(srv.root) or not os.path.isfile(f):
                    self.send_response(404); self.end_headers(); return
                gz = "gzip" in (self.headers.get("Accept-Encoding") or "")
                body = srv.body(f, gz)
                self.send_response(200)
                self.send_header("Content-Type", TYPES.get(os.path.splitext(f)[1], "application/octet-stream"))
                self.send_header("Cache-Control", "no-store")
                if gz: self.send_header("Content-Encoding", "gzip")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers(); self.wfile.write(body)
                with srv.lock: srv.sent += len(body); srv.files.append([path, len(body)])
            def do_POST(self):
                n = int(self.headers.get("Content-Length") or 0)
                data = self.rfile.read(n)
                self.send_response(204); self.end_headers()
                try: obj = json.loads(data.decode("utf-8"))
                except Exception: return
                with srv.lock: srv.posts.append((time.monotonic(), obj))
                srv.ev.set()
        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
        self.port = self.httpd.server_address[1]
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def body(self, f, gz):
        k = (f, gz)
        if k not in self.cache:
            raw = open(f, "rb").read()
            self.cache[k] = gzip.compress(raw, 6) if gz else raw
        return self.cache[k]

    def reset(self):
        self.sent = 0; self.files = []; self.posts = []; self.ev = threading.Event()

    def wait(self, pred, timeout):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            with self.lock:
                for t, o in self.posts:
                    if pred(o): return t, o
            self.ev.wait(0.2); self.ev.clear()
        return None, None

    def close(self):
        self.httpd.shutdown()


# ---- 行程樹、記憶體、CPU（只用系統內建工具：Mac／Linux 用 ps，Windows 用 PowerShell 的 Get-CimInstance）
def _ps_rows():
    if IS_WIN:
        out = subprocess.run(["powershell", "-NoProfile", "-Command",
                              "Get-CimInstance Win32_Process | ForEach-Object { '{0} {1} {2} {3}' -f $_.ProcessId,$_.ParentProcessId,$_.WorkingSetSize,($_.KernelModeTime+$_.UserModeTime) }"],
                             capture_output=True, text=True).stdout
        rows = []
        for l in out.splitlines():
            p = l.split()
            if len(p) == 4: rows.append((int(p[0]), int(p[1]), int(p[2]) / 1048576.0, int(p[3]) / 1e7))
        return rows
    out = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,rss=,time="], capture_output=True, text=True).stdout
    rows = []
    for l in out.splitlines():
        p = l.split()
        if len(p) == 4 and p[0].isdigit():
            rows.append((int(p[0]), int(p[1]), int(p[2]) / 1024.0, _cputime(p[3])))
    return rows


def _cputime(t):
    days = 0
    if "-" in t: d, t = t.split("-", 1); days = int(d)
    secs = 0.0
    for part in t.split(":"): secs = secs * 60 + float(part)
    return days * 86400 + secs


def tree_stats(root):
    """(行程數, RSS MB 總和, CPU 秒總和)；瀏覽器的 GPU／繪圖／分頁行程都是 root 的子孫"""
    rows = _ps_rows()
    kids = {}
    for pid, ppid, _, _ in rows: kids.setdefault(ppid, []).append(pid)
    s = {root}; stack = [root]
    while stack:
        for c in kids.get(stack.pop(), []):
            if c not in s: s.add(c); stack.append(c)
    sel = [r for r in rows if r[0] in s]
    return len(sel), round(sum(r[2] for r in sel), 1), sum(r[3] for r in sel)


def kill_tree(proc):
    try:
        if IS_WIN: subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)], capture_output=True)
        else:
            n, _, _ = tree_stats(proc.pid)
            proc.terminate()
            try: proc.wait(5)
            except Exception: proc.kill()
    except Exception: pass


def run_one(browser, srv, fw, cfg, query, run_no, say, timeout=150):
    """開一次瀏覽器跑一次頁面自動壓測；回傳一筆紀錄（失敗丟例外）"""
    srv.reset()
    prof = tempfile.mkdtemp(prefix="xq-web-")
    q = "bench=1" + ("&" + query if query else "")
    if os.environ.get("XQ_WEB_QUERY"): q += "&" + os.environ["XQ_WEB_QUERY"]  # 開發者自測用（例如雲端：XQ_WEB_BACKEND=webgl）
    url = "http://127.0.0.1:%d/%s/index.html?%s" % (srv.port, fw, q)
    args = [browser, "--user-data-dir=" + prof, "--no-first-run", "--no-default-browser-check", "--disable-extensions",
            "--disable-background-timer-throttling", "--disable-renderer-backgrounding", "--disable-backgrounding-occluded-windows",
            "--window-position=40,40", "--window-size=1320,960"]
    if IS_MAC: args += ["--use-mock-keychain"]  # 新設定檔不要跳「鑰匙圈」權限視窗
    args += [x for x in os.environ.get("XQ_BROWSER_ARGS", "").split() if x]  # 開發者自測用（例如雲端無頭：--headless=new --use-angle=swiftshader）
    args += ["--new-window", url]
    t0 = time.monotonic()
    proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        ts, _ = srv.wait(lambda o: o.get("mark") == "fps-start", timeout)
        if ts is None: raise RuntimeError("等不到頁面開始量測（%d 秒）；頁面可能載入失敗" % timeout)
        n0, _, c0 = tree_stats(proc.pid); w0 = time.monotonic()
        te, _ = srv.wait(lambda o: o.get("mark") == "fps-end", 30)
        n1, rss_fps, c1 = tree_stats(proc.pid); w1 = time.monotonic()
        _, res = srv.wait(lambda o: "result" in o or "error" in o, 60)
        if res is None: raise RuntimeError("等不到頁面回傳結果")
        if "error" in res: raise RuntimeError("頁面回報錯誤：%s" % res["error"])
        r = res["result"]
        rec = {"framework": fw, "config": cfg, "run": run_no, "url": q, "userAgent": res.get("userAgent"),
               "loadToFpsStartS": round(ts - t0, 2), "downloadBytes": srv.sent, "files": srv.files[:20],
               "browserProcs": n1, "browserRssMb": rss_fps,
               "browserCpuPct": round((c1 - c0) / max(w1 - w0, 0.001) * 100, 1)}
        rec.update(r)
        return rec
    finally:
        kill_tree(proc)
        time.sleep(1.0)
        shutil.rmtree(prof, ignore_errors=True)


def run_all(web_root, runs, say, frameworks=FRAMEWORKS, configs=CONFIGS):
    """跑完全部（框架 × 版面 × 輪），回傳 (紀錄清單, 失敗清單, 用的瀏覽器)"""
    recs, fails = [], []
    browser = find_browser()
    if not browser:
        say("！找不到 Chrome 或 Edge，略過網頁情境（請安裝 Google Chrome 後重跑 --scenarios web）")
        return recs, [{"fn": "web", "err": "找不到 Chrome／Edge"}], None
    fws = [f for f in frameworks if os.path.isfile(os.path.join(web_root, f, "index.html"))]
    if not fws:
        say("！工具包裡沒有網頁版檔案（web/egui、web/gpui），略過網頁情境")
        return recs, [{"fn": "web", "err": "沒有 web/ 資料夾"}], browser
    srv = Server(web_root)
    try:
        for run in range(1, runs + 1):
            for cfg, query in configs:
                for fw in fws:  # 每輪框架交錯跑
                    say("第 %d/%d 輪 網頁 %s %s…" % (run, runs, fw, CONFIG_ZH[cfg]))
                    for k in (1, 2):
                        try:
                            r = run_one(browser, srv, fw, cfg, query, run, say)
                            recs.append(r)
                            say("  幀數 %s、wasm %s MB、瀏覽器 RSS %s MB、CPU %s%%、下載 %.2f MB" % (
                                r.get("fps"), (r.get("memory") or {}).get("wasmMemoryMB"), r["browserRssMb"], r["browserCpuPct"], r["downloadBytes"] / 1048576))
                            break
                        except Exception as e:
                            say("  ！失敗（第 %d 次）：%s" % (k, e))
                            fails.append({"fn": "web", "args": [fw, cfg, run], "try": k, "err": str(e)})
                            time.sleep(2)
    finally:
        srv.close()
    return recs, fails, browser


def _med(xs):
    v = [float(x) for x in xs if isinstance(x, (int, float))]
    return round(statistics.median(v), 1) if v else None


def summary_md(recs, browser):
    """summary.md 的「網頁：一頁嵌多個元件」一節（中位數；看的是能不能同時跑、互相干擾、資源用量，不判勝負）"""
    if not recs:
        return ["", "## 網頁：一頁嵌多個元件", "", "- 沒有資料（%s）。" % ("找不到 Chrome／Edge" if not browser else "全部失敗，見 failures")]
    md = ["", "## 網頁：一頁嵌多個元件（同一頁放 canvas 元件和一般 HTML）", "",
          "- 瀏覽器：%s（全新設定檔、沒有擴充功能）。%s" % (os.path.basename(browser or "?"), (recs[0].get("userAgent") or "")[:160]),
          "- egui 共用一個 wasm instance；GPUI 每個元件各自一個 instance（同一份 wasm 只下載一次）。各輪中位數。",
          "- 「幀數」是每一格自己數的每秒畫面數（網頁格用 requestAnimationFrame）；只要 > 0 就代表那格有在動。",
          "- 瀏覽器 RSS／CPU 是整個瀏覽器（含 GPU、分頁行程）在 5 秒量測區間的數字；RSS 是各行程相加（共用的部分會重複算，只適合兩邊互比）；CPU 100%＝一顆核心。", ""]
    fws = [f for f in FRAMEWORKS if any(r["framework"] == f for r in recs)]
    for cfg, _ in CONFIGS:
        rs = [r for r in recs if r["config"] == cfg]
        if not rs: continue
        ids = []
        for r in rs:
            for k in (r.get("fps") or {}):
                if k not in ids: ids.append(k)
        md += ["### " + CONFIG_ZH[cfg], "", "| 項目 | " + " | ".join(FW_ZH[f] for f in fws) + " |", "|---|" + "---|" * len(fws)]
        def row(label, get, unit=""):
            cells = []
            for f in fws:
                v = _med([get(r) for r in rs if r["framework"] == f])
                cells.append("—" if v is None else "%g%s" % (v, unit))
            md.append("| %s | %s |" % (label, " | ".join(cells)))
        row("啟動到元件都好（頁面自己量）", lambda r: r.get("readyMs"), " ms")
        for k in ids: row("幀數：%s" % k, lambda r, k=k: (r.get("fps") or {}).get(k), " fps")
        row("JS heap（已用）", lambda r: (r.get("memory") or {}).get("jsHeapUsedMB"), " MB")
        row("wasm 記憶體（全部 instance 加總）", lambda r: (r.get("memory") or {}).get("wasmMemoryMB"), " MB")
        row("瀏覽器全部行程 RSS", lambda r: r.get("browserRssMb"), " MB")
        row("瀏覽器 CPU（5 秒量測區間）", lambda r: r.get("browserCpuPct"), " %")
        row("頁面總下載量（gzip）", lambda r: r.get("downloadBytes", 0) / 1048576, " MB")
        if any(r.get("removed") for r in rs):
            for k in ids:
                row("移除 %s 後：%s" % (rs[0].get("removed"), k), lambda r, k=k: (r.get("fpsAfterRemove") or {}).get(k), " fps")
            for k in ids:
                row("報價表 panic 後：%s" % k, lambda r, k=k: (r.get("fpsAfterPanic") or {}).get(k), " fps")
        md.append("")
    md.append("- 判讀：移除／panic 後，被移除或 panic 的那格應該是 0 或空白，其他格要 > 0；如果其他格也變 0，就是互相拖垮。")
    md.append("- WebGL 繪圖器（瀏覽器回報）：%s；navigator.gpu（WebGPU）：%s。" % (recs[0].get("webglRenderer"), "有" if recs[0].get("gpu") else "沒有"))
    return md


if __name__ == "__main__":
    import argparse
    ap = argparse.ArgumentParser(description="只跑網頁「一頁嵌多個元件」情境")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--web-dir", default=os.path.join(HERE, "web"))
    ap.add_argument("--out", default=os.path.join(HERE, "results", "web-" + time.strftime("%Y%m%d-%H%M%S")))
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    say = lambda m: print("[%s] %s" % (time.strftime("%H:%M:%S"), m), flush=True)
    recs, fails, browser = run_all(a.web_dir, a.runs, say)
    json.dump({"browser": browser, "records": recs, "failures": fails}, open(os.path.join(a.out, "web.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    open(os.path.join(a.out, "web.md"), "w", encoding="utf-8").write("\n".join(summary_md(recs, browser)) + "\n")
    say("完成：%s" % a.out)
