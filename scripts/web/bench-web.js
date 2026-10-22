#!/usr/bin/env node
// 網頁版（wasm）壓測：egui / GPUI 的 wasm build 在 Chrome（Xvfb、無 GPU → WebGL2 = ANGLE + Mesa llvmpipe）裡跑，
// 跟原生壓測（scripts/bench-main-window.sh、bench-ticks.sh）同樣的情境、同樣由程式自己印的 BENCH／CHARTPERF／GROUPPERF／TICKBENCH 行。
//
// 每一輪：起行情（main/stress/heavy＝WryFeedHost --demo-ticks；ticks＝xq-tickgen）→ 起 xq-ws-bridge（同時提供網頁檔）
//        → 開全新 Chrome（新的 user-data-dir、--app 視窗 1200×720）→ 導到 index.html?XQ_BENCH=1&...（網址參數＝原生的環境變數）
//        → 量 → 關 Chrome、bridge、行情。一次只跑一個。
// 量測：
//   載入：goto 開始 → console 出現 "BENCH first-frame"／"first-quote"／"heavy-frame"（瀏覽器已經開好；原生是「程式啟動 →」）。
//   記憶體：Chrome 整棵行程樹（browser＋renderer＋GPU＋utility…）RSS／PSS（/proc/*/smaps_rollup），另列 renderer 與 GPU 行程；
//           JS heap（CDP Performance.getMetrics）、wasm 線性記憶體（程式 [metrics] rss_mb＝網頁版的 wasm heap）。
//   CPU：行程樹 utime+stime ÷ 秒數（100%＝一顆核心）；另列扣掉 llvmpipe 執行緒（GPU 行程裡的軟體光柵化）。
//   FPS：程式自己每秒報的（[metrics] fps=），跟原生同一段程式碼。
// 用法：node bench-web.js <egui|gpui> <main|stress|heavy|ticks:s1k|ticks:s10k|ticks:s50k|ticks:s100k|ticks:burst> <runIndex> <outDir>
// 環境：DISPLAY（預設 :7）、EGUI_DIST／GPUI_DIST（dist/web）、BRIDGE（xq-ws-bridge）、TICKGEN、FEEDHOST_DLL、XQ_FEED_PORT_MAIN（預設 47641）
const fs = require('fs'), path = require('path'), cp = require('child_process');
const puppeteer = require('puppeteer-core');

const [app, mode, runIx, OUT] = process.argv.slice(2);
const W = '/workspace/web';
const DIST = { egui: process.env.EGUI_DIST || `${W}/egui-xq-demo/dist/web`, gpui: process.env.GPUI_DIST || `${W}/gpui-xq-demo/dist/web` }[app];
const BRIDGE = process.env.BRIDGE || `${W}/gpui-xq-demo/target/release/xq-ws-bridge`;
const TICKGEN = process.env.TICKGEN || `${W}/gpui-xq-demo/target/release/xq-tickgen`;
const FEEDHOST_DLL = process.env.FEEDHOST_DLL || `${W}/feedhost/WryFeedHost/bin/Release/net8.0/WryFeedHost.dll`;
const DOTNET = process.env.DOTNET || `${process.env.HOME}/.dotnet/dotnet`;
const MAIN_PORT = +(process.env.XQ_FEED_PORT_MAIN || 47741), TICK_PORT = +(process.env.XQ_TICK_PORT || 47751), WEB_PORT = +(process.env.XQ_WEB_PORT || 47760);
const CLK = 100;
const SCEN = { s1k: ['--rate', '1000'], s10k: ['--rate', '10000'], s50k: ['--rate', '50000'], s100k: ['--rate', '100000'], burst: ['--burst', '150000,3,5000'] };
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const now = () => performance.now() / 1000;

