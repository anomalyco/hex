package local.hex.bench;

import ly.anoma.hex.typing.*;
import org.json.*;
import java.io.*;
import java.util.*;

/** Runs via app_process against the actual installed APK. Never built into Hex. */
public final class Replay {
    static KeyGrid originalGeometry(String language) {
        // Portrait KeyboardLayout at 402 pt (side 6.5, gap 6, letter height 45, row gap 11), and its
        // space bar. German and Bokmål have Android's eleven-key rows at a 36 pt pitch.
        boolean wide = language.equals("de") || language.equals("nb");
        double pitch = wide ? 36 : 39.5; Map<Character,KeyPoint> points = new LinkedHashMap<>();
        String[] rows = language.equals("de") ? new String[]{"qwertzuiopü", "asdfghjklöä", "yxcvbnm"}
            : language.equals("nb") ? new String[]{"qwertyuiopå", "asdfghjkløæ", "zxcvbnm"}
            : new String[]{"qwertyuiop", language.equals("es") ? "asdfghjklñ" : "asdfghjkl", "zxcvbnm"};
        for (int row=0; row<rows.length; row++) {
            String letters=rows[row];
            for (int i=0; i<letters.length(); i++)
                points.put(letters.charAt(i),new KeyPoint(201 + (i-(letters.length()-1)/2.0)*pitch,66.5+56*row));
        }
        return new KeyGrid(points, pitch, 56, new KeyRect(64.76, 212, 303.1, 257));
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
    // The strip as the keyboard shows it: strip() where the engine has it, else the old
    // suggestions() with its literal chip.
    static List<String> strip(TypingWord state) throws Exception {
        List<String> out = new ArrayList<>();
        try {
            for (Object s : (List<?>) TypingWord.class.getMethod("strip", boolean.class).invoke(state, true))
                out.add((String) s.getClass().getMethod("getText").invoke(s));
        } catch (NoSuchMethodException old) {
            List<String> words = state.suggestions(); String literal = state.literalChip(words);
            if (literal != null) out.add(literal);
            out.addAll(words);
        }
        return out;
    }
    @SuppressWarnings("unchecked")
    static List<String> predictions(TypingWord state, String before) throws Exception {
        try {
            return (List<String>) TypingWord.class.getMethod("predictions", String.class, Boolean.class).invoke(state, before, null);
        } catch (NoSuchMethodException old) {
            return (List<String>) TypingWord.class.getMethod("predictions", String.class).invoke(state, before);
        }
    }
    static boolean hit(List<String> slots, String word) {
        for (String s : slots) if (s.toLowerCase(Locale.ROOT).replace('\u2019', '\'').equals(word)) return true;
        return false;
    }
    public static void main(String[] args) throws Exception {
        String lang=args[2], group=lang.equals("en") ? "test-" : lang+"-test-";
        PrintWriter outputs=args.length>3 ? new PrintWriter(new FileWriter(args[3])) : null;
        boolean hasIos=false;
        TypingModel model=TypingModel.Companion.file(new File(args[0]),lang);
        KeyGrid grid=originalGeometry(lang); TypingWord state=new TypingWord(new SpatialDecoder(model), true);
        List<Long> letters=new ArrayList<>(), spaces=new ArrayList<>();
        int count=0, total=0, matches=0; int[] errors=new int[3], exact=new int[3];
        // Strip hits, as iOS's harness counts them: the intended word among the slots once its last
        // letter is typed, after its first two letters (words of three or more), and among the
        // predictions after the space before it.
        int[] hits=new int[6];
        try (BufferedReader reader=new BufferedReader(new FileReader(args[1]))) {
            String line;
            while((line=reader.readLine())!=null) {
                JSONObject r=new JSONObject(line);
                if(!r.getString("id").equals(group+count)) throw new IllegalArgumentException("Unexpected row "+count);
                String typed=r.getString("typed"); JSONArray taps=r.getJSONArray("taps");
                if(typed.length()!=taps.length()) throw new IllegalArgumentException("Unaligned taps");
                StringBuilder out=new StringBuilder(); state.reset("");
                String[] planned=r.getString("intended").toLowerCase(Locale.ROOT).split(" ");
                Map<Integer,Integer> ends=new HashMap<>(), early=new HashMap<>();
                String want=r.optString("want","");
                JSONArray endList=r.optJSONArray("ends");
                if(endList!=null) for(int j=0, from=0;j<endList.length();j++) {
                    int end=endList.getInt(j); ends.put(end,j);
                    if(end-from>=3) early.put(from+2,j);
                    from=end+(end<want.length()&&want.charAt(end)==' '?1:0);
                }
                List<String> shown=Collections.emptyList();
                for(int i=0;i<typed.length();i++) {
                    char key=typed.charAt(i); double x=taps.getJSONArray(i).getDouble(0), y=taps.getJSONArray(i).getDouble(1);
                    long start=System.nanoTime();
                    if(key==' ') {
                        Correction fix=state.finish(' ',new KeyPoint(x,y),grid);
                        if(fix!=null) {
                            if(!out.toString().endsWith(fix.getOriginal())) throw new IllegalStateException("Correction off the text");
                            out.replace(out.length()-fix.getOriginal().length(),out.length(),fix.getReplacement());
                        }
                        out.append(' '); String before=out.toString(); state.reset(before); shown=predictions(state,before);
                    } else {
                        state.letter(String.valueOf(key),x,y,grid); out.append(key); shown=strip(state);
                    }
                    (key==' '?spaces:letters).add(System.nanoTime()-start);
                    Integer j;
                    if((j=early.get(i+1))!=null) { hits[4]+=hit(shown,planned[j])?1:0; hits[5]++; }
                    if((j=ends.get(i+1))!=null) { hits[0]+=hit(shown,planned[j])?1:0; hits[1]++; }
                    if(i<want.length()&&want.charAt(i)==' '&&(j=ends.get(i))!=null&&j+1<planned.length) {
                        hits[2]+=hit(shown,planned[j+1])?1:0; hits[3]++;
                    }
                }
                String[] ref=words(r.getString("intended")); total+=ref.length; count++;
                String ios=r.getString("ios_shared"); hasIos|=!ios.isEmpty();
                if(outputs!=null) outputs.println(r.getString("id")+"\t"+out.toString().trim());
                if (Arrays.equals(words(ios),words(out.toString()))) matches++;
                String[] results={typed,ios,out.toString()};
                for(int i=0;i<3;i++) { int err=edits(ref,words(results[i])); errors[i]+=err; if(err==0) exact[i]++; }
            }
        }
        if(count!=2000) throw new IllegalArgumentException("Incomplete synthetic set: "+count);
        if(outputs!=null) outputs.close();
        String[] names={"raw","ios_shared","android_spatial"};
        for(int i=0;i<3;i++) if(i!=1||hasIos)
            System.out.printf(Locale.ROOT,"wer\t%s\t%d\t%d\t%.4f\t%d/%d\n",lang+"_"+names[i],errors[i],total,100.0*errors[i]/total,exact[i],count);
        if(hasIos) System.out.printf("agreement\t%s\t%d/%d\n",lang,matches,count);
        if(hits[1]>0) System.out.printf("strip\t%s\tword %d/%d\tnext %d/%d\tafter two letters %d/%d\n",lang,hits[0],hits[1],hits[2],hits[3],hits[4],hits[5]);
        latency(lang+"_letter",letters); latency(lang+"_space",spaces);
    }
}
