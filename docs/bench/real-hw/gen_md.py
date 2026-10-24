import json
A=json.load(open('analysis.json'))
W={'A':'GPUI','B':'egui','tie':'平手',None:'—'}
def f(v,u):
    if v is None: return '—'
    if isinstance(v,float) and v.is_integer(): v=int(v)
    return f'{v} {u}'.strip()
def mark(v,u,res,side):
    s=f(v,u)
    return f'**{s}** 🏆' if res==side else s
def mac_md():
    L=[]; cur=None
    for r in A['mac']:
        if r['title']!=cur:
            cur=r['title']; L+=['',f'### {cur}','','| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |','|---|---:|---:|---:|---|---|']
        best = r['vs_glow']; 
        L.append(f"| {r['label']}{'（越高越好）' if r['hib'] else ''} | {f(r['gpui'],r['unit'])} | {f(r['egui_wgpu'],r['unit'])} | {f(r['egui_glow'],r['unit'])} | {W[r['vs_glow']]} | {W[r['vs_wgpu']]} |")
    return '\n'.join(L)
def win_md():
    L=[]; cur=None
    for r in A['windows']:
        if r['title']!=cur:
            cur=r['title']; L+=['',f'### {cur}','','| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |','|---|---:|---:|---|']
        L.append(f"| {r['label']}{'（越高越好）' if r['hib'] else ''} | {mark(r['gpui'],r['unit'],r['vs_glow'],'A')} | {mark(r['egui_glow'],r['unit'],r['vs_glow'],'B')} | {W[r['vs_glow']]} |")
    h=A['windows_heavy_partial']
    L+=['','### 重圖表（壓測包逾時，以下取自程式自己的 log，只當參考）','','| 段落 | GPUI FPS | egui FPS | GPUI 每幀繪圖 (ms) | egui 每幀繪圖 (ms) | GPUI RSS (MB) | egui RSS (MB) |','|---|---:|---:|---:|---:|---:|---:|']
    for ph,zh in [('ticks','跳價'),('crosshair','十字線'),('pan','平移'),('drag','拖曳畫線')]:
        g=h['gpui'][ph]; e=h['egui'][ph]
        L.append(f"| {zh} | {g['fps']} | {e['fps']} | {g['avgPaintMs']} | {e['avgPaintMs']} | {g['rssMb']} | {e['rssMb']} |")
    return '\n'.join(L)
open('2026-10-24-mac/tables.md','w').write(mac_md())
open('2026-10-24-windows/tables.md','w').write(win_md())
import collections
for k,rows,key in [('mac-glow',A['mac'],'vs_glow'),('mac-wgpu',A['mac'],'vs_wgpu'),('win',A['windows'],'vs_glow')]:
    print(k,collections.Counter(W[r[key]] for r in rows))
