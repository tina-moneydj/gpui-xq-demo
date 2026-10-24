#!/usr/bin/env python3
"""網頁版 vs 原生對照表（Markdown）。原生數字取自 docs/bench/2026-10-21-parity（同一套情境，2026-10-21 晚上那一輪）。
用法：tables.py <web summary.json> <parity 資料夾>"""
import json, sys
W = json.load(open(sys.argv[1])); P = sys.argv[2]
NM = json.load(open(f"{P}/main/summary.json")); NH = json.load(open(f"{P}/heavy/summary-heavy.json")); NT = json.load(open(f"{P}/ticks/ticks-summary.json"))["medians"]
f0 = lambda v: "—" if v is None else f"{v:,.0f}"
f1 = lambda v: "—" if v is None else f"{v:,.1f}"
f2 = lambda v: "—" if v is None else f"{v:.2f}"
cols = [("egui 網頁", W.get("egui-web", {})), ("GPUI 網頁", W.get("gpui-web", {}))]
nat = ["egui", "gpui", "wry"]
hdr = "| 項目 | " + " | ".join(c for c, _ in cols) + " | egui 原生 | GPUI 原生 | wry（WebKitGTK） |\n|---|" + "---|" * (len(cols) + 3)
out = []
out.append("#### 主視窗（33 檔自選、日K＋MA＋量，demo-ticks 行情）\n\n" + hdr)
row = lambda name, web, natv: out.append(f"| {name} | " + " | ".join(web) + " | " + " | ".join(natv) + " |")
m = lambda c, k: c.get("main", {}).get(k)
row("開啟→第一筆報價 (秒)", [f2(m(c, "first_quote_s")) for _, c in cols], [f2(NM[a]["baseline"]["first_quote_ms"] / 1000) for a in nat])
row("PSS 60 秒 (MB)", [f0(m(c, "pss60_mb")) for _, c in cols], [f0(NM[a]["baseline"]["pss60_mb"]) for a in nat])
row("RSS 60 秒 (MB)", [f0(m(c, "rss60_mb")) for _, c in cols], [f0(NM[a]["baseline"]["rss60_mb"]) for a in nat])
row("　其中 wasm 記憶體 (MB)", [f0(m(c, "wasm60_mb")) for _, c in cols], ["—"] * 3)
row("CPU 全部 (%)", [f0(m(c, "cpu_pct")) for _, c in cols], [f0(NM[a]["baseline"]["cpu_pct"]) for a in nat])
def nollvm_native(a):
    th = NM[a]["baseline"].get("cpu_threads_last") or {}
    return NM[a]["baseline"]["cpu_pct"] - sum(v for k, v in th.items() if "llvmpipe" in k)
row("CPU 扣軟體繪圖 (%)", [f0(m(c, "cpu_nollvm_pct")) for _, c in cols], [f0(nollvm_native(a)) for a in nat])
row("FPS", [f1(m(c, "fps")) for _, c in cols], [f1(NM[a]["baseline"]["fps"]) for a in nat])

out.append("\n#### 5 萬列報價表（每秒 3000 筆合成跳價 30 秒，再邊跳邊捲 10 秒）\n\n" + hdr)
s = lambda c, k: c.get("stress", {}).get(k)
row("跳價 FPS", [f1(s(c, "tick_fps")) for _, c in cols], [f1(NM[a]["stress"]["tick_fps"]) for a in nat])
row("捲動 FPS", [f1(s(c, "scroll_fps")) for _, c in cols], [f1(NM[a]["stress"]["scroll_fps"]) for a in nat])
row("捲動卡頓次數（>16.7ms×2）", [f0(s(c, "scroll_jank")) for _, c in cols], [f0(NM[a]["stress"]["scroll_jank_count"]) for a in nat])
row("跳價 CPU 全部／扣軟體繪圖 (%)", [f"{f0(s(c,'cpu_tick_pct'))}／{f0(s(c,'cpu_tick_nollvm_pct'))}" for _, c in cols],
    [f"{f0(NM[a]['stress']['cpu_tick_pct'])}／{f0(NM[a]['stress']['cpu_tick_pct'] - sum(v for k, v in (NM[a]['stress'].get('cpu_tick_threads_last') or {}).items() if 'llvmpipe' in k))}" for a in nat])