// ---------- /proc ----------
const ppid = (p) => { try { return +fs.readFileSync(`/proc/${p}/stat`, 'utf8').split(') ').pop().split(' ')[1]; } catch { return -1; } };
function tree(root) {
  const pids = fs.readdirSync('/proc').filter((e) => /^\d+$/.test(e)).map(Number);
  const par = new Map(pids.map((p) => [p, ppid(p)]));
  const s = new Set([root]); let ch = true;
  while (ch) { ch = false; for (const p of pids) if (!s.has(p) && s.has(par.get(p))) { s.add(p); ch = true; } }
  return [...s];
}
const ptype = (p) => { try { const c = fs.readFileSync(`/proc/${p}/cmdline`, 'utf8').split(/[\0 ]/); const t = c.find((a) => a.startsWith('--type=')); return t ? t.slice(7) + (c.some((a) => a.includes('utility-sub-type=network')) ? ':network' : '') : (c[0].includes('chrome') ? 'browser' : path.basename(c[0])); } catch { return '?'; } };
function mem(p) {
  const r = { rss: 0, pss: 0, threads: 0 };
  try {
    for (const l of fs.readFileSync(`/proc/${p}/status`, 'utf8').split('\n')) { if (l.startsWith('VmRSS:')) r.rss = +l.split(/\s+/)[1]; if (l.startsWith('Threads:')) r.threads = +l.split(/\s+/)[1]; }
    for (const l of fs.readFileSync(`/proc/${p}/smaps_rollup`, 'utf8').split('\n')) if (l.startsWith('Pss:')) r.pss = +l.split(/\s+/)[1];
  } catch {}
  return r;
}
function memTree(root) {
  const by = {}; let rss = 0, pss = 0, n = 0;
  for (const p of tree(root)) { const m = mem(p); if (!m.rss) continue; n++; rss += m.rss; pss += m.pss; const t = ptype(p); by[t] = by[t] || { rss_mb: 0, pss_mb: 0, n: 0 }; by[t].rss_mb += m.rss / 1024; by[t].pss_mb += m.pss / 1024; by[t].n++; }
  for (const k in by) { by[k].rss_mb = +by[k].rss_mb.toFixed(1); by[k].pss_mb = +by[k].pss_mb.toFixed(1); }
  return { rss_mb: +(rss / 1024).toFixed(1), pss_mb: +(pss / 1024).toFixed(1), nproc: n, by };
}
function threadTicks(pids) {
  const out = {};
  for (const p of pids) {
    let tids = []; try { tids = fs.readdirSync(`/proc/${p}/task`); } catch { continue; }
    const t = ptype(p);
    for (const tid of tids) {
      try {
        const st = fs.readFileSync(`/proc/${p}/task/${tid}/stat`, 'utf8'); const comm = st.slice(st.indexOf('(') + 1, st.lastIndexOf(')'));
        const f = st.split(') ').pop().split(' '); out[`${p}/${tid}`] = { k: `${t}/${comm.replace(/\d+$/, '')}`, v: +f[11] + +f[12] };
      } catch {}
    }
  }
  return out;
}
function cpuSplit(a, b, secs) {
  let tot = 0, llvm = 0; const by = {};
  for (const id in b) { const d = b[id].v - (a[id] ? a[id].v : 0); if (d <= 0) continue; tot += d; if (b[id].k.includes('llvmpipe')) llvm += d; by[b[id].k] = (by[b[id].k] || 0) + d; }
  const top = Object.fromEntries(Object.entries(by).sort((x, y) => y[1] - x[1]).slice(0, 8).map(([k, v]) => [k, +(v / CLK / secs * 100).toFixed(1)]));
  return { cpu_pct: +(tot / CLK / secs * 100).toFixed(1), cpu_nollvm_pct: +((tot - llvm) / CLK / secs * 100).toFixed(1), top };
}

