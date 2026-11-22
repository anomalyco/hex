#!/usr/bin/env python3
"""Replay and score the fixed Android typing gate. No device, install, or system dictionary."""
import argparse
from collections import Counter
import gzip
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
ANDROID = HERE.parents[1]
MODELS = ANDROID / 'app/src/main/assets/typing'


def config(name):
    return json.loads((HERE / name).read_text(encoding='utf-8'))


def align(reference, output):
    """Minimum word edits, and the aligned output index (or None) for each reference position.
    Break ties by preserving the most exact matches, then by preferring the diagonal.
    """
    n, m = len(reference), len(output)
    scores = [[(0, 0)] * (m + 1) for _ in range(n + 1)]  # edits, negative matches
    move = [[None] * (m + 1) for _ in range(n + 1)]
    for i in range(1, n + 1):
        scores[i][0], move[i][0] = (i, 0), 'delete'
    for j in range(1, m + 1):
        scores[0][j], move[0][j] = (j, 0), 'insert'
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            same = reference[i-1] == output[j-1]
            d, k = scores[i-1][j-1]
            delete, insert = scores[i-1][j], scores[i][j-1]
            scores[i][j], move[i][j] = min(
                [((d + (not same), k - same), 'pair'), ((delete[0]+1, delete[1]), 'delete'),
                 ((insert[0]+1, insert[1]), 'insert')], key=lambda entry: entry[0])
    paired = [None] * n
    i, j = n, m
    while i or j:
        step = move[i][j]
        if step == 'pair':
            paired[i-1] = j-1
        i -= step != 'insert'
        j -= step != 'delete'
    return scores[n][m][0], paired


def changed_spans(ref, hyp, paired):
    """Describe changes between exact-word anchors, keeping splits and joins readable."""
    i = j = 0
    anchors = [(k, p) for k, p in enumerate(paired) if p is not None and ref[k] == hyp[p]]
    for k, p in anchors + [(len(ref), len(hyp))]:
        before, after = ref[i:k], hyp[j:p]
        if before:
            if len(before) == len(after):
                yield from zip(before, after)
            else:
                yield ' '.join(before), ' '.join(after)
        i, j = k + 1, p + 1


def tally(matches, opportunities):
    return dict(count=matches, total=opportunities,
                percent=100 * matches / opportunities if opportunities else None)


def score(lang, mapping, cases, limits, tmp, command):
    with gzip.open(HERE / 'held-out' / (lang + '.tsv.gz'), 'rt', encoding='utf-8') as f:
        rows = [line.rstrip('\n').split('\t') for line in f]
    if len(rows) != 2000 or len(cases) != 2:
        raise ValueError(f'{lang}: missing held-out sentences or critical cases')
    refs = [text.split() for _, _, text, _, _ in rows]
    model = MODELS / (lang + '.hextm')
    holds, *vocabulary = subprocess.check_output(command + ['local.hex.bench.GateLexicon', model, lang],
                                                 encoding='utf-8').splitlines()
    actual = dict(pair.split(':') for pair in holds.split())
    if actual != mapping:
        raise ValueError(f'{lang}: accent-keys.json disagrees with KeyboardLetters/LetterFeedback: {actual}')
    scored = set(vocabulary)
    with (MODELS / (lang+'.valid')).open(encoding='utf-8') as valid:
        lexicon = scored.union(line.split('\t')[0].rstrip('\n') for line in valid)
    table = str.maketrans(mapping)
    ambiguous_forms = set()
    for word in lexicon:
        base = word.translate(table)
        if base != word and base in lexicon:
            ambiguous_forms.update((word, base))

    accents = [ref for ref in refs if any(w != w.translate(table) for w in ref)][:500]
    extra = HERE / 'held-out' / ('accent-'+lang+'.tsv')
    if extra.exists():
        for row in extra.read_text(encoding='utf-8').splitlines():
            _, _, text = row.split('\t')
            if text.translate(table) == text:
                raise ValueError(f'{lang}: ineligible accent supplement: {text}')
            accents.append(text.split())
    if (mapping and not accents) or len(accents) > 500:
        raise ValueError(f'{lang}: incorrect accent coverage ({len(accents)} sentences)')
    inputs = [typed + '\t' + points for _, _, _, typed, points in rows]
    inputs += [row[2] + ' ' for row in rows]
    inputs += [' '.join(ref).translate(table) + ' ' for ref in accents]
    inputs += [typed + ' ' for _, typed, _ in cases]
    path = tmp / 'input.tsv'
    path.write_text('\n'.join(inputs) + '\n', encoding='utf-8')
    output = subprocess.check_output(command + ['local.hex.bench.Replay', model, path, lang, '--gate'],
                                     encoding='utf-8').splitlines()
    if len(output) != len(inputs):
        raise ValueError(f'{lang}: replay returned {len(output)} of {len(inputs)} sentences')
    # All six fixtures contain lowercase words, spaces and internal apostrophes only.
    words = [s.lower().split() for s in output]
    count = len(rows)
    total = sum(map(len, refs))
    raw = sum(align(ref, row[3].split())[0] for ref, row in zip(refs, rows))
    errors = sum(align(ref, hyp)[0] for ref, hyp in zip(refs, words))
    changed = harm = opportunities = 0
    changes = Counter()
    for ref, hyp in zip(refs, words[count:2*count]):
        paired = align(ref, hyp)[1]
        for w, pos in zip(ref, paired):
            got = hyp[pos] if pos is not None else None
            changed += w != got
            if w in ambiguous_forms:
                opportunities += 1
                if got in ambiguous_forms and w != got and w.translate(table) == got.translate(table):
                    harm += 1
        if lang == 'nb':
            changes.update(changed_spans(ref, hyp, paired))
    buckets = ('unambiguous', 'ambiguous', 'plain_known_target_unknown')
    totals, hits, miss_source = Counter(), Counter(), Counter()
    for ref, hyp in zip(accents, words[2*count:]):
        for w, pos in zip(ref, align(ref, hyp)[1]):
            base = w.translate(table)
            if base == w:
                continue
            bucket = buckets[0] if base not in lexicon else buckets[1] if w in lexicon else buckets[2]
            totals[bucket] += 1
            if pos is not None and w == hyp[pos]:
                hits[bucket] += 1
            elif bucket == 'unambiguous':
                miss_source['in_model' if w in scored else 'valid_only' if w in lexicon else 'unknown'] += 1
    restoration = {b: tally(hits[b], totals[b]) for b in buckets}
    critical = [dict(name=name, typed=typed, expected=want, actual=got, passed=want.split()==got.lower().split())
                for (name, typed, want), got in zip(cases, output[2*count+len(accents):])]
    reduction = 100 * (raw-errors) / raw
    change_rate = 100 * changed / total
    restore_rate = restoration['unambiguous']['percent']
    failed = []
    if reduction < limits['min_error_reduction_percent']:
        failed.append('error reduction')
    if change_rate > limits['max_correct_changed_percent']:
        failed.append('correct words changed')
    if restore_rate is not None and restore_rate < limits['min_accents_restored_percent']:
        failed.append('unambiguous accents restored')
    if mapping and restore_rate is None:
        failed.append('no unambiguous accent evidence')
    if limits['require_all_critical_cases']:
        failed.extend(c['name'] for c in critical if not c['passed'])
    return dict(language=lang, sentences=count, reference_words=total, raw_errors=raw, noisy_errors=errors,
                raw_wer_percent=100*raw/total, noisy_wer_percent=100*errors/total, error_reduction_percent=reduction,
                correct_changed=changed, correct_changed_percent=change_rate, accent_sentences=len(accents),
                accent_restoration=restoration, unambiguous_misses_by_vocabulary=dict(miss_source),
                ambiguous_harm=tally(harm, opportunities),
                top_clean_changes=[dict(typed=a, became=b, count=n) for (a, b), n in
                                   sorted(changes.items(), key=lambda pair: (-pair[1], pair[0]))[:10]],
                critical_cases=critical, failed=failed, passed=not failed)


