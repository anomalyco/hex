#!/usr/bin/env python3
"""Replay and score the fixed Android typing gate. No device, install, or system dictionary."""
import argparse
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
    """Minimum word edits, and the reference positions left intact on that alignment.
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
    kept = set()
    i, j = n, m
    while i or j:
        step = move[i][j]
        if step == 'pair' and reference[i-1] == output[j-1]:
            kept.add(i-1)
        i -= step != 'insert'
        j -= step != 'delete'
    return scores[n][m][0], kept


def score(lang, mapping, cases, limits, tmp, command):
    with gzip.open(HERE / 'held-out' / (lang + '.tsv.gz'), 'rt', encoding='utf-8') as f:
        rows = [line.rstrip('\n').split('\t') for line in f]
    if len(rows) != 2000 or not cases:
        raise ValueError(f'{lang}: missing held-out sentences or critical cases')
    refs = [text.split() for _, _, text, _, _ in rows]
    accents = [(i, {j for j, w in enumerate(words) if any(c in mapping for c in w)})
               for i, words in enumerate(refs)]
    accents = [(i, affected) for i, affected in accents if affected][:500]
    if mapping and len(accents) != 500:
        raise ValueError(f'{lang}: incomplete held-out accent set ({len(accents)}/500)')
    table = str.maketrans(mapping)
    inputs = [typed + '\t' + points for _, _, _, typed, points in rows]
    inputs += [row[2] + ' ' for row in rows]
    inputs += [rows[i][2].translate(table) + ' ' for i, _ in accents]
    inputs += [typed + ' ' for _, typed, _ in cases]
    path = tmp / 'input.tsv'
    path.write_text('\n'.join(inputs) + '\n', encoding='utf-8')
    output = subprocess.check_output(command + [MODELS / (lang + '.hextm'), path, lang, '--gate'],
                                     encoding='utf-8').splitlines()
    if len(output) != len(inputs):
        raise ValueError(f'{lang}: replay returned {len(output)} of {len(inputs)} sentences')
    # All six fixtures contain lowercase words, spaces and internal apostrophes only.
    words = [s.lower().split() for s in output]
    count = len(rows)
    total = sum(map(len, refs))
    raw = sum(align(ref, row[3].split())[0] for ref, row in zip(refs, rows))
    errors = sum(align(ref, hyp)[0] for ref, hyp in zip(refs, words))
    changed = total - sum(len(align(ref, hyp)[1]) for ref, hyp in zip(refs, words[count:]))
    eligible = sum(len(affected) for _, affected in accents)
    restored = sum(len(affected & align(refs[i], hyp)[1])
                   for (i, affected), hyp in zip(accents, words[2*count:]))
    critical = [dict(name=name, typed=typed, expected=want, actual=got, passed=want.split()==got.lower().split())
                for (name, typed, want), got in zip(cases, output[2*count+len(accents):])]
    reduction = 100 * (raw-errors) / raw
    change_rate = 100 * changed / total
    restore_rate = 100 * restored / eligible if eligible else None
    failed = []
    if reduction < limits['min_error_reduction_percent']:
        failed.append('error reduction')
    if change_rate > limits['max_correct_changed_percent']:
        failed.append('correct words changed')
    if restore_rate is not None and restore_rate < limits['min_accents_restored_percent']:
        failed.append('accents restored')
    if limits['require_all_critical_cases']:
        failed.extend(c['name'] for c in critical if not c['passed'])
    return dict(language=lang, sentences=count, reference_words=total, raw_errors=raw, noisy_errors=errors,
                raw_wer_percent=100*raw/total, noisy_wer_percent=100*errors/total, error_reduction_percent=reduction,
                correct_changed=changed, correct_changed_percent=change_rate, accent_sentences=len(accents),
                accent_words=eligible, accents_restored=restored, accents_restored_percent=restore_rate,
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
        subprocess.run([jdk/'javac', '-encoding', 'UTF-8', '-cp', cp.read_text(), '-d', tmp, HERE/'Replay.java'], check=True)
        command = [jdk/'java', '-Xmx768m', '-Dfile.encoding=UTF-8', '-cp', str(tmp)+os.pathsep+cp.read_text(),
                   'local.hex.bench.Replay']
        print('Lang   Noisy WER  Reduction   Changed  Restored  Critical  Gate', flush=True)
        for lang in languages if args.language == 'all' else [args.language]:
            r = score(lang, maps[lang], cases[lang], limits, tmp, command)
            results.append(r)
            accent = 'skip' if r['accents_restored_percent'] is None else f"{r['accents_restored_percent']:.2f}%"
            hits = sum(c['passed'] for c in r['critical_cases'])
            print(f"{lang:4} {r['noisy_wer_percent']:9.2f}% {r['error_reduction_percent']:9.2f}%"
                  f" {r['correct_changed_percent']:8.2f}% {accent:>9} {hits:5}/{len(r['critical_cases'])}"
                  f"   {'PASS' if r['passed'] else 'FAIL'}", flush=True)
    elapsed = round(time.monotonic()-start, 1)
    report = dict(engine='android-jvm', thresholds=limits, elapsed_seconds=elapsed, languages=results)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n', encoding='utf-8')
    print(f'{elapsed}s; {args.output}')
    return int(any(not r['passed'] for r in results))


if __name__ == '__main__':
    raise SystemExit(main())
