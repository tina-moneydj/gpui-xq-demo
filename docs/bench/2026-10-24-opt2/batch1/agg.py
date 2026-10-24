import json, glob, os, statistics as st, sys
B = '/workspace/realhw/ilv/raw'
def med(v):
    v = [x for x in v if x is not None]
    return round(st.median(v), 1) if v else None
def load(k, v):
    out = []
    for d in sorted(glob.glob(f'{B}/r*-{v}-{k}')):
        f = f'{d}/results.json' if k != 'multi' else f'{d}/multi-results.json'
        if not os.path.exists(f): continue
        j = json.load(open(f))
        if k == 'main': out += j['baseline']
        elif k == 'stress': out += j['stress']
        else: out += (j if isinstance(j, list) else [j])
    return out
M = {
 'main': [('第一幀 ms', lambda r: r['first_frame_s']*1000, False), ('第一筆報價 ms', lambda r: r['first_quote_s']*1000, False),
          ('RSS 60 秒 MB', lambda r: r['steady60']['rss_mb'], False), ('PSS 60 秒 MB', lambda r: r['steady60']['pss_mb'], False),
          ('CPU 全部 %', lambda r: r['cpu_pct'], False), ('CPU 主執行緒 %（不含 llvmpipe 光柵化）', lambda r: r['cpu_threads'].get('gpui-xq-demo/gpui-xq-demo'), False),
          ('FPS', lambda r: r['fps_avg'], True)],
 'stress': [('FPS 跳價段', lambda r: r['tick_end']['fps'], True), ('FPS 捲動段', lambda r: r['done']['scrollFps'], True),
            ('捲動卡頓次數（>32 ms）', lambda r: r['done']['scrollJankCount'], False),
            ('CPU 跳價段 全部 %', lambda r: r['cpu_tick_pct'], False), ('CPU 跳價段 主執行緒 %', lambda r: r['cpu_tick_threads'].get('gpui-xq-demo/gpui-xq-demo'), False),
            ('CPU 捲動段 全部 %', lambda r: r['cpu_scroll_pct'], False), ('RSS 跳價末 MB', lambda r: r['mem_tick']['rss_mb'], False)],
 'multi': [('延遲 p50 ms（全部視窗）', lambda r: r['lat']['p50'], False), ('延遲 p99 ms（全部視窗）', lambda r: r['lat']['p99'], False),
           ('最慢視窗 p99 ms', lambda r: r['worstP99'], False),
           ('主視窗 p50 ms', lambda r: r['perWindow']['0']['p50'], False),
           ('其他 3 視窗 p50 平均 ms', lambda r: st.mean([r['perWindow'][k]['p50'] for k in ('1','2','3')]), False),
           ('其他視窗比主視窗慢 ms（p50 差）', lambda r: st.mean([r['perWindow'][k]['p50'] for k in ('1','2','3')]) - r['perWindow']['0']['p50'], False),
           ('最慢視窗 FPS', lambda r: r['fpsMin'], True), ('CPU 全部 %', lambda r: r['cpu'], False),
           ('CPU 不含 llvmpipe %', lambda r: r['cpuExLlvmpipe'], False), ('RSS 結束 MB', lambda r: r['mem']['end']['rss_mb'] if isinstance(r['mem'].get('end'),dict) else r['mem'].get('rssEnd', r['mem'].get('end')), False)],
}
res = {}
for k, mets in M.items():
    res[k] = []
    rb, ra = load(k, 'before'), load(k, 'after')
    for name, f, hib in mets:
        def vals(rs):
            o = []
            for r in rs:
                try: o.append(f(r))
                except Exception: o.append(None)
            return o
        vb, va = vals(rb), vals(ra)
        res[k].append({'metric': name, 'higher_is_better': hib, 'before': med(vb), 'after': med(va),
                       'before_runs': [round(x,1) if x is not None else None for x in vb], 'after_runs': [round(x,1) if x is not None else None for x in va]})
json.dump(res, open('/workspace/realhw/ilv/summary.json', 'w'), ensure_ascii=False, indent=1)
for k, rows in res.items():
    print('##', k)
    for r in rows: print(f"  {r['metric']}: before {r['before']} {r['before_runs']} → after {r['after']} {r['after_runs']}")
