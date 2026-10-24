import json, statistics, re, collections
MAC='2026-10-24-mac/raw/results.json'
WIN='2026-10-24-windows/raw/results.json'
def win(a_val, b_val, hib, kind, refresh):
    """return 'A','B','tie' comparing GPUI(a) vs egui(b)"""
    if a_val is None or b_val is None: return None
    if a_val == b_val: return 'tie'
    if kind=='fps' and min(a_val,b_val) >= 0.95*refresh: return 'tie'
    if kind=='paint' and abs(a_val-b_val) < 0.5: return 'tie'
    if abs(a_val-b_val) <= 0.05*max(abs(a_val),abs(b_val)): return 'tie'
    better_a = (a_val > b_val) if hib else (a_val < b_val)
    return 'A' if better_a else 'B'
def kind_of(metric, unit):
    m=metric.lower()
    if 'fps' in m: return 'fps'
    if 'paint' in m: return 'paint'
    return ''
out={}
# ---- Mac
d=json.load(open(MAC))
rows=[]
for g,v in d['summary'].items():
    for m in v['metrics']:
        ap=m['apps']; k=kind_of(m['metric'],m['unit'])
        g_=ap['gpui']['median']; gl=ap['egui']['median']; gw=ap['egui-wgpu']['median']
        rows.append(dict(group=g,title=v['title'],metric=m['metric'],label=m['label'],unit=m['unit'],hib=m['higher_is_better'],
            gpui=g_,egui_glow=gl,egui_wgpu=gw,
            vs_glow=win(g_,gl,m['higher_is_better'],k,60), vs_wgpu=win(g_,gw,m['higher_is_better'],k,60),
            runs={a:ap[a]['runs'] for a in ap}, kit_winner=m['winner']))
out['mac']=rows
# ---- Windows
w=json.load(open(WIN,encoding='utf-8-sig'))
M=w['medians']
spec=[('main','主視窗',[('first_quote_ms','啟動到第一筆報價','ms',False),('first_frame_ms','啟動到第一幀','ms',False),('ws60_mb','Working Set 60 秒','MB',False),('wsPrivate60_mb','Working Set-Private 60 秒','MB',False),('private60_mb','Private Bytes 60 秒','MB',False),('cpu_pct','CPU','%',False),('gpu3d','GPU 3D','%',False),('gpuDedicated_mb','GPU 專用記憶體','MB',False),('fps','FPS','fps',True),('threads','執行緒','個',False)]),
 ('stress','5 萬列報價表',[('cpu_tick_pct','CPU 跳價','%',False),('cpu_scroll_pct','CPU 捲動','%',False),('gpu3d_tick','GPU 3D 跳價','%',False),('tick_fps','FPS 跳價','fps',True),('scroll_fps','FPS 捲動','fps',True),('scroll_jank','捲動卡頓次數','次',False),('ws_tick_mb','Working Set 跳價末','MB',False),('tick_paint_ms','表格重畫耗時 跳價','ms',False)])]
rows=[]
for sc,title,ms in spec:
    for key,label,unit,hib in ms:
        a=M['gpui'][sc].get(key); b=M['egui'][sc].get(key)
        k='fps' if unit=='fps' else ('paint' if 'paint' in key else '')
        rows.append(dict(group=sc,title=title,metric=key,label=label,unit=unit,hib=hib,gpui=a,egui_glow=b,vs_glow=win(a,b,hib,k,59)))
tk=[('lat_p50','延遲 p50','ms',False),('lat_p99','延遲 p99','ms',False),('lat_max','延遲最大','ms',False),('loss','遺失','筆',False),('cpu','CPU','%',False),('gpu3d','GPU 3D','%',False),('fps','FPS','fps',True),('wsEnd','Working Set 結束','MB',False),('switch_p50','點選切換 p50（egui 多數點擊沒對到，僅參考）','ms',False)]
for s in ['s1k','s10k','s50k','s100k','burst']:
    for key,label,unit,hib in tk:
        a=M['gpui']['ticks'][s].get(key); b=M['egui']['ticks'][s].get(key)
        rows.append(dict(group='ticks-'+s,title='高頻進價 '+s,metric=key,label=label,unit=unit,hib=hib,gpui=a,egui_glow=b,vs_glow=win(a,b,hib,'fps' if unit=='fps' else '',59)))
mk=[('lat_p50','延遲 p50（全部視窗）','ms',False),('lat_p99','延遲 p99（全部視窗）','ms',False),('worst_window_p99','最慢視窗 p99','ms',False),('fps_min_window','最慢視窗 FPS','fps',True),('loss','遺失','筆',False),('cpu','CPU','%',False),('gpu3d','GPU 3D','%',False),('gpuDedicated_mb','GPU 專用記憶體','MB',False),('wsEnd','Working Set 結束','MB',False)]
for n in ['w1','w4']:
    for key,label,unit,hib in mk:
        a=M['gpui']['multi'][n].get(key); b=M['egui']['multi'][n].get(key)
        rows.append(dict(group='multi-'+n,title='多視窗 ×%s（每秒 1 萬筆）'%n[1],metric=key,label=label,unit=unit,hib=hib,gpui=a,egui_glow=b,vs_glow=win(a,b,hib,'fps' if unit=='fps' else '',59)))
# heavy partial from app logs
heavy={}
for app in ['gpui','egui']:
    txt=open(f'2026-10-24-windows/raw/{app}-heavy.log',encoding='utf-8-sig',errors='replace').read()
    agg=collections.defaultdict(lambda: collections.defaultdict(list)); setup=[]
    for r in txt.split('==== heavy')[1:]:
        for m in re.finditer(r'CHARTPERF\|(\{.*\})',r):
            j=json.loads(m.group(1))
            if j['phase']=='setup': setup.append(j)
            for k in ['fps','avgPaintMs','rssMb']:
                if k in j: agg[j['phase']][k].append(j[k])
    heavy[app]={ph:{k:round(statistics.median(v),2) for k,v in dd.items()} for ph,dd in agg.items()}
    heavy[app]['_attempts']=len(txt.split('==== heavy'))-1
    heavy[app]['_chart_css']=sorted({(s['cssW'],s['cssH']) for s in setup})
out['windows']=rows; out['windows_heavy_partial']=heavy; out['windows_failures']=w['failures']
json.dump(out,open('analysis.json','w'),ensure_ascii=False,indent=1)
# counts
def cnt(rows,key):
    c=collections.Counter(r[key] for r in rows if r.get(key)); return dict(c)
print('mac vs glow',cnt(out['mac'],'vs_glow'),'mac vs wgpu',cnt(out['mac'],'vs_wgpu'),'win',cnt(out['windows'],'vs_glow'))
print(json.dumps(heavy,ensure_ascii=False))
