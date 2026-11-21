"""Run against the foreground SystemProbe. All missing/error outcomes are retained."""
import csv, subprocess, sys, time
import grpc
from pathlib import Path
from inject import inject

def adb(*args):
    return subprocess.check_output(["adb",*args],text=True,stderr=subprocess.DEVNULL).strip()
def event(token):
    lines=adb("logcat","-d","-s","HexMeasure:I").splitlines()
    return [l.split(token+"|",1)[1] for l in lines if token+"|" in l]
def start(op,token,lang):
    adb("shell","am","start","-n","local.hex.measure/.SystemProbe","--es","op",op,"--es","id",token,"--es","lang",lang)
def uptime_ms():
    return float(adb("shell","cat","/proc/uptime").split()[0])*1000
def trial(row):
    token="clip"+row["id"]+"_"+str(time.monotonic_ns())
    locale={"en":"en-US","es":"es-ES","de":"de-DE","no":"nb-NO"}[row["lang"]]
    start("recognize",token,locale)
    end=None;deadline=time.monotonic()+7
    while time.monotonic()<deadline:
        for e in event(token):
            if e.startswith("error") or e=="unavailable": return [row["id"],"",e,"","",""]
            if e.startswith("result "):
                _,now,ready,stop,text=e.split(" ",4)
                if end is None: return [row["id"],"","before audio","","",""]
                return [row["id"],round(max(0,int(now)-end)), "ok" if text else "empty",int(now)-int(ready),int(now)-int(stop) if int(stop) else "",text]
        if end is None and any(e.startswith("ready ") for e in event(token)):
            time.sleep(.12)
            try:
                inject(Path("wavs")/(row["id"]+".wav"))
            except grpc.RpcError as e:
                failure = next((x for x in event(token) if x.startswith("error")), None)
                start("stop",token,locale)
                return [row["id"],"",failure or ("audio delivery " + e.code().name),"","",""]
            end=uptime_ms()
            start("stop",token,locale)
            deadline=time.monotonic()+8
        time.sleep(.04)
    return [row["id"],"","timeout after stop" if end else "no ready callback","","",""]
if __name__=="__main__":
    assert adb("shell","settings","get","global","airplane_mode_on")=="1"
    with open(Path(__file__).with_name("corpus.tsv")) as f: rows=list(csv.DictReader(f,delimiter="\t"))
    for row in rows if len(sys.argv)==1 else [r for r in rows if r["id"] in sys.argv[1:]]:
        print(*trial(row),sep="\t",flush=True)
