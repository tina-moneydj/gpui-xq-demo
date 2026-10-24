import re,json,sys,glob
# 對話框開著（Pause→Resume）那段時間裡，PACER frames 增加多少
for f in sorted(glob.glob(sys.argv[1]+"/*.log")):
    L=open(f).read().splitlines(); ev=[]; pc=[]
    for l in L:
        t=float(l.split()[0]) if l.strip() and l.split()[0].replace('.','').isdigit() else None
        if t is None: continue
        m=re.search(r"MainEvent::(\w+)",l)
        if m: ev.append((t,m.group(1)))
        m=re.search(r"PACER\|(\{.*\})",l)
        if m: pc.append((t,json.loads(m.group(1))))
    # 第一個 GainedFocus 之後的第一個 Pause，到下一個 Resume
    g=[t for t,e in ev if e=="GainedFocus"]
    if not g: print(f,"no focus"); continue
    p=[t for t,e in ev if e=="Pause" and t>g[0]]
    if not p: print(f,"no pause"); continue
    r=[t for t,e in ev if e=="Resume" and t>p[0]]
    if not r: print(f,"no resume"); continue
    a=[x for x in pc if p[0]+0.5<=x[0]<=r[0]-0.5]
    if len(a)<2: print(f, "window", round(r[0]-p[0],1),"s, too few PACER samples"); continue
    print(json.dumps({"file":f.split('/')[-1],"dialogSecs":round(r[0]-p[0],1),"framesInWindow":a[-1][1]["frames"]-a[0][1]["frames"],"spanSecs":round(a[-1][0]-a[0][0],1),"canDraw":[x[1]["canDraw"] for x in a]}))
