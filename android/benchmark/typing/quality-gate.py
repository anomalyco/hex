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
    anchors = [(k, p) for k, p in enumerate(paired) if p is not None and ref[k].lower() == hyp[p].lower()]
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


TYPISTS = ('T1', 'T2', 'T3')


def replay(command, model, path, lang, *options):
    """One Replay run of the gate's input at [path]: its outputs, and its stderr (Adaptive touch's report)."""
    done = subprocess.run(command + ['local.hex.bench.Replay', model, path, lang, '--gate', *map(str, options)],
                          encoding='utf-8', capture_output=True, check=True)
    return done.stdout.splitlines(), done.stderr


def word_errors(refs, outputs):
    return sum(align(ref, out.lower().split())[0] for ref, out in zip(refs, outputs))


def right_words(refs, outputs):
    """(sentence, word) of every reference word the output has, on the minimum-edit alignment."""
    right = set()
    for i, (ref, out) in enumerate(zip(refs, outputs)):
        hyp = out.lower().split()
        right.update((i, k) for k, (w, p) in enumerate(zip(ref, align(ref, hyp)[1])) if p is not None and hyp[p] == w)
    return right


def adaptive_info(stderr):
    info = dict(contexts=[])
    for line in stderr.splitlines():
        parts = line.split('\t')
        if parts[0] == 'adaptive':
            info.update(active_from=int(parts[2]), active_end=parts[4] == 'true')
        elif parts[0] == 'uses':
            # The context in use before the first sentence, then after each.
            info.update(start=int(parts[1]), uses=[int(c) for c in parts[2].split(',')] if parts[2] else [])
        elif parts[0] == 'context':
            fields = dict(zip(parts[2::2], parts[3::2]))
            info['contexts'].append(dict(regime=int(fields['regime']), accepted=int(fields['accepted']),
                                         weak=int(fields['weak']), strong=int(fields['strong']),
                                         sessions=int(fields['sessions']), in_use=fields['in_use'] == 'true',
                                         mean_offset=[float(v) for v in parts[-2:]]))
    return info


def broken(refs, off, on, sentences):
    """Reference words the output without Adaptive touch has and the one with it lacks, in [sentences]."""
    keep = [refs[i] for i in sentences]
    return len(right_words(keep, [off[i] for i in sentences]) - right_words(keep, [on[i] for i in sentences]))