def main():
    maps, cases, limits = map(config, ['accent-keys.json', 'critical-cases.json', 'thresholds.json'])
    languages = sorted(p.stem for p in MODELS.glob('*.hextm') if (HERE/'held-out'/(p.stem+'.tsv.gz')).is_file())
    if set(languages) != set(maps) or set(languages) != set(cases):
        raise ValueError('Model/set coverage does not match the configured gate languages')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('language', nargs='?', default='all', choices=['all'] + languages)
    parser.add_argument('--output', type=Path, default=ANDROID/'build/typing-quality.json')
    args = parser.parse_args()
    start = time.monotonic()
    results = []
    with tempfile.TemporaryDirectory() as scratch:
        tmp = Path(scratch)
        cp = tmp / 'classpath'
        subprocess.run([ANDROID/'gradlew', '-q', '-I', HERE/'gate-classpath.gradle', ':app:typingGateClasspath',
                        '-x', 'prepareSpeechModel', '-PgateClasspath='+str(cp)], cwd=ANDROID, check=True)
        jdk = Path(os.environ['JAVA_HOME']) / 'bin' if 'JAVA_HOME' in os.environ else Path('')
        subprocess.run([jdk/'javac', '-encoding', 'UTF-8', '-cp', cp.read_text(), '-d', tmp, HERE/'Replay.java', HERE/'GateLexicon.java'], check=True)
        command = [jdk/'java', '-Xmx768m', '-Dfile.encoding=UTF-8', '-cp', str(tmp)+os.pathsep+cp.read_text()]
        print('Lang   Noisy WER  Reduction   Changed  Unambig.  Ambig.  Harm   Critical  Gate', flush=True)
        for lang in languages if args.language == 'all' else [args.language]:
            r = score(lang, maps[lang], cases[lang], limits, tmp, command)
            results.append(r)
            def rate(metric):
                return '—' if metric['percent'] is None else f"{metric['percent']:.2f}%"
            unambiguous = rate(r['accent_restoration']['unambiguous']) if maps[lang] else 'skip'
            ambiguous = rate(r['accent_restoration']['ambiguous'])
            harm = rate(r['ambiguous_harm'])
            hits = sum(c['passed'] for c in r['critical_cases'])
            print(f"{lang:4} {r['noisy_wer_percent']:9.2f}% {r['error_reduction_percent']:9.2f}%"
                  f" {r['correct_changed_percent']:8.2f}% {unambiguous:>8} {ambiguous:>7} {harm:>6} {hits:5}/{len(r['critical_cases'])}"
                  f"   {'PASS' if r['passed'] else 'FAIL'}", flush=True)
    elapsed = round(time.monotonic()-start, 1)
    report = dict(engine='android-jvm', thresholds=limits, elapsed_seconds=elapsed, languages=results)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n', encoding='utf-8')
    print(f'{elapsed}s; {args.output}')
    return int(any(not r['passed'] for r in results))


if __name__ == '__main__':
    raise SystemExit(main())
