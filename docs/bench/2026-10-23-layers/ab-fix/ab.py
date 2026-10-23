import os,subprocess,sys,time,json
# ab.py label dir bin [k=v ...] -> prints ticks/crosshair/pan fps,paint
lab,d,b=sys.argv[1:4]
env=dict(os.environ,DISPLAY=":7",XQ_BENCH="1",XQ_PERIOD="D",XQ_CHART_STRESS="1",XQ_TOP_FRAC="0.8",XQ_CHART_WARM_MS="1000",XQ_CHART_PHASE_MS="6000",XQ_CHART_WAIT_MS="1500",XQ_FEED_ADDR="127.0.0.1:47667")
for kv in sys.argv[4:]:
    k,v=kv.split("=",1); env[k]=v
p=subprocess.Popen([b],cwd=d,env=env,stderr=subprocess.PIPE,stdout=subprocess.DEVNULL,text=True)
time.sleep(1.0)
w=subprocess.run(["xdotool","search","--onlyvisible","--name","^XQ"],env=env,capture_output=True,text=True).stdout.split()
if w: subprocess.run(["xdotool","windowsize","--sync",w[0],"1200","720"],env=env)
res={}
def cpu():
    f=open(f"/proc/{p.pid}/stat").read().rsplit(")",1)[1].split()
    return (int(f[11])+int(f[12]))/os.sysconf("SC_CLK_TCK")
c0={}
for line in p.stderr:
    if "CHARTPHASE|" in line:
        ph,ev=line.strip().split("|")[1:3]
        if ev=="start": c0[ph]=(cpu(),time.time())
        elif ph in c0: res[ph+"_cpu%"]=round(100*(cpu()-c0[ph][0])/(time.time()-c0[ph][1]))
        if ph=="pan" and ev=="end": break
    if "CHARTPERF|{" in line:
        j=json.loads(line.split("CHARTPERF|",1)[1])
        if j.get("phase")=="setup": res["setup"]=(j.get("drawPaths"),j.get("drawVertices"),j.get("visibleDrawings"))
        if j.get("phase") in ("ticks","crosshair","pan","drag"): res[j["phase"]]=(j.get("fps"),j.get("avgPaintMs"),j.get("drawPaints"),j.get("drawMs"),j.get("frames"))
p.terminate()
try: p.wait(10)
except subprocess.TimeoutExpired: p.kill()
print(lab,res,flush=True)