// ---------- 子行程 ----------
const procs = [];
function spawn(cmd, args, opts = {}, logName) {
  const p = cp.spawn(cmd, args, { stdio: ['ignore', 'pipe', 'pipe'], ...opts });
  procs.push(p);
  if (logName) { const f = fs.createWriteStream(`${OUT}/${logName}`, { flags: 'a' }); p.stdout.pipe(f); p.stderr.pipe(f); }
  return p;
}
async function waitPort(port, ms = 20000) {
  const net = require('net'); const end = Date.now() + ms;
  while (Date.now() < end) {
    const ok = await new Promise((r) => { const s = net.connect(port, '127.0.0.1', () => { s.destroy(); r(true); }); s.on('error', () => r(false)); });
    if (ok) return; await sleep(100);
  }
  throw new Error(`port ${port} 起不來`);
}
function killAll() { for (const p of procs.reverse()) { try { if (p.exitCode === null) p.kill('SIGTERM'); } catch {} } }
process.on('exit', killAll);

(async () => {
  const isTicks = mode.startsWith('ticks:'); const scen = isTicks ? mode.split(':')[1] : null;
  const rec = { app: `${app}-web`, mode, run: +runIx, date: new Date().toString(), loadavg: fs.readFileSync('/proc/loadavg', 'utf8').trim() };
  // 0. 埠口要是空的（box 上還有別的壓測在跑）
  for (const port of [MAIN_PORT, TICK_PORT, WEB_PORT]) {
    const busy = await new Promise((r) => { const s = require('net').connect(port, '127.0.0.1', () => { s.destroy(); r(true); }); s.on('error', () => r(false)); });
    if (busy) { console.error(`FATAL 埠 ${port} 已被佔用`); process.exit(2); }
  }
  // 1. 行情
  let feedPort = MAIN_PORT; const gev = [];
  if (isTicks) {
    feedPort = TICK_PORT;
    const g = spawn(TICKGEN, ['--addr', `127.0.0.1:${TICK_PORT}`, ...SCEN[scen], '--duration', '30', '--settle', '3', '--warmup', '3']);
    let buf = ''; g.stdout.on('data', (d) => { buf += d; let i; while ((i = buf.indexOf('\n')) >= 0) { const l = buf.slice(0, i); buf = buf.slice(i + 1); try { gev.push([now(), JSON.parse(l)]); } catch {} } });
    rec.gen_pid = g.pid;
  } else {
    // 只用行程內的 XQ_FEED_PORT；不碰別人在 47631 上的 WryFeedHost
    spawn(DOTNET, [FEEDHOST_DLL, '--demo-ticks'], { cwd: path.dirname(FEEDHOST_DLL), env: { ...process.env, XQ_FEED_PORT: String(MAIN_PORT) } }, 'feedhost.log');
  }
  // xq-tickgen 只服務第一個連上的 client，所以不能用探測連線，等它印 listen
  if (isTicks) { const end = Date.now() + 10000; while (!gev.some(([, j]) => j.ev === 'listen')) { if (Date.now() > end) throw new Error('tickgen 沒有 listen'); await sleep(20); } }
  else { await waitPort(feedPort); await sleep(1000); }
  // 2. bridge
  spawn(BRIDGE, ['--listen', `127.0.0.1:${WEB_PORT}`, '--feed', `127.0.0.1:${feedPort}`, '--www', DIST], {}, 'bridge.log');
  await waitPort(WEB_PORT);
  // 3. Chrome
  const prof = `/tmp/xq-web-bench-${app}-${process.pid}`; fs.rmSync(prof, { recursive: true, force: true });
  const browser = await puppeteer.launch({
    executablePath: '/usr/bin/google-chrome', headless: false, defaultViewport: null,
    args: ['--no-first-run', '--no-default-browser-check', '--window-size=1200,720', '--window-position=0,0', '--app=about:blank',
           `--user-data-dir=${prof}`, '--ignore-gpu-blocklist', '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
           '--disable-backgrounding-occluded-windows', '--disable-features=Translate,OptimizationHints,MediaRouter', '--password-store=basic'],
    env: { ...process.env, DISPLAY: process.env.DISPLAY || ':7' },
  });
  const bpid = browser.process().pid;
  const pages = await browser.pages(); const page = pages[0];
  const ev = {}; const fps = []; const metrics = []; const chartperf = {}; const groupperf = {}; const tb = []; const log = [];
  let t0 = now();
  page.on('console', (m) => {
    const s = m.text(); const t = now() - t0;
    if (s.startsWith('TICKBENCH|')) { try { tb.push([now(), JSON.parse(s.slice(10))]); } catch {} return; }
    log.push(`${t.toFixed(3)} ${s.slice(0, 400)}`);
    if (s.startsWith('BENCH ')) { const k = s.slice(6).trim(); if (!(k in ev)) ev[k] = t; }
    let mm;
    if ((mm = s.match(/^\[metrics\] fps=(\d+) rss_mb=([\d.]+)/))) { fps.push([t, +mm[1]]); metrics.push([t, +mm[2]]); }
    if (s.startsWith('CHARTPHASE|')) { const [, ph, se] = s.split('|'); ev[`ph:${ph}:${se}`] = t; }
    if (s.startsWith('CHARTPERF|')) { try { const j = JSON.parse(s.slice(10)); chartperf[j.phase] = j; ev[`cp:${j.phase}`] = t; } catch {} }
    if (s.startsWith('GROUPPERF|')) { try { const j = JSON.parse(s.slice(10)); groupperf[j.phase] = j; ev[`gp:${j.phase}`] = t; } catch {} }
  });
  page.on('pageerror', (e) => log.push(`PAGEERROR ${e.message}`));
  const cdp = await page.target().createCDPSession(); await cdp.send('Performance.enable');
  let bytes = 0; await cdp.send('Network.enable'); cdp.on('Network.loadingFinished', (e) => { bytes += e.encodedDataLength; });
  const jsHeap = async () => { const r = await cdp.send('Performance.getMetrics'); const g = (n) => (r.metrics.find((x) => x.name === n) || {}).value || 0; return { used_mb: +(g('JSHeapUsedSize') / 1048576).toFixed(1), total_mb: +(g('JSHeapTotalSize') / 1048576).toFixed(1) }; };
  const wasmMb = () => (metrics.length ? metrics[metrics.length - 1][1] : null);
  const q = { XQ_BENCH: '1', XQ_METRICS: '1', XQ_PERIOD: 'D', XQ_SEARCH: '0' };
  if (mode === 'stress') Object.assign(q, { XQ_GROUP_STRESS: '1', XQ_STRESS_COLS: '6', XQ_STRESS_TICK_HZ: '3000', XQ_STRESS_TICK_MS: '30000', XQ_STRESS_SCROLL_MS: '10000' });
  if (mode === 'heavy') Object.assign(q, { XQ_CHART_STRESS: '1', XQ_TOP_FRAC: '0.8', XQ_CHART_WARM_MS: '3000', XQ_CHART_PHASE_MS: '10000', XQ_CHART_DRAWINGS: '1000' });
  if (isTicks) Object.assign(q, { XQ_TICK_BENCH: '1' });
  const url = `http://127.0.0.1:${WEB_PORT}/index.html?` + new URLSearchParams(q);
  const waitEv = async (k, ms) => { const end = Date.now() + ms; while (!(k in ev)) { if (Date.now() > end) throw new Error(`等不到 ${k}；log 尾：${log.slice(-5).join(' | ')}`); await sleep(10); } return ev[k]; };
  const gwait = async (k, ms) => { const end = Date.now() + ms; for (;;) { const e = gev.find(([, j]) => j.ev === k); if (e) return e; if (Date.now() > end) throw new Error(`tickgen 等不到 ${k}`); await sleep(10); } };
  const shot = async (name) => { const f = `${OUT}/${name}.png`; await page.screenshot({ path: f }); return path.basename(f); };
  const ttick = () => threadTicks(tree(bpid));
  t0 = now();
  await page.goto(url, { waitUntil: 'domcontentloaded' });
  rec.goto_ms = +((now() - t0) * 1000).toFixed(0);
  try {
    if (mode === 'main') {
      await waitEv('first-quote', 30000);
      const at = async (s) => { while (now() - t0 < s) await sleep(50); };
      await at(10); rec.mem10 = memTree(bpid); rec.js10 = await jsHeap(); rec.wasm10_mb = wasmMb();
      await at(30); const a = ttick(); const ta = now();
      await at(58); if (+runIx === 1) rec.screenshot = await shot(`${app}-web`);
      await at(60); const b = ttick(); rec.cpu = cpuSplit(a, b, now() - ta);
      rec.mem60 = memTree(bpid); rec.js60 = await jsHeap(); rec.wasm60_mb = wasmMb();
      const w = fps.filter(([t]) => t >= 30 && t <= 60).map(([, v]) => v); rec.fps = w.length ? +(w.reduce((x, y) => x + y, 0) / w.length).toFixed(1) : null;
    } else if (mode === 'stress') {
      await waitEv('gp:tick-start', 40000); const a = ttick(); const ta = now();
      if (+runIx === 1) { while (now() - ta < 25) await sleep(50); rec.screenshot = await shot(`${app}-web-stress`); }
      await waitEv('gp:tick-end', 60000); const b = ttick(); const tb_ = now();
      rec.cpu_tick = cpuSplit(a, b, tb_ - ta); rec.mem_tick = memTree(bpid); rec.js_tick = await jsHeap(); rec.wasm_tick_mb = wasmMb();
      while (now() - tb_ < 8) await sleep(50); rec.mem_scroll = memTree(bpid);
      await waitEv('gp:done', 30000); const c = ttick(); rec.cpu_scroll = cpuSplit(b, c, now() - tb_);
      rec.groupperf = groupperf;
    } else if (mode === 'heavy') {
      await waitEv('heavy-frame', 60000); await waitEv('cp:setup', 10000);
      rec.setup = chartperf.setup; rec.phases = {};
      for (const ph of ['ticks', 'crosshair', 'pan']) {
        await waitEv(`ph:${ph}:start`, 40000); const a = ttick(); const ta = now();
        while (now() - ta < 5) await sleep(20);
        const m = memTree(bpid); const js = await jsHeap();
        if (+runIx === 1 && ph === 'crosshair') rec.screenshot = await shot(`${app}-web-heavy`);
        await waitEv(`ph:${ph}:end`, 40000); const b = ttick();
        const c = cpuSplit(a, b, now() - ta); await waitEv(`cp:${ph}`, 5000);
        rec.phases[ph] = { ...c, mem: m, js, wasm_mb: wasmMb(), perf: chartperf[ph] };
      }
    } else if (isTicks) {
      const [tStart, jStart] = await gwait('start', 60000);
      const a = ttick(); const ga = threadTicks([rec.gen_pid]); rec.mem_start = memTree(bpid);
      let shotDone = +runIx !== 1 || !['s50k', 'burst'].includes(scen); const mems = [];
      for (;;) {
        if (gev.some(([, j]) => j.ev === 'end') || now() - tStart > 60) break;
        if (!shotDone && now() - tStart >= 15) { rec.screenshot = await shot(`tick-${app}-web-${scen}`); shotDone = true; }
        if (mems.length < (now() - tStart) / 2) mems.push(memTree(bpid).rss_mb);
        await sleep(50);
      }
      const [tEnd, jEnd] = await gwait('end', 5000); const secs = tEnd - tStart;
      const b = ttick(); const gb = threadTicks([rec.gen_pid]); rec.mem_end = memTree(bpid); rec.js_end = await jsHeap(); rec.wasm_end_mb = wasmMb();
      rec.cpu = cpuSplit(a, b, secs); rec.gen_cpu = cpuSplit(ga, gb, secs).cpu_pct;
      await sleep(4000);
      rec.ticks = analyzeTicks(tb, tStart, tEnd, jStart.unixUs, jEnd, secs);
      rec.rss_peak_mb = Math.max(...mems, rec.mem_end.rss_mb);
    }
    rec.ok = true;
  } catch (e) {
    rec.ok = false; rec.error = String(e.message || e);
  }
  rec.load = { first_frame_s: ev['first-frame'], first_quote_s: ev['first-quote'], heavy_frame_s: ev['heavy-frame'], wasm_start_s: ev['wasm-start'], bytes_transferred: bytes };
  fs.appendFileSync(`${OUT}/${app}-web-${mode.replace(':', '-')}-console.log`, `==== run ${runIx} ${rec.date}\n${log.join('\n')}\n`);
  fs.appendFileSync(`${OUT}/results.jsonl`, JSON.stringify(rec) + '\n');
  console.log(JSON.stringify({ app: rec.app, mode, run: rec.run, ok: rec.ok, error: rec.error, load: rec.load, fps: rec.fps, cpu: rec.cpu && rec.cpu.cpu_pct, rss60: rec.mem60 && rec.mem60.rss_mb, phases: rec.phases && Object.fromEntries(Object.entries(rec.phases).map(([k, v]) => [k, [v.perf && v.perf.fps, v.cpu_pct, v.mem.rss_mb]])), ticks: rec.ticks && { p50: rec.ticks.latOldest.p50, p99: rec.ticks.latOldest.p99, loss: rec.ticks.loss, fps: rec.ticks.fps } }));
  await browser.close().catch(() => {});
  killAll(); await sleep(500);
  fs.rmSync(prof, { recursive: true, force: true });
  process.exit(0);
})().catch((e) => { console.error('FATAL', e); killAll(); process.exit(1); });

