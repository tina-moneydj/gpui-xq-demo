import os,subprocess,sys,time,json
# ab_app.py app label dir bin  -> ticks/crosshair/pan fps + process-tree CPU%
app,lab,d,b=sys.argv[1:5]
env=dict(os.environ,DISPLAY=":7",XQ_BENCH="1",XQ_PERIOD="D",XQ_CHART_STRESS="1",XQ_TOP_FRAC="0.8",XQ_CHART_TOP_RATIO="0.8",
 XQ_CHART_WARM_MS="1000",XQ_CHART_PHASE_MS="6000",XQ_CHART_WAIT_MS="1500",XQ_CHART_DRAWINGS="1000",XQ_FEED_ADDR="127.0.0.1:47667",
 WEBKIT_DISABLE_COMPOSITING_MODE="1",XQ_RENDERER="glow",XQ_EGUI_STATE="/tmp/ab2/egui-state.json")
for kv in sys.argv[5:]:
    k,v=kv.split("=",1); env[k]=v
p=subprocess.Popen([b],cwd=d,env=env,stderr=subprocess.PIPE,stdout=subprocess.DEVNULL,text=True)
tck=os.sysconf("SC_CLK_TCK")
def tree_cpu():
    st={}
    for pid in os.listdir("/proc"):
        if not pid.isdigit(): continue
        try: f=open(f"/proc/{pid}/stat").read().rsplit(")",1)[1].split()
        except Exception: continue
        st[int(pid)]=(int(f[1]),int(f[11])+int(f[12]))
    tot=0; todo=[p.pid]; seen=set()
    while todo:
        x=todo.pop(); 
        if x in seen: continue
        seen.add(x); tot+=st.get(x,(0,0))[1]
        todo+=[c for c,(pp,_) in st.items() if pp==x]
    return tot/tck
time.sleep(1.5)
w=subprocess.run(["xdotool","search","--onlyvisible","--name","^XQ"],env=env,capture_output=True,text=True).stdout.split()
if w: subprocess.run(["xdotool","windowsize","--sync",w[0],"1200","720"],env=env)
res={}; c0={}; t0=time.time()
for line in p.stderr:
    if "CHARTPHASE|" in line:
        ph,ev=line.strip().split("CHARTPHASE|",1)[1].split("|")[:2]
        if ev=="start": c0[ph]=(tree_cpu(),time.time())
        elif ph in c0: res[ph+"_cpu%"]=round(100*(tree_cpu()-c0[ph][0])/(time.time()-c0[ph][1]))
        if ph=="pan" and ev.startswith("end"): break
    if "CHARTPERF|{" in line:
        j=json.loads(line.split("CHARTPERF|",1)[1].strip())
        if j.get("phase") in ("ticks","crosshair","pan"): res[j["phase"]]=(j.get("fps"),j.get("avgPaintMs"))
    if time.time()-t0>90: res["timeout"]=1; break
p.terminate()
try: p.wait(10)
except subprocess.TimeoutExpired: p.kill()
print(app,lab,res,flush=True)
