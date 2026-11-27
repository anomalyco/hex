package local.hex.bench;

import ly.anoma.hex.keyboard.KeyboardLetters;
import ly.anoma.hex.typing.*;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPInputStream;

/** Replays swipe_paths.py's paths through the keyboard's swipe reader (SwipeReader: SwipeDecoder with English's
 * contractions, never-suggest, then the rare-name ranking) on the host JVM or, with app_process, against the
 * installed APK. Never built into Hex.
 *   SwipeReplay MODEL.hextm LANG PATHS.tsv.gz [WORDS.tsv] [--outside LIST]
 * Prints top-1/3/4 of the decoder's readings and of the keyboard's ranking, by frequency and length as the iPhone
 * replay does, and the time per swipe. WORDS.tsv gets each word's first four readings, the keyboard's then the decoder's.
 * LIST (one lowercase word per line) holds words an earlier model lacked: they are swiped whether or not this model
 * has them and scored on their own, apart from the rest, so a model that adds them is measured on both. */
public final class SwipeReplay {
    public static void main(String[] args) throws Exception {
        Set<String> listed = new HashSet<>();
        List<String> positional = new ArrayList<>();
        for (int i = 0; i < args.length; i++) {
            if (args[i].equals("--outside")) {
                for (String w : java.nio.file.Files.readAllLines(new File(args[++i]).toPath(), java.nio.charset.StandardCharsets.UTF_8))
                    if (!w.isEmpty() && !w.startsWith("#")) listed.add(w);
            } else positional.add(args[i]);
        }
        args = positional.toArray(new String[0]);
        String lang = args[1];
        TypingModel model = TypingModel.Companion.file(new File(args[0]), lang);
        SwipeReader reader = new SwipeReader(new SpatialDecoder(model));
        reader.prepare(false);
        KeyGrid grid = canonical(lang);
        // Frequency ranks by unigram cost, as the iPhone bench buckets them.
        Integer[] order = new Integer[model.getWords()];
        for (int i = 0; i < order.length; i++) order[i] = i;
        Arrays.sort(order, Comparator.comparingInt((Integer i) -> model.frequencyCost(i)).thenComparingInt(i -> i));
        int[] rank = new int[order.length];
        for (int r = 0; r < order.length; r++) rank[order[r]] = r;
        Set<String> extra = new HashSet<>();
        if (lang.equals("en")) for (String c : SwipeReader.Companion.getContractions()) extra.add(c.toLowerCase(Locale.ROOT));

        PrintWriter words = args.length > 3 ? new PrintWriter(new OutputStreamWriter(new FileOutputStream(args[3]), "UTF-8")) : null;
        List<long[]> rows = new ArrayList<>(); // swiped words: rank, length, decoder hit (1/3/4), keyboard hit (1/3/4), listed
        List<Long> nanos = new ArrayList<>();
        int total = 0, outside = 0, listedSwiped = 0;
        double[] xs = new double[4096], ys = new double[4096], ts = new double[4096];
        try (BufferedReader in = new BufferedReader(new InputStreamReader(new GZIPInputStream(new FileInputStream(args[2])), "UTF-8"))) {
            String line;
            // Warm up the JIT on the first 200 swipes, then start over and time everything.
            List<String> lines = new ArrayList<>();
            while ((line = in.readLine()) != null) if (!line.startsWith("#")) lines.add(line);
            for (int pass = 0; pass < 2; pass++) {
                for (int li = 0; li < (pass == 0 ? Math.min(200, lines.size()) : lines.size()); li++) {
                    String[] f = lines.get(li).split("\t", -1);
                    String word = f[2], previous = f[3].isEmpty() ? null : f[3];
                    if (pass == 1) total++;
                    int index = model.index(word);
                    boolean isListed = listed.contains(word);
                    if (index < 0 && !extra.contains(word) && !isListed) { if (pass == 1) outside++; continue; }
                    String[] pts = f[4].split(" ");
                    int n = Math.min(pts.length, xs.length);
                    for (int i = 0; i < n; i++) {
                        String[] p = pts[i].split(",");
                        xs[i] = Double.parseDouble(p[0]); ys[i] = Double.parseDouble(p[1]); ts[i] = Double.parseDouble(p[2]);
                    }
                    long start = System.nanoTime();
                    SwipeDecoder.Reading reading = reader.decode(xs, ys, ts, n, grid, previous, 8);
                    List<String> ranked = reader.ranked(reading.getCandidates());
                    long spent = System.nanoTime() - start;
                    if (pass == 0) continue;
                    nanos.add(spent);
                    List<String> decoded = new ArrayList<>();
                    for (SwipeDecoder.Candidate c : reading.getCandidates()) decoded.add(c.getText().toLowerCase(Locale.ROOT));
                    List<String> keyboard = new ArrayList<>();
                    for (String r : ranked) keyboard.add(r.toLowerCase(Locale.ROOT));
                    int length = word.replace("'", "").length();
                    rows.add(new long[]{index >= 0 ? rank[index] : 999, length, hit(decoded, word), hit(keyboard, word), isListed ? 1 : 0});
                    if (isListed) listedSwiped++;
                    if (words != null) words.println(f[0] + "\t" + f[1] + "\t" + word + "\t" + String.join(" ", keyboard.subList(0, Math.min(4, keyboard.size())))
                        + "\t" + String.join(" ", decoded.subList(0, Math.min(4, decoded.size()))));
                }
            }
        }
        if (words != null) words.close();
        System.out.printf(Locale.ROOT, "lang %s paths %s: words %d, swiped %d, not in the model %d%n", lang, new File(args[2]).getName(), total, rows.size() - listedSwiped, outside);
        for (int which = 2; which <= 3; which++) {
            String name = which == 2 ? "decoder" : "keyboard";
            summary(name + " ALL   ", rows, which, r -> r[4] == 0);
            summary(name + " top1k ", rows, which, r -> r[4] == 0 && r[0] < 1000);
            summary(name + " top5k ", rows, which, r -> r[4] == 0 && r[0] < 5000);
            summary(name + " top20k", rows, which, r -> r[4] == 0 && r[0] < 20000);
            summary(name + " rare  ", rows, which, r -> r[4] == 0 && r[0] >= 20000);
        }
        long[][] lengths = {{2, 2}, {3, 3}, {4, 4}, {5, 5}, {6, 6}, {7, 8}, {9, 99}};
        for (long[] l : lengths) summary("keyboard len " + (l[0] == l[1] ? l[0] + "  " : l[1] == 99 ? l[0] + "+ " : l[0] + "-" + l[1]), rows, 3, r -> r[4] == 0 && r[1] >= l[0] && r[1] <= l[1]);
        if (!listed.isEmpty()) {
            System.out.printf(Locale.ROOT, "listed words (an earlier model lacked them): swiped %d%n", listedSwiped);
            summary("decoder  listed", rows, 2, r -> r[4] == 1);
            summary("keyboard listed", rows, 3, r -> r[4] == 1);
        }
        Collections.sort(nanos);
        System.out.printf(Locale.ROOT, "time per swipe ms: median %.2f p90 %.2f p99 %.2f max %.2f%n", ms(nanos, .5), ms(nanos, .9), ms(nanos, .99), nanos.get(nanos.size() - 1) / 1e6);
    }

