# 2026-10-23 圖層版退步 A/B（修正前後輪流測）

- DISPLAY=:7（Xvfb）、WryFeedHost --demo-ticks 在 127.0.0.1:47667，視窗 1200×720，XQ_CHART_PHASE_MS=6000，1000 條畫線（預設 100 層）。
- GPUI：ab.py；before=360cb93、after=0eeca4e、fix=修正版。results.txt＝第一組（只有 FPS），results-cpu.txt＝第二組（加整個程式 CPU%）。
  tuple = (fps, avgPaintMs, drawPaints, drawMs, frames)。
- egui／wry：ab_app.py／ab_app2.py（CPU 為整個行程樹，wry 含 WebKitWebProcess）。egui old=3c5d06d、wry old=d9e06ec；new-L1 = XQ_CHART_LAYERS=1。
- 每幀畫線批次（4 → 14 → 4）是另外在 gpui-pre-wgpu 加計數（只量測用，沒有提交）。
