// 一頁嵌多個元件：無頭 Chromium（Playwright）驗證腳本（egui 版和 GPUI 版共用，兩個 repo 的內容相同）。
// 用法：node verify.js <dist/web-multi 資料夾> <輸出資料夾> [額外網址參數，例如 XQ_WEB_BACKEND=webgl]
// 會自己開一個靜態檔案伺服器；輸出 results.json、console-*.log、截圖。
// 這台沒有 GPU：Chromium 用 SwiftShader（軟體）跑 WebGL2，數字只供參考。
const http = require("http");
const fs = require("fs");
const path = require("path");
const zlib = require("zlib");
const { chromium } = require("playwright");

const DIST = path.resolve(process.argv[2] || "../../dist/web-multi");
const OUT = path.resolve(process.argv[3] || "out");
const EXTRA = process.argv[4] || "";
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const qs = (s) => { const p = [s, EXTRA].filter(Boolean).join("&"); return p ? "?" + p : ""; };

const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".wasm": "application/wasm" };
// 伺服器可選 gzip（對 Accept-Encoding: gzip 的請求壓縮），量「頁面總下載量」用
function serve(gzip) {
  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => {
      const u = new URL(req.url, "http://x");
      const f = path.join(DIST, decodeURIComponent(u.pathname === "/" ? "/index.html" : u.pathname));
      if (!f.startsWith(DIST) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) {
        res.writeHead(404).end();
        return;
      }
      let body = fs.readFileSync(f);
      const h = { "Content-Type": TYPES[path.extname(f)] || "application/octet-stream", "Cache-Control": "no-store" };
      if (gzip && /gzip/.test(req.headers["accept-encoding"] || "")) {
        body = zlib.gzipSync(body, { level: 9 });
        h["Content-Encoding"] = "gzip";
      }
      res.writeHead(200, h).end(body);
    });
    srv.listen(0, "127.0.0.1", () => resolve(srv));
  });
}

async function open(browser, base, query, tag) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const log = [];
  const net = [];
  page.on("console", (m) => log.push(`[${m.type()}] ${m.text()}`));
  page.on("pageerror", (e) => log.push(`[pageerror] ${e.message}`));
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Network.enable");
  await cdp.send("Performance.enable");
  const reqUrl = {};
  cdp.on("Network.requestWillBeSent", (e) => (reqUrl[e.requestId] = e.request.url));
  cdp.on("Network.loadingFinished", (e) => net.push({ url: reqUrl[e.requestId], bytes: e.encodedDataLength }));
  await page.goto(`${base}/index.html${query}`);
  await page.waitForFunction(() => window.__xq && (window.__xq.readyMs !== undefined || window.__xq.bootError), null, { timeout: 120000 });
  const boot = await page.evaluate(() => ({ readyMs: window.__xq.readyMs, bootError: window.__xq.bootError, framework: window.__xq.framework }));
  const save = () => fs.writeFileSync(path.join(OUT, `console-${tag}.log`), log.join("\n") + "\n");
  return { page, cdp, log, net, boot, save };
}

const frames = (p) => p.evaluate(() => window.__xq.frames());
async function alive(p, ms = 3000) {
  const a = await frames(p);
  await sleep(ms);
  const b = await frames(p);
  const r = {};
  for (const k of Object.keys(a)) r[k] = { before: a[k], after: b[k], moving: typeof a[k] === "number" && b[k] > a[k] };
  return r;
}
// 載入期間的 WebGPU 格式警告不算錯誤
const bad = (log) => log.filter((l) => /panic|PANIC|\[error\]|\[pageerror\]|LOADFAIL/i.test(l));

async function memory(o) {
  await o.cdp.send("HeapProfiler.collectGarbage");
  await sleep(500);
  const m = await o.cdp.send("Performance.getMetrics");
  const g = (n) => (m.metrics.find((x) => x.name === n) || {}).value || 0;
  const wasm = await o.page.evaluate(() => window.__xq.wasmBytes());
  return { jsHeapUsedMB: +(g("JSHeapUsedSize") / 1048576).toFixed(1), jsHeapTotalMB: +(g("JSHeapTotalSize") / 1048576).toFixed(1), wasmMemoryMB: +(wasm / 1048576).toFixed(1) };
}