def adaptive(lang, model, rows, cases, gate_input, gate_output, tmp, command):
    """Labs › Keyboard › Adaptive touch on this language's held-out set (README › Adaptive touch). A: with it on but
    before it has learned (one session, so never in use), the gate's output is the same, item for item. B: typists who
    aim off the centres the same way every time (TYPISTS, Replay.bias) type the noisy sentences in order with it
    learning, a new session every 50; the second thousand's word errors against it off. C: the profile each learned,
    frozen, never changes more correctly typed words than it off, typed on the centres or on the typist's own aim, and
    every critical case still passes. D: learned on T1, typed with T1's aim mirrored: T1's centres fall back within three
    sentences, breaking at most 2% of the words they would break with no fallback."""
    count = len(rows)
    refs = [text.lower().split() for _, _, text, _, _ in rows]
    later = slice(count // 2, count)
    noisy, clean = tmp / 'noisy.tsv', tmp / 'clean.tsv'
    noisy.write_text('\n'.join(typed + '\t' + points for _, _, _, typed, points in rows) + '\n', encoding='utf-8')
    clean.write_text('\n'.join([row[2] + ' ' for row in rows] + [typed + ' ' for _, typed, _ in cases]) + '\n',
                     encoding='utf-8')

    def run(path, lines, *options):
        out, err = replay(command, model, path, lang, *options)
        if len(out) != lines:
            raise ValueError(f'{lang}: replay returned {len(out)} of {lines} sentences')
        info = adaptive_info(err)
        return out, info

    learning = ('--adaptive', 'learn', '--sessions', 50)
    same, _ = run(gate_input, len(gate_output), '--adaptive', 'learn', '--sessions', 0)
    report = dict(identical=dict(items=len(gate_output), same=sum(a == b for a, b in zip(same, gate_output))))
    population, info = run(noisy, count, *learning)
    info.pop('uses', None)
    report['population'] = dict(off=word_errors(refs[later], gate_output[:count][later]),
                                on=word_errors(refs[later], population[later]), adaptive=info)
    clean_off, _ = run(clean, count + len(cases))
    reference = sum(map(len, refs))
    off_changed = reference - len(right_words(refs, clean_off[:count]))

    def critical(outputs):
        return sum(want.split() == got.lower().split() for (_, _, want), got in zip(cases, outputs))

    failed = []
    if report['identical']['same'] != report['identical']['items']:
        failed.append('adaptive: not identical before learning')
    report['typists'] = {}
    for typist in TYPISTS:
        profile = tmp / f'{typist}.bin'
        off, _ = run(noisy, count, '--typist', typist)
        on, info = run(noisy, count, '--typist', typist, *learning, '--profile-out', profile)
        alone, _ = run(noisy, count, '--typist', typist, *learning, '--unguarded', 1)
        info.pop('uses', None)
        right_off, right_on = right_words(refs[later], off[later]), right_words(refs[later], on[later])
        clean_on, _ = run(clean, count + len(cases), '--adaptive', 'frozen', '--profile-in', profile)
        aimed_off, _ = run(clean, count + len(cases), '--typist', typist)
        aimed_on, _ = run(clean, count + len(cases), '--typist', typist, '--adaptive', 'frozen', '--profile-in', profile)
        result = dict(
            off=word_errors(refs[later], off[later]), on=word_errors(refs[later], on[later]),
            alone=word_errors(refs[later], alone[later]), fixed=len(right_on - right_off), broken=len(right_off - right_on),
            centres_changed=dict(off=off_changed, on=reference - len(right_words(refs, clean_on[:count]))),
            aimed_changed=dict(off=reference - len(right_words(refs, aimed_off[:count])),
                               on=reference - len(right_words(refs, aimed_on[:count]))),
            critical=dict(passed=critical(clean_on[count:]), total=len(cases)), adaptive=info)
        # What the guard holds back, the same frozen centres deciding alone (report only).
        alone_profile = tmp / f'{typist}-alone.bin'
        run(noisy, count, '--typist', typist, *learning, '--unguarded', 1, '--profile-out', alone_profile)
        for name, aim in (('centres_changed', ()), ('aimed_changed', ('--typist', typist))):
            out, _ = run(clean, count + len(cases), *aim, '--adaptive', 'frozen', '--unguarded', 1, '--profile-in', alone_profile)
            result[name]['alone'] = reference - len(right_words(refs, out[:count]))
        report['typists'][typist] = result
        if result['on'] >= result['off']:
            failed.append(f'adaptive: no fewer errors for {typist}')
        if result['centres_changed']['on'] > result['centres_changed']['off'] or \
                result['aimed_changed']['on'] > result['aimed_changed']['off']:
            failed.append(f'adaptive: more correct words changed for {typist}')
        if result['critical']['passed'] < len(cases):
            failed.append(f'adaptive: critical cases with {typist}')
    off, _ = run(noisy, count, '--typist', 'T1m')
    on, info = run(noisy, count, '--typist', 'T1m', *learning, '--profile-in', tmp / 'T1.bin')
    stuck, _ = run(noisy, count, '--typist', 'T1m', '--adaptive', 'frozen', '--profile-in', tmp / 'T1.bin')
    # The sentences typed while T1's centres were still in use, up to the one after which they fell back.
    uses = info.pop('uses')
    held = next((i for i, c in enumerate(uses) if c != info['start']), count)
    right_off, right_on = right_words(refs, off), right_words(refs, on)
    report['grip_change'] = dict(off=word_errors(refs, off), on=word_errors(refs, on), fixed=len(right_on - right_off),
                                 broken=len(right_off - right_on), fell_back_after=held + 1,
                                 broken_before_fallback=broken(refs, off, on, range(min(held + 1, count))),
                                 broken_after_fallback=broken(refs, off, on, range(held + 1, count)),
                                 broken_without_fallback=broken(refs, off, stuck, range(count)), adaptive=info)
    grip = report['grip_change']
    if grip['fell_back_after'] > 3 or grip['broken_before_fallback'] > .02 * grip['broken_without_fallback']:
        failed.append('adaptive: the grip change')
    report['failed'] = failed
    return report


def score(lang, mapping, cases, limits, tmp, command):
    with gzip.open(HERE / 'held-out' / (lang + '.tsv.gz'), 'rt', encoding='utf-8') as f:
        rows = [line.rstrip('\n').split('\t') for line in f]
    if len(rows) != 2000 or len(cases) < 2:
        raise ValueError(f'{lang}: missing held-out sentences or critical cases')
    refs = [text.lower().split() for _, _, text, _, _ in rows]
    model = MODELS / (lang + '.hextm')
    holds, *vocabulary = subprocess.check_output(command + ['local.hex.bench.GateLexicon', model, lang],
                                                 encoding='utf-8').splitlines()
    actual = dict(pair.split(':') for pair in holds.split())
    # Most omissions tap the hold key once. Some have a conventional typed substitute (ß → ss).
    holds = {c: spec['hold'] if isinstance(spec, dict) else spec for c, spec in mapping.items()}
    substitutes = {c: spec['substitute'] if isinstance(spec, dict) else spec for c, spec in mapping.items()}
    if actual != holds:
        raise ValueError(f'{lang}: accent-keys.json disagrees with KeyboardLetters/LetterFeedback: {actual}')
    scored = set(vocabulary)
    with (MODELS / (lang+'.valid')).open(encoding='utf-8') as valid:
        lexicon = scored.union(line.split('\t')[0].rstrip('\n') for line in valid)
    table = str.maketrans(substitutes)
    shifted = str.maketrans({**substitutes, **{('ẞ' if c == 'ß' else c.upper()): sub.upper()
                                                for c, sub in substitutes.items()}})
    ambiguous_forms = set()
    for word in lexicon:
        base = word.translate(table)
        if base != word and base in lexicon:
            ambiguous_forms.update((word, base))

    accents = [text for _, _, text, _, _ in rows if text != text.translate(shifted)][:500]
    extra = HERE / 'held-out' / ('accent-'+lang+'.tsv')
    if extra.exists():
        for row in extra.read_text(encoding='utf-8').splitlines():
            _, _, text = row.split('\t')
            if text.translate(shifted) == text:
                raise ValueError(f'{lang}: ineligible accent supplement: {text}')
            accents.append(text)
    if len(accents) > 500:
        raise ValueError(f'{lang}: incorrect accent coverage ({len(accents)} sentences)')
    inputs = [typed + '\t' + points for _, _, _, typed, points in rows]
    inputs += [row[2] + ' ' for row in rows]
    inputs += [text.translate(shifted) + ' ' for text in accents]
    inputs += [typed + ' ' for _, typed, _ in cases]
    path = tmp / 'input.tsv'
    path.write_text('\n'.join(inputs) + '\n', encoding='utf-8')
    output = subprocess.check_output(command + ['local.hex.bench.Replay', model, path, lang, '--gate'],
                                     encoding='utf-8').splitlines()
    if len(output) != len(inputs):
        raise ValueError(f'{lang}: replay returned {len(output)} of {len(inputs)} sentences')
    # Score spelling case-insensitively, as before; input to the engine above retains Shift.
    words = [s.lower().split() for s in output]
    count = len(rows)
    total = sum(map(len, refs))
    raw = sum(align(ref, row[3].lower().split())[0] for ref, row in zip(refs, rows))
    errors = sum(align(ref, hyp)[0] for ref, hyp in zip(refs, words))
    changed = harm = opportunities = 0
    changes = Counter()
    for i, (ref, hyp) in enumerate(zip(refs, words[count:2*count])):
        paired = align(ref, hyp)[1]
        for w, pos in zip(ref, paired):
            got = hyp[pos] if pos is not None else None
            changed += w != got
            if w in ambiguous_forms:
                opportunities += 1
                if got in ambiguous_forms and w != got and w.translate(table) == got.translate(table):
                    harm += 1
        if lang == 'nb':
            changes.update(changed_spans(rows[i][2].split(), output[count+i].split(), paired))
    buckets = ('unambiguous', 'ambiguous', 'plain_known_target_unknown')
    totals, hits, miss_source = Counter(), Counter(), Counter()
    for text, hyp in zip(accents, words[2*count:]):
        ref = text.lower().split()
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
    sample = restoration['unambiguous']['total']
    accent_status = ('skip' if not mapping else f'too few ({sample})'
                     if sample < limits['min_unambiguous_words'] else 'scored')
    if accent_status == 'scored' and restore_rate < limits['min_accents_restored_percent']:
        failed.append('unambiguous accents restored')
    if limits['require_all_critical_cases']:
        failed.extend(c['name'] for c in critical if not c['passed'])
    return dict(language=lang, sentences=count, reference_words=total, raw_errors=raw, noisy_errors=errors,
                raw_wer_percent=100*raw/total, noisy_wer_percent=100*errors/total, error_reduction_percent=reduction,
                correct_changed=changed, correct_changed_percent=change_rate, accent_sentences=len(accents),
                accent_restoration=restoration, unambiguous_status=accent_status, unambiguous_misses_by_vocabulary=dict(miss_source),
                ambiguous_harm=tally(harm, opportunities),
                top_clean_changes=[dict(typed=a, became=b, count=n) for (a, b), n in
                                   sorted(changes.items(), key=lambda pair: (-pair[1], pair[0]))[:10]],
                critical_cases=critical, failed=failed, passed=not failed, _input=path, _output=output)


def main():
    maps, cases, limits = map(config, ['accent-keys.json', 'critical-cases.json', 'thresholds.json'])
    languages = sorted(p.stem for p in MODELS.glob('*.hextm') if (HERE/'held-out'/(p.stem+'.tsv.gz')).is_file())
    # A language may be configured before Android has its model (Polish: iOS's typing-gate.py).
    if not set(languages) <= set(maps) & set(cases):
        raise ValueError('Model/set coverage does not match the configured gate languages')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('language', nargs='?', default='all', choices=['all'] + languages)
    parser.add_argument('--output', type=Path, default=ANDROID/'build/typing-quality.json')
    parser.add_argument('--adaptive', action='store_true', help='also check Labs › Keyboard › Adaptive touch (adaptive())')
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
            gate_input, gate_output = r.pop('_input'), r.pop('_output')
            results.append(r)
            def rate(metric):
                return '—' if metric['percent'] is None else f"{metric['percent']:.2f}%"
            unambiguous = r['unambiguous_status']
            if unambiguous == 'scored':
                unambiguous = rate(r['accent_restoration']['unambiguous'])
            ambiguous = rate(r['accent_restoration']['ambiguous'])
            harm = rate(r['ambiguous_harm'])
            hits = sum(c['passed'] for c in r['critical_cases'])
            print(f"{lang:4} {r['noisy_wer_percent']:9.2f}% {r['error_reduction_percent']:9.2f}%"
                  f" {r['correct_changed_percent']:8.2f}% {unambiguous:>8} {ambiguous:>7} {harm:>6} {hits:5}/{len(r['critical_cases'])}"
                  f"   {'PASS' if r['passed'] else 'FAIL'}", flush=True)
            if args.adaptive:
                with gzip.open(HERE / 'held-out' / (lang + '.tsv.gz'), 'rt', encoding='utf-8') as f:
                    rows = [line.rstrip('\n').split('\t') for line in f]
                a = r['adaptive'] = adaptive(lang, MODELS / (lang + '.hextm'), rows, cases[lang], gate_input, gate_output,
                                             tmp, command)
                r['failed'] += a['failed']
                r['passed'] = not r['failed']
                typists = '  '.join(f"{t} {v['off']}→{v['on']} ({v['alone']})" for t, v in a['typists'].items())
                print(f"{lang:4} adaptive: same {a['identical']['same']}/{a['identical']['items']}  "
                      f"held-out {a['population']['off']}→{a['population']['on']}  {typists}  "
                      f"changed " + ' '.join(f"{v['centres_changed']['off']}/{v['centres_changed']['on']},"
                                              f"{v['aimed_changed']['off']}/{v['aimed_changed']['on']}"
                                              for v in a['typists'].values()) +
                      f"  grip: broken {a['grip_change']['broken_before_fallback']} before the fallback"
                      f" (sentence {a['grip_change']['fell_back_after']}), {a['grip_change']['broken_after_fallback']} after,"
                      f" {a['grip_change']['broken_without_fallback']} without it; {a['grip_change']['fixed']} fixed"
                      f"   {'PASS' if not a['failed'] else 'FAIL'}", flush=True)
    elapsed = round(time.monotonic()-start, 1)
    report = dict(engine='android-jvm', thresholds=limits, elapsed_seconds=elapsed, languages=results)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n', encoding='utf-8')
    print(f'{elapsed}s; {args.output}')
    return int(any(not r['passed'] for r in results))


if __name__ == '__main__':
    raise SystemExit(main())
