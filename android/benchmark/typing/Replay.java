package local.hex.bench;

import ly.anoma.hex.typing.*;
import org.json.*;
import java.io.*;
import java.util.*;

/** Runs via app_process against the actual installed APK. Never built into Hex. */
public final class Replay {
    static KeyGrid originalGeometry(String language) {
        // Portrait KeyboardLayout at 402 pt (side 6.5, gap 6, letter height 45, row gap 11), and its
        // space bar. German and Bokmål have Android's eleven-key rows at a 36 pt pitch; French is
        // AZERTY, its bottom row's apostrophe key (never typed) left out of the letters.
        boolean wide = language.equals("de") || language.equals("nb");
        double pitch = wide ? 36 : 39.5; Map<Character,KeyPoint> points = new LinkedHashMap<>();
        String[] rows = language.equals("de") ? new String[]{"qwertzuiopü", "asdfghjklöä", "yxcvbnm"}
            : language.equals("nb") ? new String[]{"qwertyuiopå", "asdfghjkløæ", "zxcvbnm"}
            : language.equals("fr") ? new String[]{"azertyuiop", "qsdfghjklm", "wxcvbn'"}
            : new String[]{"qwertyuiop", language.equals("es") ? "asdfghjklñ" : "asdfghjkl", "zxcvbnm"};
        for (int row=0; row<rows.length; row++) {
            String letters=rows[row];
            for (int i=0; i<letters.length(); i++)
                if (letters.charAt(i) != '\'')
                    points.put(letters.charAt(i),new KeyPoint(201 + (i-(letters.length()-1)/2.0)*pitch,66.5+56*row));
        }
        return new KeyGrid(points, pitch, 56, new KeyRect(64.76, 212, 303.1, 257));
    }
    // A keyboard's own geometry, as the end-to-end check measures it on the emulator: key centres,
    // pitches and the space bar, in the key plane's pixels.
    static KeyGrid geometry(String path) throws Exception {
        JSONObject g=new JSONObject(new String(java.nio.file.Files.readAllBytes(java.nio.file.Paths.get(path)),"UTF-8"));
        Map<Character,KeyPoint> points=new LinkedHashMap<>(); JSONObject keys=g.getJSONObject("keys");
        for(Iterator<String> it=keys.keys(); it.hasNext();) {
            String k=it.next(); if(k.equals("'")) continue;
            points.put(k.charAt(0),new KeyPoint(keys.getJSONArray(k).getDouble(0),keys.getJSONArray(k).getDouble(1)));
        }
        JSONObject cj=g.optJSONObject("cells");
        if(cj!=null) {
            cells=new HashMap<>(); density=g.getDouble("density");
            for(Iterator<String> it=cj.keys(); it.hasNext();) {
                String k=it.next(); JSONArray a=cj.getJSONArray(k);
                cells.put(k.charAt(0),new double[]{a.getDouble(0),a.getDouble(1),a.getDouble(2),a.getDouble(3)});
            }
        }
        JSONArray s=g.getJSONArray("space");
        return new KeyGrid(points,g.getDouble("pitchX"),g.getDouble("pitchY"),new KeyRect(s.getDouble(0),s.getDouble(1),s.getDouble(2),s.getDouble(3)));
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
    // The replay geometry's touch areas tile each row: a key's centre ± half its pitch. A neighbouring
    // letter within reach of the touch, by its distance to that area, in points (iOS's dp).
    // A measured keyboard's touch cells (left, top, right, bottom) and pixels per dp; null and 1 for the
    // benchmark's own layout, whose cells tile the rows around the centres and whose units are points.
    static Map<Character,double[]> cells; static double density=1;
    // French AZERTY's apostrophe key on the letter rows, which the keyboard reads as it reads the letters.
    static KeyPoint apostropheKey;
    static Map<Character,Double> near(KeyGrid grid, char hit, double x, double y) {
        // KeyPlaneView.likelier: a shifted label uses the same cell, with shifted neighbours.
        boolean shifted=Character.isUpperCase(hit); hit=Character.toLowerCase(hit);
        Map<Character,Double> out = new HashMap<>();
        if (cells != null) {
            // As KeyPlaneView.likelier: the distance from the touch to each other letter's cell, in dp.
            for (Map.Entry<Character,double[]> e : cells.entrySet()) {
                if (e.getKey() == hit) continue;
                double[] c = e.getValue();
                double dx = Math.max(Math.max(c[0] - x, 0), x - c[2]), dy = Math.max(Math.max(c[1] - y, 0), y - c[3]);
                double d = Math.hypot(dx, dy) / density;
                if (d < TypingWord.REACH) out.put(shifted ? Character.toUpperCase(e.getKey()) : e.getKey(), d);
            }
            return out;
        }
        Map<Character,KeyPoint> keys = new HashMap<>(grid.getCentres());
        if (apostropheKey != null) keys.put('\'', apostropheKey);
        for (Map.Entry<Character,KeyPoint> e : keys.entrySet()) {
            if (e.getKey() == hit) continue;
            KeyPoint c = e.getValue();
            double dx = Math.max(0, Math.abs(x - c.getX()) - grid.getPitchX() / 2), dy = Math.max(0, Math.abs(y - c.getY()) - grid.getPitchY() / 2);
            double d = Math.sqrt(dx * dx + dy * dy);
            if (d < TypingWord.REACH) out.put(shifted ? Character.toUpperCase(e.getKey()) : e.getKey(), d);
        }
        return out;
    }
    // Whether a letter after `before` starts a word the keyboard reads (startsWord, else a boundary).
    static boolean startsWord(TypingWord state, String before) throws Exception {
        try {
            return (Boolean) TypingWord.class.getMethod("startsWord", String.class).invoke(state, before);
        } catch (NoSuchMethodException old) {
            return TypingWord.Companion.boundary(before.isEmpty() ? null : before.charAt(before.length() - 1));
        }
    }
    static boolean letterOdds = true;
    static char likelierKey(TypingWord state, String before, char hit, double x, double y, KeyGrid grid) throws Exception {
        if (!letterOdds) return hit;
        try {
            java.lang.reflect.Method m = TypingWord.class.getMethod("likelierKey", String.class, char.class, Map.class, Collection.class);
            return (Character) m.invoke(state, before, hit, near(grid, hit, x, y), grid.getCentres().keySet());
        } catch (NoSuchMethodException old) { return hit; }
    }
    // `memory MODEL SET LANG ...`: every language's replay in one process, each kept loaded, then the
    // process's resident memory with all of them, against before the first.
    static final List<Object> kept=new ArrayList<>();
    static String memory() throws IOException {
        StringBuilder b=new StringBuilder();
        for(String l:java.nio.file.Files.readAllLines(java.nio.file.Paths.get("/proc/self/status")))
            if(l.startsWith("VmRSS")||l.startsWith("RssAnon")||l.startsWith("RssFile")) b.append(l.replaceAll("\\s+"," ")).append("; ");
        for(String l:java.nio.file.Files.readAllLines(java.nio.file.Paths.get("/proc/self/smaps_rollup")))
            if(l.startsWith("Pss:")) b.append(l.replaceAll("\\s+"," "));
        return b.toString();
    }
    public static void main(String[] args) throws Exception {
        if(args[0].equals("memory")) {
            System.out.println("memory\tbefore\t"+memory());
            for(int i=1;i+2<args.length;i+=3) replay(new String[]{args[i],args[i+1],args[i+2]});
            System.gc();
            System.out.println("memory\tall loaded\t"+memory());
            return;
        }
        replay(args);
    }
    static void replay(String[] args) throws Exception {
        String lang=args[2], group=lang.equals("en") ? "test-" : lang+"-test-";
        boolean gate=args.length==4 && args[3].equals("--gate");
        PrintWriter outputs=!gate && args.length>3 ? new PrintWriter(new FileWriter(args[3])) : null;
        boolean hasIos=false;
        TypingModel model=TypingModel.Companion.file(new File(args[0]),lang);
        // `MODEL SET LANG OUT [GEOMETRY]`: with a measured geometry, any number of sentences.
        KeyGrid grid=args.length>4 ? geometry(args[4]) : originalGeometry(lang); TypingWord state=new TypingWord(new SpatialDecoder(model), true);
        apostropheKey=lang.equals("fr")&&cells==null ? new KeyPoint(201+3*39.5,66.5+56*2) : null;
        boolean apostropheOnLetters=lang.equals("fr");
        kept.add(state);
        List<Long> letters=new ArrayList<>(), spaces=new ArrayList<>();
        int count=0, total=0, matches=0; int[] errors=new int[3], exact=new int[3];
        // Strip hits, as iOS's harness counts them: the intended word among the slots once its last
        // letter is typed, after its first two letters (words of three or more), and among the
        // predictions after the space before it.
        int[] hits=new int[6];
        try (BufferedReader reader=new BufferedReader(new FileReader(args[1]))) {
            String line;
            while((line=reader.readLine())!=null) {
                // Gate input is TSV so the host JVM needs no Android JSON implementation.
                // No coordinates means exact centre taps, including explicit long-press letters.
                String[] fields=gate ? line.split("\\t", -1) : null;
                JSONObject r=gate ? null : new JSONObject(line);
                if(!gate && !r.getString("id").equals(group+count)) throw new IllegalArgumentException("Unexpected row "+count);
                String typed=gate ? fields[0] : r.getString("typed"); JSONArray taps=gate ? null : r.getJSONArray("taps");
                String[] points=gate && fields.length==2 ? fields[1].split(" ") : null;
                if((!gate && typed.length()!=taps.length()) || (points!=null && typed.length()!=points.length)) throw new IllegalArgumentException("Unaligned taps");
                StringBuilder out=new StringBuilder(); state.reset("");
                String[] planned=gate ? null : r.getString("intended").toLowerCase(Locale.ROOT).split(" ");
                Map<Integer,Integer> ends=new HashMap<>(), early=new HashMap<>();
                String want=gate ? "" : r.optString("want","");
                JSONArray endList=gate ? null : r.optJSONArray("ends");
                if(endList!=null) for(int j=0, from=0;j<endList.length();j++) {
                    int end=endList.getInt(j); ends.put(end,j);
                    if(end-from>=3) early.put(from+2,j);
                    from=end+(end<want.length()&&want.charAt(end)==' '?1:0);
                }
                List<String> shown=Collections.emptyList();
                // As the keyboard: a letter starts a word it reads only after a space, an opening
                // mark or (French, Italian) an elision; anything else is typed as it is until the next
                // space. An apostrophe ends the word as typed.
                boolean tracked=true;
                for(int i=0;i<typed.length();i++) {
                    char key=typed.charAt(i); double x, y;
                    boolean explicit=false;
                    if(!gate) { x=taps.getJSONArray(i).getDouble(0); y=taps.getJSONArray(i).getDouble(1); }
                    else if(points!=null) {
                        String[] xy=points[i].split(","); x=Double.parseDouble(xy[0]); y=Double.parseDouble(xy[1]);
                    } else {
                        KeyPoint p=grid.getCentres().get(Character.toLowerCase(key));
                        if(key==' ') { KeyRect space=grid.getSpace(); p=new KeyPoint((space.getLeft()+space.getRight())/2,space.getCentreY()); }
                        if(key=='\'') p=apostropheKey;
                        explicit=p==null && Character.isLetter(key); // IME long-press choice: w.explicit, no spatial evidence
                        x=p==null ? 0 : p.getX(); y=p==null ? 0 : p.getY();
                    }
                    long start=System.nanoTime();
                    // As the keyboard: a touch on a letter key, or on French's apostrophe key among them, types
                    // the likelier neighbour when it is near enough (the engine leaves a word it isn't reading alone).
                    if(!explicit && (Character.isLetter(key)||(key=='\''&&apostropheOnLetters))) key=likelierKey(state,out.toString(),key,x,y,grid);
                    if(key!=' '&&!Character.isLetter(key)) {
                        out.append(key); state.reset(out.toString()); tracked=false; shown=Collections.emptyList();
                    } else if(key==' '&&!tracked) {
                        out.append(' '); String before=out.toString(); state.reset(before); if(!gate) shown=predictions(state,before); tracked=true;
                    } else if(key!=' '&&state.getToken().isEmpty()&&!startsWord(state,out.toString())&&!(tracked&&out.length()==0)) {
                        out.append(key); tracked=false; shown=Collections.emptyList();
                    } else if(key==' ') {
                        Correction fix=state.finish(' ',new KeyPoint(x,y),grid);
                        if(fix!=null) {
                            if(!out.toString().endsWith(fix.getOriginal())) throw new IllegalStateException("Correction off the text");
                            out.replace(out.length()-fix.getOriginal().length(),out.length(),fix.getReplacement());
                        }
                        out.append(' '); String before=out.toString(); state.reset(before); if(!gate) shown=predictions(state,before);
                    } else {
                        tracked=true;
                        if(explicit) state.explicit(String.valueOf(key)); else state.letter(String.valueOf(key),x,y,grid);
                        out.append(key); if(!gate) shown=strip(state);
                    }
                    if(gate) continue;
                    (key==' '?spaces:letters).add(System.nanoTime()-start);
                    Integer j;
                    if((j=early.get(i+1))!=null) { hits[4]+=hit(shown,planned[j])?1:0; hits[5]++; }
                    if((j=ends.get(i+1))!=null) { hits[0]+=hit(shown,planned[j])?1:0; hits[1]++; }
                    if(i<want.length()&&want.charAt(i)==' '&&(j=ends.get(i))!=null&&j+1<planned.length) {
                        hits[2]+=hit(shown,planned[j+1])?1:0; hits[3]++;
                    }
                }
                if(gate) { System.out.println(out.toString().trim()); continue; }
                String[] ref=words(r.getString("intended")); total+=ref.length; count++;
                String ios=r.getString("ios_shared"); hasIos|=!ios.isEmpty();
                if(outputs!=null) outputs.println(r.getString("id")+"\t"+out.toString().trim());
                if (Arrays.equals(words(ios),words(out.toString()))) matches++;
                String[] results={typed,ios,out.toString()};
                for(int i=0;i<3;i++) { int err=edits(ref,words(results[i])); errors[i]+=err; if(err==0) exact[i]++; }
            }
        }
        if(gate) return;
        if(count!=2000&&args.length<=4) throw new IllegalArgumentException("Incomplete synthetic set: "+count);
        if(outputs!=null) outputs.close();
        String[] names={"raw","ios_shared","android_spatial"};
        for(int i=0;i<3;i++) if(i!=1||hasIos)
            System.out.printf(Locale.ROOT,"wer\t%s\t%d\t%d\t%.4f\t%d/%d\n",lang+"_"+names[i],errors[i],total,100.0*errors[i]/total,exact[i],count);
        if(hasIos) System.out.printf("agreement\t%s\t%d/%d\n",lang,matches,count);
        if(hits[1]>0) System.out.printf("strip\t%s\tword %d/%d\tnext %d/%d\tafter two letters %d/%d\n",lang,hits[0],hits[1],hits[2],hits[3],hits[4],hits[5]);
        latency(lang+"_letter",letters); latency(lang+"_space",spaces);
    }
}
