"""Micro-average word error (sum edit distance / sum reference words); never omit a failed clip."""
import csv, re, statistics, unicodedata, sys
from pathlib import Path

def words(s):
    s=unicodedata.normalize("NFC",s).casefold().replace("’","'")
    return re.findall(r"[^\W_]+(?:'[^\W_]+)*",s,re.UNICODE)
def distance(a,b):
    d=list(range(len(b)+1))
    for i,x in enumerate(a,1):
        n=[i]
        for j,y in enumerate(b,1):
            n.append(min(n[-1]+1,d[j]+1,d[j-1]+int(x!=y)))
        d=n
    return d[-1]
def summarize(rows):
    def wer(rs):
        return 100*sum(r["errors"] for r in rs)/sum(r["words"] for r in rs) if rs else None
    result={k:wer([r for r in rows if fn(r)]) for k,fn in {
        "all":lambda r:True,"completed":lambda r:r["status"]=="ok","clean":lambda r:r["snr_db"]=="clean","music":lambda r:r["snr_db"]!="clean",
        **{l:(lambda r,l=l:r["lang"]==l) for l in ("en","es","de","no")}}.items()}
    times=sorted(float(r["wall_ms"]) for r in rows if r["wall_ms"]!="" and r["status"]=="ok")
    result.update(ok=len(times),p50_ms=statistics.median(times) if times else None,
        p95_ms=times[(len(times)*95+99)//100-1] if times else None,
        peak_mib=max((int(r["hwm_kib"])/1024 for r in rows if r["hwm_kib"]),default=None))
    return {k:round(v,2) if v is not None else None for k,v in result.items()}
if __name__=="__main__":
    base=Path(__file__).parent
    with (base/"corpus.tsv").open() as f: corpus={r["id"]:r for r in csv.DictReader(f,delimiter="\t")}
    data=Path(sys.argv[1])
    fields="engine id lang snr_db words errors wall_ms cpu_ms load_ms hwm_kib ready_to_result_ms stop_to_result_ms hypothesis status".split()
    out=csv.DictWriter(sys.stdout,fieldnames=fields,delimiter="\t",lineterminator="\n");out.writeheader()
    for name in ("tiny","base","small","small-short","parakeet","android"):
        if not (data/(name+".tsv")).exists():continue
        rows=[]
        for x in (data/(name+".tsv")).read_text().splitlines():
            if name=="android":
                id,wall,status,ready,stop,text=x.split("\t",5);cpu=load=hwm=""
            else:
                id,wall,cpu,load,hwm,text=x.split("\t",5); status="ok" if text.strip() else "empty";ready=stop=""
            c=corpus[id]; ref=words(c["reference"])
            r=dict(zip(fields,[name,id,c["lang"],c["snr_db"],len(ref),distance(ref,words(text)),wall,cpu,load,hwm,ready,stop,text.strip(),status]))
            rows.append(r);out.writerow(r)
        assert len(rows)==len(corpus) and len({r["id"] for r in rows})==len(corpus)
        print(name,summarize(rows),file=sys.stderr)
