// Chrome 本身的記憶體基準：同樣的啟動參數，開 about:blank 或一張每幀清畫面的 WebGL2 canvas（1200×720），30 秒後量整棵行程樹 PSS。
// 用法：node chrome-baseline.js <blank|webgl> <run> <outDir>
const fs = require('fs'), path = require('path'), puppeteer = require('puppeteer-core');
const [kind, run, OUT] = process.argv.slice(2);
const ppid = (p) => { try { return +fs.readFileSync(`/proc/${p}/stat`, 'utf8').split(') ').pop().split(' ')[1]; } catch { return -1; } };
function tree(root) { const pids = fs.readdirSync('/proc').filter((e) => /^\d+$/.test(e)).map(Number); const par = new Map(pids.map((p) => [p, ppid(p)])); const s = new Set([root]); let ch = true; while (ch) { ch = false; for (const p of pids) if (!s.has(p) && s.has(par.get(p))) { s.add(p); ch = true; } } return [...s]; }
const ptype = (p) => { try { const c = fs.readFileSync(`/proc/${p}/cmdline`, 'utf8').split(/[\0 ]/); const t = c.find((a) => a.startsWith('--type=')); return t ? t.slice(7) : 'browser'; } catch { return '?'; } };
function mem(p) { const r = { rss: 0, pss: 0 }; try { for (const l of fs.readFileSync(`/proc/${p}/status`, 'utf8').split('\n')) if (l.startsWith('VmRSS:')) r.rss = +l.split(/\s+/)[1]; for (const l of fs.readFileSync(`/proc/${p}/smaps_rollup`, 'utf8').split('\n')) if (l.startsWith('Pss:')) r.pss = +l.split(/\s+/)[1]; } catch {} return r; }
function memTree(root) { const by = {}; let rss = 0, pss = 0; for (const p of tree(root)) { const m = mem(p); if (!m.rss) continue; rss += m.rss; pss += m.pss; const t = ptype(p); by[t] = (by[t] || 0) + m.pss / 1024; } for (const k in by) by[k] = +by[k].toFixed(1); return { rss_mb: +(rss / 1024).toFixed(1), pss_mb: +(pss / 1024).toFixed(1), by }; }
const GL = 'data:text/html,<body style="margin:0"><canvas id=c width=1200 height=720></canvas><script>const g=document.getElementById("c").getContext("webgl2");let i=0;(function f(){g.clearColor((i++%60)/60,0,0,1);g.clear(g.COLOR_BUFFER_BIT);requestAnimationFrame(f)})()</script>';
(async () => {
  const prof = `/tmp/xq-web-baseline-${process.pid}`; fs.rmSync(prof, { recursive: true, force: true });
  const browser = await puppeteer.launch({ executablePath: '/usr/bin/google-chrome', headless: false, defaultViewport: null,
    args: ['--no-first-run', '--no-default-browser-check', '--window-size=1200,720', '--window-position=0,0', '--app=about:blank', `--user-data-dir=${prof}`, '--ignore-gpu-blocklist', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows', '--disable-features=Translate,OptimizationHints,MediaRouter', '--password-store=basic'],
    env: { ...process.env, DISPLAY: process.env.DISPLAY || ':7' } });
  const page = (await browser.pages())[0];
  if (kind === 'webgl') await page.goto(GL);
  await new Promise((r) => setTimeout(r, 30000));
  const rec = { kind, run: +run, date: new Date().toString(), loadavg: fs.readFileSync('/proc/loadavg', 'utf8').trim(), mem30: memTree(browser.process().pid) };
  fs.appendFileSync(`${OUT}/baseline.jsonl`, JSON.stringify(rec) + '\n'); console.log(JSON.stringify(rec));
  await browser.close(); fs.rmSync(prof, { recursive: true, force: true });
})();