    /** 1 when the word is first, 3 when it is 2nd or 3rd, 4 when 4th, 0 otherwise. */
    static long hit(List<String> readings, String word) {
        int i = readings.indexOf(word);
        return i == 0 ? 1 : i == 1 || i == 2 ? 3 : i == 3 ? 4 : 0;
    }
    interface Filter { boolean test(long[] r); }
    static void summary(String name, List<long[]> rows, int column, Filter filter) {
        int n = 0, t1 = 0, t3 = 0, t4 = 0;
        for (long[] r : rows) {
            if (!filter.test(r)) continue;
            n++;
            long h = r[column];
            if (h == 1) t1++;
            if (h == 1 || h == 3) t3++;
            if (h != 0) t4++;
        }
        System.out.printf(Locale.ROOT, "%s n=%6d top1 %5.1f%% top3 %5.1f%% top4 %5.1f%%%n", name, n, pct(t1, n), pct(t3, n), pct(t4, n));
    }
    static double pct(int a, int b) { return b == 0 ? 0 : 100.0 * a / b; }
    static double ms(List<Long> sorted, double q) { return sorted.get(Math.min(sorted.size() - 1, (int) (sorted.size() * q))) / 1e6; }

    /** The keyboard's letters on the canonical grid in key pitches (swipe_paths.py's): KeyPlaneView centres rows 1
     * and 2 in the wider row's columns and row 3 between shift and delete; the apostrophe is no letter. */
    static KeyGrid canonical(String lang) {
        List<String> rows = KeyboardLetters.INSTANCE.rows(lang);
        int columns = Math.max(rows.get(0).length(), rows.get(1).length());
        Map<Character, KeyPoint> centres = new LinkedHashMap<>();
        for (int r = 0; r < 3; r++) {
            String row = rows.get(r);
            double offset = (columns - row.length()) / 2.0;
            for (int i = 0; i < row.length(); i++)
                if (Character.isLetter(row.charAt(i))) centres.put(row.charAt(i), new KeyPoint(offset + i + 0.5, r + 0.5));
        }
        return new KeyGrid(centres, 1.0, 1.0, null);
    }
}