row("PSS 跳價中 (MB)", [f0(s(c, "pss_tick_mb")) for _, c in cols], [f0(NM[a]["stress"]["pss_tick_mb"]) for a in nat])

out.append("\n#### 重圖表（20 疊加＋8 副圖＋1000 條畫線，每幀跳價／十字線／平移各 10 秒）\n\n" + hdr)
h = lambda c, ph, k: c.get("heavy", {}).get(ph, {}).get(k)
nh = lambda a, ph, k: NH[a]["heavy"][ph][k]
row("開啟→重圖表第一幀 (秒)", [f2(c.get("heavy", {}).get("heavy_frame_s")) for _, c in cols], [f2(NH[a]["heavy"]["heavy_frame_ms"] / 1000) for a in nat])
row("FPS 跳價／十字線／平移", ["／".join(f1(h(c, ph, "fps")) for ph in ("ticks", "crosshair", "pan")) for _, c in cols],
    ["／".join(f1(nh(a, ph, "fps")) for ph in ("ticks", "crosshair", "pan")) for a in nat])
row("CPU 全部 (%)（十字線段）", [f0(h(c, "crosshair", "cpu_pct")) for _, c in cols], [f0(nh(a, "crosshair", "cpu_pct")) for a in nat])
row("CPU 扣軟體繪圖 (%)（十字線段）", [f0(h(c, "crosshair", "cpu_nollvm_pct")) for _, c in cols], [f0(nh(a, "crosshair", "cpu_nollvm_pct")) for a in nat])
row("PSS（十字線段，MB）", [f0(h(c, "crosshair", "pss_mb")) for _, c in cols], [f0(nh(a, "crosshair", "pss_mb")) for a in nat])
row("　其中 wasm 記憶體 (MB)", [f0(h(c, "crosshair", "wasm_mb")) for _, c in cols], ["—"] * 3)

out.append("\n#### 高頻報價（xq-tickgen，5 萬列表＋2330 日K；延遲＝產生器送出→畫面，中位數／p99，ms）\n\n" + hdr)
for sc, name in (("s1k", "每秒 1 千筆"), ("s10k", "每秒 1 萬筆"), ("s50k", "每秒 5 萬筆"), ("s100k", "每秒 10 萬筆"), ("burst", "開盤爆量（前 3 秒約 30 萬筆）")):
    t = lambda c, k: c.get(f"ticks_{sc}", {}).get(k)
    nt = lambda a, k: NT.get(f"{a}/{sc}", {}).get(k)
    row(f"{name} 延遲", [f"{f0(t(c,'lat_p50'))}／{f0(t(c,'lat_p99'))}" for _, c in cols], [f"{f0(nt(a,'lat_oldest_p50'))}／{f0(nt(a,'lat_oldest_p99'))}" for a in nat])
row("每秒 10 萬筆 掉筆", [str(max(c.get("ticks_s100k", {}).get("loss") or [None], key=lambda x: x or 0)) for _, c in cols], [str(NT.get(f"{a}/s100k", {}).get("lossFinal")) for a in nat])
row("每秒 10 萬筆 CPU 全部／扣軟體繪圖 (%)", [f"{f0(c.get('ticks_s100k', {}).get('cpu_pct'))}／{f0(c.get('ticks_s100k', {}).get('cpu_nollvm_pct'))}" for _, c in cols],
    [f"{f0(NT.get(f'{a}/s100k', {}).get('cpu'))}／{f0(NT.get(f'{a}/s100k', {}).get('cpuExLlvmpipe'))}" for a in nat])
row("每秒 10 萬筆 FPS", [f1(c.get("ticks_s100k", {}).get("fps")) for _, c in cols], [f1(NT.get(f"{a}/s100k", {}).get("fps")) for a in nat])
row("每秒 10 萬筆 PSS 結束 (MB)", [f0(c.get("ticks_s100k", {}).get("pss_end_mb")) for _, c in cols], [f0(NT.get(f"{a}/s100k", {}).get("pssEnd")) for a in nat])
print("\n".join(out))