(async () => {
  const browser = await chromium.launch({ args: ["--ignore-gpu-blocklist", "--enable-unsafe-swiftshader", "--use-angle=swiftshader"] });
  const srv = await serve(false);
  const gsrv = await serve(true);
  const base = `http://127.0.0.1:${srv.address().port}`;
  const gbase = `http://127.0.0.1:${gsrv.address().port}`;
  const R = { browser: browser.version(), when: new Date().toISOString(), extra: EXTRA };

  // 1) 穿插版面（預設）：左上報價表、右上網頁、左下網頁、右下 K 線。都畫、都在動、網頁格能打字、移除一格、讓報價表 panic
  {
    const o = await open(browser, base, qs(""), "mixed");
    const fw = (R.framework = o.boot.framework);
    const s = (R.mixed = { boot: o.boot });
    await sleep(3000);
    await o.page.screenshot({ path: path.join(OUT, `${fw}-1-mixed.png`) });
    s.alive = await alive(o.page);
    s.consoleBad = bad(o.log);
    // 網頁格的輸入框能打字、按鈕能按（canvas 元件不會搶鍵盤）；點報價表第 3 列、滑鼠移到 K 線
    await o.page.fill("#order-sym", "2330");
    await o.page.click("#order-btn");
    s.typed = await o.page.evaluate(() => document.getElementById("order-log").textContent);
    const tb = await o.page.locator("#slot-table").boundingBox();
    const cb = await o.page.locator("#slot-chart").boundingBox();
    await o.page.mouse.click(tb.x + 60, tb.y + 82);
    await o.page.mouse.move(cb.x + 300, cb.y + 150);
    await sleep(800);
    await o.page.screenshot({ path: path.join(OUT, `${fw}-2-mixed-typed-clicked.png`) });
    s.aliveAfterInput = await alive(o.page);
    await o.page.evaluate(() => window.__xq.remove("webB", true));
    s.afterRemoveWebB = await alive(o.page);
    await o.page.evaluate(() => window.__xq.panic("table"));
    await sleep(500);
    s.afterPanicTable = await alive(o.page);
    s.consoleAfterPanic = bad(o.log);
    await o.page.screenshot({ path: path.join(OUT, `${fw}-3-mixed-removed-panicked.png`) });
    o.save();
    await o.page.close();
  }
  const fw = R.framework;

  // 2) 4 個 canvas 元件（對照）：都畫、都在動、移除一格、讓一格 panic
  {
    const o = await open(browser, base, qs("layout=canvas4"), "canvas4");
    const s = (R.canvas4 = { boot: o.boot });
    await sleep(3000);
    await o.page.screenshot({ path: path.join(OUT, `${fw}-4-canvas4.png`) });
    s.alive = await alive(o.page);
    s.consoleBad = bad(o.log);
    await o.page.evaluate(() => window.__xq.remove("c2", true));
    s.afterRemoveC2 = await alive(o.page);
    await o.page.evaluate(() => window.__xq.panic("c1"));
    await sleep(500);
    s.afterPanicC1 = await alive(o.page);
    await o.page.evaluate(() => window.__xq.panic("table"));
    await sleep(500);
    s.afterSecondPanicTable = await alive(o.page);
    s.consoleAfterPanic = bad(o.log);
    o.save();
    await o.page.close();
  }

  // 3) 只拿掉 canvas、不呼叫 destroy
  {
    const o = await open(browser, base, qs("layout=canvas4"), "canvas4-remove-nodestroy");
    const s = (R.removeNoDestroy = { boot: o.boot });
    await sleep(2000);
    await o.page.evaluate(() => window.__xq.remove("chart", false));
    s.after = await alive(o.page);
    s.consoleBad = bad(o.log);
    o.save();
    await o.page.close();
  }

  // 4) egui 才有：eframe 原本的 panic hook（一格 panic 全停）、每格各自 instance
  if (fw === "egui") {
    {
      const o = await open(browser, base, qs("layout=canvas4&isolate=0"), "canvas4-noisolate");
      const s = (R.noIsolate = { boot: o.boot });
      await sleep(2000);
      s.before = await alive(o.page);
      await o.page.evaluate(() => window.__xq.panic("c1"));
      await sleep(500);
      s.afterPanicC1 = await alive(o.page);
      s.panickedAfter = await o.page.evaluate(() => window.__xq.panicked());
      s.consoleAfterPanicCount = bad(o.log).length;
      await o.page.screenshot({ path: path.join(OUT, `${fw}-5-noisolate-panic-c1.png`) });
      o.save();
      await o.page.close();
    }
    {
      const o = await open(browser, base, qs("layout=canvas4&mode=separate"), "canvas4-separate");
      const s = (R.separate = { boot: o.boot });
      await sleep(3000);
      s.alive = await alive(o.page);
      s.consoleBad = bad(o.log);
      await o.page.evaluate(() => window.__xq.panic("c1"));
      await sleep(500);
      s.afterPanicC1 = await alive(o.page);
      o.save();
      await o.page.close();
    }
  }

  // 5) 下載量（伺服器開 gzip）
  const dl = [["1 個元件（報價表）", "only=table"], ["穿插版面（2 個元件＋2 個網頁格）", ""], ["4 個元件", "layout=canvas4"]];
  if (fw === "egui") dl.push(["4 個元件，各自 instance", "layout=canvas4&mode=separate"]);
  R.download = {};
  for (const [k, q] of dl) {
    const o = await open(browser, gbase, qs(q), "download");
    await sleep(1500);
    R.download[k] = { totalBytes: o.net.reduce((s, x) => s + x.bytes, 0), requests: o.net.map((x) => ({ url: x.url.replace(gbase, ""), bytes: x.bytes })) };
    await o.page.close();
  }

  // 6) 記憶體：各開新分頁、跑 5 秒後量，3 次取中位數
  const mem = [["1 個元件（報價表）", "only=table"], ["穿插版面（2 個元件＋2 個網頁格）", ""], ["4 個元件", "layout=canvas4"]];
  if (fw === "egui") mem.push(["4 個元件，各自 instance", "layout=canvas4&mode=separate"]);
  R.memory = {};
  for (const [k, q] of mem) {
    const runs = [];
    for (let i = 0; i < 3; i++) {
      const o = await open(browser, base, qs(q), "mem");
      await sleep(5000);
      runs.push(await memory(o));
      await o.page.close();
    }
    const med = (f) => runs.map((r) => r[f]).sort((a, b) => a - b)[1];
    R.memory[k] = { runs, median: { jsHeapUsedMB: med("jsHeapUsedMB"), jsHeapTotalMB: med("jsHeapTotalMB"), wasmMemoryMB: med("wasmMemoryMB") } };
  }

  fs.writeFileSync(path.join(OUT, "results.json"), JSON.stringify(R, null, 2));
  console.log(JSON.stringify(R, null, 2));
  await browser.close();
  srv.close();
  gsrv.close();
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