function pct(v, q) { if (!v.length) return null; const s = [...v].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.max(0, Math.round(q / 100 * (s.length - 1))))]; }
function analyzeTicks(tb, tStart, tEnd, startUs, jEnd, secs) {
  const endUs = jEnd.unixUs; const r = (v) => (v == null ? null : +v.toFixed(1));
  const ps = (v) => ({ p50: r(pct(v, 50)), p95: r(pct(v, 95)), p99: r(pct(v, 99)), max: r(v.length ? Math.max(...v) : null), n: v.length });
  const base = tb.filter(([t]) => t < tStart); if (!base.length) return { error: '沒有開始前的 TICKBENCH' };
  const c0 = base[base.length - 1][1]; const fin = tb[tb.length - 1][1];
  const samples = tb.flatMap(([, j]) => j.lat).filter((s) => s[1] >= startUs && s[3] <= endUs);
  const oldest = samples.map((s) => (s[6] - s[1]) / 1000), newest = samples.map((s) => (s[6] - s[3]) / 1000);
  const transport = samples.map((s) => (s[4] - s[3]) / 1000), toApply = samples.map((s) => (s[5] - s[4]) / 1000), toPresent = samples.map((s) => (s[6] - s[5]) / 1000);
  const first5 = samples.filter((s) => s[1] < startUs + 5e6).map((s) => (s[6] - s[1]) / 1000), last5 = samples.filter((s) => s[1] >= endUs - 5e6).map((s) => (s[6] - s[1]) / 1000);
  const fpsW = tb.filter(([t]) => t >= tStart + 1 && t <= tEnd).map(([, j]) => j.fps);
  const recv = fin.rowsIn - c0.rowsIn;
  return { sent: jEnd.sent, recvFinal: recv, loss: jEnd.sent - recv, toUi: fin.inboxRows - c0.inboxRows, applied: fin.applied - c0.applied, batches: fin.applies - c0.applies,
    latOldest: ps(oldest), latNewest: ps(newest),
    breakdown_p50: { transport: r(pct(transport, 50)), recvToApply: r(pct(toApply, 50)), applyToPresent: r(pct(toPresent, 50)) },
    trend: { first5_p50: r(pct(first5, 50)), last5_p50: r(pct(last5, 50)) },
    fps: fpsW.length ? r(fpsW.sort((a, b) => a - b)[Math.floor(fpsW.length / 2)]) : null, secs: r(secs) };
}
