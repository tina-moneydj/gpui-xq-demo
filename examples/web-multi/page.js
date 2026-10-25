// 一頁嵌多個元件：egui 版和 GPUI 版共用的頁面邏輯（兩個 repo 的 examples/web-multi/page.js 內容相同）。
// 版面、一般網頁格、移除／panic、幀數，以及「自動壓測模式」（?bench=1：照固定流程量完，把結果 POST 回 /__bench）。
//
// 網址參數：
//   layout=mixed（預設）：左上報價表、右上網頁、左下網頁、右下 K 線（canvas 元件和一般 HTML 穿插）
//   layout=canvas4：報價表、K 線、兩個計數器，四格都是 canvas 元件
//   only=table,chart：只啟動列出的格子（量一個元件的記憶體用）
//   bench=1：自動壓測（壓測包用）；steps=basic 只量幀數與記憶體，預設 full 再加移除／panic

const q = new URLSearchParams(location.search);
export const params = q;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function setupPage({ framework, mode, boot }) {
  const layout = q.get("layout") === "canvas4" ? "canvas4" : "mixed";
  const name = framework === "egui" ? "egui" : "GPUI";
  const CELLS =
    layout === "canvas4"
      ? [["table", `報價表（${name}，50 列，合成行情）`, "table"], ["chart", `K 線（${name}，2330 日K）`, "chart"], ["c1", `計數器 1（${name}）`, "counter"], ["c2", `計數器 2（${name}）`, "counter"]]
      : [["table", `報價組合（${name}，50 列，合成行情）`, "table"], ["webA", "網頁格：公告（一般 HTML）", "web"], ["webB", "網頁格：下單匣（一般 HTML）", "web"], ["chart", `技術分析（${name}，2330 日K）`, "chart"]];
  const only = q.get("only");
  const ids = CELLS.map((c) => c[0]).filter((id) => !only || only.split(",").includes(id));
  const t0 = performance.now();
  const xq = (window.__xq = { framework, mode, layout, ids, widgets: {}, errors: [] });

  const grid = document.getElementById("grid");
  for (const [id, title, kind] of CELLS) {
    const cell = Object.assign(document.createElement("div"), { className: "cell", id: "cell-" + id });
    cell.innerHTML = `<h2>${title}<span id="st-${id}"></span></h2>`;
    cell.append(Object.assign(document.createElement("div"), { className: kind === "web" ? "web" : "slot", id: "slot-" + id }));
    grid.append(cell);
  }

  // 移除一格：destroy=true 先關掉元件（建議）；false＝只把 canvas 從 DOM 拿掉
  function remove(id, destroy = true) {
    const w = xq.widgets[id];
    if (w && destroy) w.destroy();
    const slot = document.getElementById("slot-" + id);
    if (slot) slot.replaceWith(Object.assign(document.createElement("div"), { className: "gone", textContent: destroy ? "已移除（destroy）" : "已移除（只拿掉 canvas）" }));
  }
  xq.remove = remove;
  xq.panic = (id) => xq.widgets[id].panic();
  xq.frames = () => Object.fromEntries(Object.entries(xq.widgets).map(([id, w]) => { try { return [id, w.frames()]; } catch (e) { return [id, "錯誤：" + e]; } }));
  xq.panicked = () => Object.fromEntries(Object.entries(xq.widgets).filter(([, w]) => w.hasPanicked).map(([id, w]) => { try { return [id, w.hasPanicked()]; } catch (e) { return [id, "錯誤：" + e]; } }));
  window.addEventListener("error", (e) => xq.errors.push(String(e.message || e)));
  window.addEventListener("unhandledrejection", (e) => xq.errors.push(String(e.reason)));

  setInterval(() => {
    for (const [id, v] of Object.entries(xq.frames())) { const el = document.getElementById("st-" + id); if (el) el.textContent = `幀 ${v}`; }
  }, 500);

  (async () => {
    for (const [id, , kind] of CELLS) if (ids.includes(id) && kind === "web") xq.widgets[id] = webCell(id);
    await boot({ xq, cells: CELLS.filter(([id, , kind]) => ids.includes(id) && kind !== "web").map(([id, , kind]) => ({ id, kind, slot: document.getElementById("slot-" + id) })) });
    for (const [id] of CELLS) if (!ids.includes(id)) remove(id, true);
    xq.readyMs = performance.now() - t0;
    document.getElementById("mode").textContent =
      `版面：${layout === "mixed" ? `${name} 和一般網頁穿插` : `4 個 ${name} 元件`}｜${xq.modeText || ""}｜啟動 ${xq.readyMs.toFixed(0)} ms`;
    if (q.get("bench") === "1") bench(xq).catch((e) => post({ error: String(e) }));
  })().catch((e) => {
    document.getElementById("mode").textContent = "載入失敗：" + e;
    console.error("LOADFAIL " + e);
    xq.bootError = String(e);
    if (q.get("bench") === "1") post({ error: "載入失敗：" + e });
  });
  return xq;
}

