package local.hex.bench;

import ly.anoma.hex.typing.*;
import org.json.*;
import java.io.*;
import java.util.*;

/** Runs via app_process against the actual installed APK. Never built into Hex. */
public final class Replay {
    static KeyGrid originalGeometry() {
        // Portrait KeyboardLayout at 402 pt (side 6.5, gap 6, letter height 45, row gap 11).
        double pitch = 39.5; Map<Character,KeyPoint> points = new LinkedHashMap<>();
        String[] rows = {"qwertyuiop", "asdfghjkl", "zxcvbnm"};
        for (int row=0; row<rows.length; row++) {
            String letters=rows[row];
            for (int i=0; i<letters.length(); i++)
                points.put(letters.charAt(i),new KeyPoint(201 + (i-(letters.length()-1)/2.0)*pitch,66.5+56*row));
        }
        return new KeyGrid(points, pitch, 56);
    }
    static String[] words(String s) {
        return Arrays.stream(s.toLowerCase(Locale.ROOT).trim().split("\\s+"))
                .map(w -> w.replaceAll("[.,!?;:]+$", "")).filter(w -> !w.isEmpty()).toArray(String[]::new);
    }
    static int edits(String[] ref, String[] hyp) {
        int[] d = new int[hyp.length+1]; for(int j=0;j<d.length;j++) d[j]=j;
        for(int i=1;i<=ref.length;i++) {
            int prev=d[0]; d[0]=i;
            for(int j=1;j<d.length;j++) {
                int old=d[j]; d[j]=Math.min(Math.min(d[j]+1,d[j-1]+1),prev+(ref[i-1].equals(hyp[j-1])?0:1));
                prev=old;
            }
        }
        return d[hyp.length];
    }
    static void latency(String name, List<Long> ns) {
        Collections.sort(ns);
        System.out.printf(Locale.ROOT,"latency\t%s\t%d\t%.4f\t%.4f\t%.4f\n",
            name,ns.size(),ns.get(ns.size()/2)/1e6,ns.get(ns.size()*95/100)/1e6,ns.get(ns.size()-1)/1e6);
    }
    public static void main(String[] args) throws Exception {
        TypingModel model=TypingModel.Companion.file(new File(args[0]),"en");
        KeyGrid grid=originalGeometry(); TypingWord state=new TypingWord(new SpatialDecoder(model));
        List<Long> letters=new ArrayList<>(), spaces=new ArrayList<>();
        int count=0, total=0; int[] errors=new int[5], exact=new int[5];
        try (BufferedReader reader=new BufferedReader(new FileReader(args[1]))) {
            String line;
            while((line=reader.readLine())!=null) {
                JSONObject r=new JSONObject(line);
                if(!r.getString("id").equals("test-"+count)) throw new IllegalArgumentException("Unexpected row "+count);
                String typed=r.getString("typed"); JSONArray taps=r.getJSONArray("taps");
                if(typed.length()!=taps.length()) throw new IllegalArgumentException("Unaligned taps");
                StringBuilder out=new StringBuilder(); state.reset("");
                for(int i=0;i<typed.length();i++) {
                    char key=typed.charAt(i); double x=taps.getJSONArray(i).getDouble(0), y=taps.getJSONArray(i).getDouble(1);
                    long start=System.nanoTime();
                    if(key==' ') {
                        String raw=state.getToken(), fix=state.replacement();
                        if(fix!=null) out.replace(out.length()-raw.length(),out.length(),fix);
                        out.append(' '); String before=out.toString(); state.reset(before); state.predictions(before);
                    } else {
                        state.letter(String.valueOf(key),x,y,grid); out.append(key); state.suggestions();
                    }
                    (key==' '?spaces:letters).add(System.nanoTime()-start);
                }
                String[] ref=words(r.getString("intended")); total+=ref.length; count++;
                String[] results={typed,r.getString("ios_previous"),r.getString("ios_spatial"),r.getString("ios_calibrated"),out.toString()};
                for(int i=0;i<5;i++) { int err=edits(ref,words(results[i])); errors[i]+=err; if(err==0) exact[i]++; }
            }
        }
        if(count!=2000) throw new IllegalArgumentException("Incomplete synthetic set: "+count);
        String[] names={"raw","ios_previous","ios_spatial","ios_calibrated","android_spatial"};
        for(int i=0;i<5;i++)
            System.out.printf(Locale.ROOT,"wer\t%s\t%d\t%d\t%.4f\t%d/%d\n",names[i],errors[i],total,100.0*errors[i]/total,exact[i],count);
        latency("letter",letters); latency("space",spaces);
    }
}
