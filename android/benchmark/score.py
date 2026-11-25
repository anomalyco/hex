"""Micro-average word error (sum edit distance / sum reference words); never omit a failed clip."""
import csv, re, statistics, unicodedata, sys
from pathlib import Path

# A prompt on a short clip (README): the words the app puts in front of a dictation as Whisper's prompt (custom
# vocabulary, or Labs › Learn from my fixes' meant words, joined with ", "). Id, language, macOS voice and rate, what is
# said, ms of silence before and after it, then the prompts; generate.py writes the clips, and the check fails if any
# comes out repeated. hint-sarah2 is cut so that "Sarah, stadium" writes the sentence exactly twice, then stops.
PROMPTED=[("hint-sarah","en","Samantha","175","Sarah is at the station.",200,400,("stadium","Sarah, stadium","Sarah, stadium, Hex, OpenCode")),
          ("hint-sarah2","en","Samantha","175","Sarah is at the station.",160,240,("Sarah, stadium",))]

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
def matched(a,b):
    """Which words of a the cheapest alignment with b keeps."""
    d=[[0]*(len(b)+1) for _ in range(len(a)+1)]
    for i in range(len(a)+1): d[i][0]=i
    for j in range(len(b)+1): d[0][j]=j
    for i in range(1,len(a)+1):
        for j in range(1,len(b)+1): d[i][j]=min(d[i-1][j]+1,d[i][j-1]+1,d[i-1][j-1]+int(a[i-1]!=b[j-1]))
    i,j,kept=len(a),len(b),[False]*len(a)
    while i>0 and j>0:
        if d[i][j]==d[i-1][j-1]+int(a[i-1]!=b[j-1]): kept[i-1]=a[i-1]==b[j-1]; i-=1; j-=1
        elif d[i][j]==d[i-1][j]+1: i-=1
        else: j-=1
    return kept
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
    if sys.argv[1]=="--prompted-list":
        for id,lang,_,_,_,_,_,prompts in PROMPTED:
            for p in prompts: print(id,lang,p)
        sys.exit(0)
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
    # The long clips at the app's encoder window against the full one (README, Long clips): fails if any clip is worse.
    with (base/"long.tsv").open() as f: long={r["id"]:r for r in csv.DictReader(f,delimiter="\t")}
    errors={}; texts={}; failed=False
    for name in ("long-full","long-app"):
        if not (data/(name+".tsv")).exists():continue
        errors[name]={}
        for x in (data/(name+".tsv")).read_text().splitlines():
            id,wall,cpu,load,hwm,text=x.split("\t",5); c=long[id]
            ref=words(" ".join(corpus[i]["reference"] for i in c["clips"].split(",")))
            e=distance(ref,words(text)); errors[name][id]=(e,len(ref)); texts[(name,id)]=text
            out.writerow(dict(zip(fields,[name,id,c["lang"],"",len(ref),e,wall,cpu,load,hwm,"","",text.strip(),"ok" if text.strip() else "empty"])))
        assert set(errors[name])==set(long)
        print(name,"errors",sum(e for e,_ in errors[name].values()),"of",sum(n for _,n in errors[name].values()),"words",file=sys.stderr)
    if len(errors)==2:
        worse=[f"{id} ({errors['long-full'][id][0]} -> {e})" for id,(e,_) in errors["long-app"].items() if e>errors["long-full"][id][0]]
        print("long clips: no clip worse than the full window" if not worse else "long clips: worse than the full window: "+", ".join(worse),file=sys.stderr)
        # Past 30 s (more than one Whisper window), every utterance must come out: at most half its words missing.
        lost=[]
        for id,c in long.items():
            if float(c["seconds"])<=30: continue
            parts=c["clips"].split(","); ref=[]; owner=[]
            for k,i in enumerate(parts): w=words(corpus[i]["reference"]); ref+=w; owner+=[k]*len(w)
            kept=matched(ref,words(texts[("long-app",id)]))
            for k,i in enumerate(parts):
                idx=[n for n,o in enumerate(owner) if o==k]
                if 2*sum(kept[n] for n in idx)<len(idx): lost.append(f"{id}: utterance {k+1} ({i})")
        print("long clips past 30 s: every utterance kept" if not lost else "long clips past 30 s: utterances lost: "+", ".join(lost),file=sys.stderr)
        failed=failed or bool(worse or lost)
    # The prompted clips at the app's decode: none may come out repeated, more than 1.5 times the words said.
    if (data/"prompted-app.tsv").exists():
        cases=[(id,lang,ref,p) for id,lang,_,_,ref,_,_,prompts in PROMPTED for p in prompts]
        lines=(data/"prompted-app.tsv").read_text().splitlines()
        assert len(lines)==len(cases)
        repeated=[]
        for (id,lang,ref,p),x in zip(cases,lines):
            rid,wall,cpu,load,hwm,text=x.split("\t",5); assert rid==id
            r=words(ref); h=words(text)
            out.writerow(dict(zip(fields,["prompted-app",id,lang,"clean",len(r),distance(r,h),wall,cpu,load,hwm,"","",text.strip(),"ok" if text.strip() else "empty"])))
            if 2*len(h)>3*len(r): repeated.append(f'{id} prompted "{p}" ({len(h)} words for {len(r)})')
        print("prompted clips: none repeated" if not repeated else "prompted clips: repeated: "+", ".join(repeated),file=sys.stderr)
        failed=failed or bool(repeated)
    # Labs › Automatic listens for the language (README): with a clip's own language as the keyboard's, no other language
    # may take a clip over. switch.tsv is native.cpp's HEX_LANGS output: id, language, other, chosen, other's probability.
    if (data/"switch.tsv").exists():
        pairs=[x.split("\t") for x in (data/"switch.tsv").read_text().splitlines()]
        switched=[f"{id} ({lang}) to {chosen}, {float(p):.3f}" for id,lang,other,chosen,p,_ in pairs if chosen!=lang]
        print(f"automatic language: {len(pairs)-len(switched)} of {len(pairs)} pairs kept the clip's language" +
              ("" if not switched else "; switched: "+", ".join(switched)),file=sys.stderr)
        failed=failed or bool(switched)
    if failed: sys.exit(1)