// 一般網頁格：用 requestAnimationFrame 數幀（跟 canvas 元件一樣的「有沒有在動」判斷）＋一點真的 DOM 內容
function webCell(id) {
  const slot = document.getElementById("slot-" + id);
  if (id === "webA") {
    slot.innerHTML = `<div class="big" id="n-${id}">0</div><div>每幀 +1（requestAnimationFrame）</div><ul id="news"></ul>`;
    let k = 0;
    setInterval(() => {
      const ul = document.getElementById("news");
      if (!ul) return;
      ul.insertAdjacentHTML("afterbegin", `<li>${new Date().toLocaleTimeString("zh-TW")} 公告 #${++k}（模擬）</li>`);
      while (ul.children.length > 8) ul.lastChild.remove();
    }, 1000);
  } else {
    slot.innerHTML = `<div class="big" id="n-${id}">0</div><div>每幀 +1（requestAnimationFrame）</div>
      <p>代號 <input id="order-sym" placeholder="例：2330"> 張數 <input id="order-qty" value="1" style="width:50px">
      <button id="order-btn">送出（模擬）</button></p><p id="order-log" style="color:#8899aa"></p>`;
    document.getElementById("order-btn").onclick = () => {
      document.getElementById("order-log").textContent = `已送出（模擬）：${document.getElementById("order-sym").value} × ${document.getElementById("order-qty").value} 張`;
    };
  }
  const w = { n: 0, alive: true };
  const loop = () => {
    if (!w.alive) return;
    w.n++;
    const el = document.getElementById("n-" + id);
    if (el) el.textContent = w.n;
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
  return { frames: () => w.n, panic: () => {}, destroy: () => { w.alive = false; } };
}

async function post(obj) {
  const body = JSON.stringify({ url: location.search, userAgent: navigator.userAgent, ...obj });
  if (!obj.mark) console.log("XQWEB " + body);
  try {
    await fetch("/__bench", { method: "POST", body, headers: { "Content-Type": "application/json" } });
  } catch (e) {
    console.log("XQWEB post 失敗（一般靜態伺服器沒有 /__bench，結果只印在 console）");
  }
}

// 每格在 ms 內畫了幾幀 → 每秒幀數
async function rates(xq, ms) {
  const a = xq.frames();
  const t = performance.now();
  await sleep(ms);
  const b = xq.frames();
  const s = (performance.now() - t) / 1000;
  return Object.fromEntries(Object.keys(a).map((k) => [k, typeof a[k] === "number" ? +((b[k] - a[k]) / s).toFixed(1) : null]));
}

function memory(xq) {
  const m = performance.memory; // Chrome／Edge 才有，Safari、Firefox 是 undefined
  return {
    jsHeapUsedMB: m ? +(m.usedJSHeapSize / 1048576).toFixed(1) : null,
    jsHeapTotalMB: m ? +(m.totalJSHeapSize / 1048576).toFixed(1) : null,
    wasmMemoryMB: +(xq.wasmBytes() / 1048576).toFixed(1),
  };
}

// 自動壓測：暖機 3 秒 → 量 5 秒幀數 → 記憶體 →（full）移除一格、再量 3 秒 → 讓報價表 panic、再量 3 秒
async function bench(xq) {
  const r = { framework: xq.framework, mode: xq.mode, layout: xq.layout, ids: xq.ids, readyMs: +xq.readyMs.toFixed(0), gpu: !!navigator.gpu, dpr: devicePixelRatio, screen: `${screen.width}x${screen.height}` };
  const gl = document.createElement("canvas").getContext("webgl2");
  const dbg = gl && gl.getExtension("WEBGL_debug_renderer_info");
  r.webglRenderer = dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : null;
  await sleep(3000);
  await post({ mark: "fps-start" }); // 壓測包在這兩個時間點量瀏覽器的 CPU
  r.fps = await rates(xq, 5000);
  await post({ mark: "fps-end" });
  r.memory = memory(xq);
  if (q.get("steps") !== "basic" && xq.ids.length > 2) {
    const victim = xq.layout === "mixed" ? "webB" : "c2";
    xq.remove(victim, true);
    r.removed = victim;
    r.fpsAfterRemove = await rates(xq, 3000);
    xq.panic("table");
    await sleep(500);
    r.fpsAfterPanic = await rates(xq, 3000);
    r.panicked = xq.panicked();
  }
  r.errors = xq.errors.slice(0, 20);
  await post({ result: r });
}
