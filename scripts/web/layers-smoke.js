// 網頁版圖層／上下順序冒煙測試（merge main 之後）：重圖表壓測＋drag/select/reorder/zshot 四段（每段縮短成 3 秒），
// 每段開始 1.5 秒截圖，收 CHARTPHASE／CHARTPERF。用法：node layers-smoke.js <egui|gpui> <outDir> [layers=100]
const fs = require('fs'), path = require('path'), cp = require('child_process'), puppeteer = require('puppeteer-core');
const [app, OUT, LAYERS = '100'] = process.argv.slice(2);
const W = '/workspace/web', FEED = 47741, WEB = 47760;
const DLL = `${W}/feedhost/WryFeedHost/bin/Release/net8.0/WryFeedHost.dll`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
fs.mkdirSync(OUT, { recursive: true });
const kids = [];
const sp = (cmd, args, opts, log) => { const p = cp.spawn(cmd, args, { stdio: ['ignore', 'pipe', 'pipe'], ...opts }); const f = fs.createWriteStream(`${OUT}/${log}`); p.stdout.pipe(f); p.stderr.pipe(f); kids.push(p); return p; };
const waitPort = async (port) => { for (let i = 0; i < 200; i++) { const ok = await new Promise((r) => { const s = require('net').connect(port, '127.0.0.1', () => { s.destroy(); r(true); }); s.on('error', () => r(false)); }); if (ok) return; await sleep(50); } throw new Error('port ' + port); };
(async () => {
  sp(`${process.env.HOME}/.dotnet/dotnet`, [DLL, '--demo-ticks'], { cwd: path.dirname(DLL), env: { ...process.env, XQ_FEED_PORT: String(FEED) } }, `${app}-feedhost.log`);
  await waitPort(FEED); await sleep(1000);
  sp(`${W}/gpui-xq-demo/target/release/xq-ws-bridge`, ['--listen', `127.0.0.1:${WEB}`, '--feed', `127.0.0.1:${FEED}`, '--www', `${W}/${app}-xq-demo/dist/web`], {}, `${app}-bridge.log`);
  await waitPort(WEB);
  const prof = `/tmp/xq-web-layers-${process.pid}`;
  const browser = await puppeteer.launch({ executablePath: '/usr/bin/google-chrome', headless: false, defaultViewport: null,
    args: ['--no-first-run', '--no-default-browser-check', '--window-size=1200,720', '--window-position=0,0', '--app=about:blank', `--user-data-dir=${prof}`, '--ignore-gpu-blocklist', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--password-store=basic'],
    env: { ...process.env, DISPLAY: process.env.DISPLAY || ':7' } });
  const page = (await browser.pages())[0];
  const log = []; let perf = null; const perfs = []; const shots = [];
  page.on('console', async (m) => {
    const s = m.text(); log.push(s.slice(0, 2000));
    const mm = s.match(/^CHARTPHASE\|(drag|select|reorder|zshot)\|start/);
    if (mm) { await sleep(1500); const f = `${OUT}/${app}-web-layers${LAYERS}-${mm[1]}.png`; await page.screenshot({ path: f }).catch(() => {}); shots.push(path.basename(f)); }
    if (s.startsWith('CHARTPERF|')) { perfs.push(s.slice(10)); if (s.includes('"phase":"done"')) perf = true; }
  });
  const q = { XQ_BENCH: '1', XQ_CHART_STRESS: '1', XQ_TOP_FRAC: '0.8', XQ_CHART_WARM_MS: '2000', XQ_CHART_PHASE_MS: '3000', XQ_CHART_DRAWINGS: '1000', XQ_CHART_LAYERS: LAYERS };
  await page.goto(`http://127.0.0.1:${WEB}/index.html?` + new URLSearchParams(q));
  const end = Date.now() + 120000; while (!perf && Date.now() < end) await sleep(200);
  await sleep(500);
  fs.writeFileSync(`${OUT}/${app}-layers${LAYERS}-console.log`, log.join('\n') + '\n');
  fs.writeFileSync(`${OUT}/${app}-layers${LAYERS}-chartperf.jsonl`, perfs.join('\n') + '\n');
  console.log(JSON.stringify({ app, layers: LAYERS, ok: !!perf, shots, phases: log.filter((l) => l.startsWith('CHARTPHASE')), panics: log.filter((l) => /panick|RuntimeError|unreachable/i.test(l)) }));
  await browser.close(); for (const k of kids) { try { process.kill(k.pid); } catch {} }
  fs.rmSync(prof, { recursive: true, force: true }); process.exit(0);
})().catch((e) => { console.error(e); for (const k of kids) { try { process.kill(k.pid); } catch {} } process.exit(1); });
