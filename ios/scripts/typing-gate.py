#!/usr/bin/env python3
"""The typing quality gate for the iOS keyboard, on the same held-out inputs and scoring as
android/benchmark/typing/quality-gate.py (#2374).

  python3 scripts/typing-gate.py run --destination 'id=<SIM_UDID>' [en es de fr it nb]
  python3 scripts/typing-gate.py prepare OUT [langs]      # inputs only
  python3 scripts/typing-gate.py score OUT [langs]        # score outputs already in OUT

`prepare` reads the Android benchmark's held-out sentences (held-out/<lang>.tsv.gz, the accent
supplements) and its accent-keys.json, critical-cases.json and thresholds.json; it writes OUT/input.json
for the opt-in TypingGateReplayTests, which types every input through FullKeyboardEngine on the
simulator and writes OUT/output-<lang>.json. `run` does all three, one xcodebuild per language (a
fresh process each, so one language's UITextChecker state never meets another's).

Inputs, as quality-gate.py builds them:
- noisy: the 2,000 sentences' hit keys and taps, moved onto Hex's layout key by key from the
  synthetic layout they were made on (402 pt wide, rows at y 66.5 / 122.5 / 178.5, space bar
  64.76…303.1 x 212…257); a tap Hex's keys don't take as a letter or space (outside them, or on
  123, shift or delete) goes to the nearest letter or space bar;
- clean: each reference typed right at the key centres, a letter without a key of its own picked
  from its key's long-press menu, an apostrophe without a key from the 123 page;
- accents: up to 500 references with a hold-only letter, typed with its substitute;
- critical: the named cases, at the key centres.
Scoring is quality-gate.py's: word edits case-insensitively; correct words changed on the clean
pass; accent words bucketed by whether the plain spelling is in the model or <lang>.valid.
Outputs also lose the . , ! ? ; : and quotes the engine may add, and ’ reads as '.
"""
import argparse
from collections import Counter
import gzip
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import unicodedata

IOS = Path(__file__).resolve().parents[1]
BENCH = IOS.parent / 'android/benchmark/typing'
MODELS = IOS / 'HEXKeyboard/TypingModel'
ANDROID_MODELS = IOS.parent / 'android/app/src/main/assets/typing'
LANGUAGES = ['en', 'es', 'de', 'fr', 'it', 'nb']
ROW_Y, SPACE = [66.5, 122.5, 178.5], [64.76, 212.0, 238.34, 45.0]
ROWS = {'en': ['qwertyuiop', 'asdfghjkl', 'zxcvbnm'], 'es': ['qwertyuiop', 'asdfghjklñ', 'zxcvbnm'],
        'de': ['qwertzuiopü', 'asdfghjklöä', 'yxcvbnm'], 'nb': ['qwertyuiopå', 'asdfghjkløæ', 'zxcvbnm'],
        'fr': ['azertyuiop', 'qsdfghjklm', "wxcvbn'"], 'it': ['qwertyuiop', 'asdfghjkl', 'zxcvbnm']}


def config(name):
    return json.loads((BENCH / name).read_text(encoding='utf-8'))


def geometry(lang):
    """The synthetic layout the held-out taps were made on (synthetic_sets.py, iOS's measured one
    for English and Spanish): key frames x, y, w, h."""
    pitch = 36.0 if lang in ('de', 'nb') else 39.5
    keys = {}
    for r, letters in enumerate(ROWS[lang]):
        for i, c in enumerate(letters):
            x = 21 + pitch * i if len(letters) == 11 else 201 + (i - (len(letters) - 1) / 2) * pitch
            keys[c] = [x - (pitch - 6) / 2, ROW_Y[r] - 22.5, pitch - 6, 45.0]
    keys['space'] = SPACE
    return keys


def centred(text, keys, holds, lang):
    """Taps at the key centres for `text`: [-1, -1] picks a letter from its key's long-press menu,
    [0, 0] types an apostrophe from the 123 page."""
    taps, want = [], []
    for ch in text:
        low = ch.lower()
        if ch == ' ':
            taps.append([SPACE[0] + SPACE[2] / 2, SPACE[1] + SPACE[3] / 2])
        elif low in keys:
            k = keys[low]
            taps.append([round(k[0] + k[2] / 2, 2), round(k[1] + k[3] / 2, 2)])
        elif ch == "'":
            taps.append([0, 0])
        elif low in holds:
            taps.append([-1, -1])
        else:
            raise ValueError(f'{lang}: no key for {ch!r} in {text!r}')
        want.append(ch)
    return taps, ''.join(want)


def model_words(path):
    b = path.read_bytes()
    words, _, blob, letters = struct.unpack_from('<4I', b, 8)
    chars = [' '] * (letters + 1)
    for i in range(letters):
        scalar, code = struct.unpack_from('<IB', b, 28 + 8 * i)
        chars[code] = chr(scalar)
    offsets = struct.unpack_from(f'<{words + 1}I', b, 28 + 8 * letters)
    base = len(b) - blob
    return {''.join(chars[c] for c in b[base + offsets[i]:base + offsets[i + 1]]) for i in range(words)}


def lists_dir(lang):
    return MODELS if (MODELS / (lang + '.valid')).exists() else ANDROID_MODELS


def prepare(out, langs):
    maps, cases = config('accent-keys.json'), config('critical-cases.json')
    items, geo = [], {}
    for lang in langs:
        keys = geo[lang] = geometry(lang)
        with gzip.open(BENCH / 'held-out' / (lang + '.tsv.gz'), 'rt', encoding='utf-8') as f:
            rows = [line.rstrip('\n').split('\t') for line in f]
        mapping = maps[lang]
        holds = {c: s['hold'] if isinstance(s, dict) else s for c, s in mapping.items()}
        subs = {c: s['substitute'] if isinstance(s, dict) else s for c, s in mapping.items()}
        shifted = str.maketrans({**subs, **{('ẞ' if c == 'ß' else c.upper()): s.upper() for c, s in subs.items()}})
        for n, (_, _, text, typed, points) in enumerate(rows):
            taps = [[float(v) for v in p.split(',')] for p in points.split(' ')]
            assert len(taps) == len(typed), (lang, n)
            items.append(dict(id=f'{lang}-noisy-{n}', set='noisy', language=lang, taps=taps, want=typed, reference=text))
        for n, (_, _, text, _, _) in enumerate(rows):
            taps, want = centred(text + ' ', keys, holds, lang)
            items.append(dict(id=f'{lang}-clean-{n}', set='clean', language=lang, taps=taps, want=want, reference=text))
        accents = [text for _, _, text, _, _ in rows if text != text.translate(shifted)][:500]
        extra = BENCH / 'held-out' / f'accent-{lang}.tsv'
        if extra.exists():
            accents += [row.split('\t')[2] for row in extra.read_text(encoding='utf-8').splitlines()]
        for n, text in enumerate(accents):
            taps, want = centred(text.translate(shifted) + ' ', keys, holds, lang)
            items.append(dict(id=f'{lang}-accent-{n}', set='accent', language=lang, taps=taps, want=want, reference=text))
        for n, (name, typed, expected) in enumerate(cases[lang]):
            taps, want = centred(typed + ' ', keys, holds, lang)
            items.append(dict(id=f'{lang}-critical-{n}', set='critical', language=lang, taps=taps, want=want,
                              reference=expected, name=name))
        print(f'{lang}: {len(rows)} sentences, {len(accents)} accent references', file=sys.stderr)
    out.mkdir(parents=True, exist_ok=True)
    (out / 'input.json').write_text(json.dumps(dict(width=402, geometry=geo, items=items), ensure_ascii=False))


def align(reference, output):
    """quality-gate.py's: minimum word edits, ties keeping the most exact matches, and the aligned
    output index (or None) for each reference word."""
    n, m = len(reference), len(output)
    scores = [[(0, 0)] * (m + 1) for _ in range(n + 1)]
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
                 ((insert[0]+1, insert[1]), 'insert')], key=lambda e: e[0])
    paired, i, j = [None] * n, n, m
    while i or j:
        step = move[i][j]
        if step == 'pair':
            paired[i-1] = j-1
        i -= step != 'insert'
        j -= step != 'delete'
    return scores[n][m][0], paired


def words(text):
    text = unicodedata.normalize('NFC', text.lower().replace('\u2019', "'"))
    return re.sub(r'[.,!?;:"\u00ab\u00bb]', ' ', text).split()


def tally(hits, total):
    return dict(count=hits, total=total, percent=100 * hits / total if total else None)


def score(out, langs):
    maps, limits = config('accent-keys.json'), config('thresholds.json')
    items = json.loads((out / 'input.json').read_text(encoding='utf-8'))['items']
    results = []
    for lang in langs:
        outputs = json.loads((out / f'output-{lang}.json').read_text(encoding='utf-8'))
        lists = lists_dir(lang)
        scored = model_words(lists / (lang + '.hextm'))
        lexicon = scored | {line.split('\t')[0] for line in (lists / (lang + '.valid')).read_text(encoding='utf-8').splitlines()}
        subs = {c: s['substitute'] if isinstance(s, dict) else s for c, s in maps[lang].items()}
        table = str.maketrans(subs)
        ambiguous_forms = set()
        for word in lexicon:
            base = word.translate(table)
            if base != word and base in lexicon:
                ambiguous_forms.update((word, base))
        mine = [i for i in items if i['language'] == lang]
        missing = [i['id'] for i in mine if i['id'] not in outputs]
        if missing:
            raise ValueError(f'{lang}: {len(missing)} inputs have no output ({missing[0]}…)')
        total = raw = errors = changed = harm = opportunities = 0
        buckets = ('unambiguous', 'ambiguous', 'plain_known_target_unknown')
        totals, hits, critical = Counter(), Counter(), []
        for item in mine:
            ref, hyp = item['reference'].lower().split(), words(outputs[item['id']])
            kind = item['set']
            if kind == 'noisy':
                total += len(ref)
                raw += align(ref, item['want'].lower().split())[0]
                errors += align(ref, hyp)[0]
            elif kind == 'clean':
                for w, pos in zip(ref, align(ref, hyp)[1]):
                    got = hyp[pos] if pos is not None else None
                    changed += w != got
                    if w in ambiguous_forms:
                        opportunities += 1
                        harm += got in ambiguous_forms and w != got and w.translate(table) == got.translate(table)
            elif kind == 'accent':
                for w, pos in zip(ref, align(ref, hyp)[1]):
                    base = w.translate(table)
                    if base == w:
                        continue
                    bucket = buckets[0] if base not in lexicon else buckets[1] if w in lexicon else buckets[2]
                    totals[bucket] += 1
                    hits[bucket] += pos is not None and w == hyp[pos]
            else:
                critical.append(dict(name=item['name'], expected=item['reference'], actual=outputs[item['id']].strip(),
                                     passed=ref == hyp))
        restoration = {b: tally(hits[b], totals[b]) for b in buckets}
        reduction, change_rate = 100 * (raw - errors) / raw, 100 * changed / total
        sample = restoration['unambiguous']['total']
        status = 'skip' if not maps[lang] else f'too few ({sample})' if sample < limits['min_unambiguous_words'] else 'scored'
        failed = []
        if reduction < limits['min_error_reduction_percent']:
            failed.append('error reduction')
        if change_rate > limits['max_correct_changed_percent']:
            failed.append('correct words changed')
        if status == 'scored' and restoration['unambiguous']['percent'] < limits['min_accents_restored_percent']:
            failed.append('unambiguous accents restored')
        failed += [c['name'] for c in critical if not c['passed']]
        results.append(dict(language=lang, reference_words=total, raw_errors=raw, noisy_errors=errors,
                            noisy_wer_percent=100 * errors / total, error_reduction_percent=reduction,
                            correct_changed=changed, correct_changed_percent=change_rate,
                            accent_restoration=restoration, unambiguous_status=status,
                            ambiguous_harm=tally(harm, opportunities), critical_cases=critical,
                            failed=failed, passed=not failed))
    print('Lang   Noisy WER  Reduction   Changed  Unambig.  Ambig.  Harm   Critical  Gate')
    rate = lambda m: '—' if m['percent'] is None else f"{m['percent']:.2f}%"
    for r in results:
        un = rate(r['accent_restoration']['unambiguous']) if r['unambiguous_status'] == 'scored' else r['unambiguous_status']
        print(f"{r['language']:4} {r['noisy_wer_percent']:9.2f}% {r['error_reduction_percent']:9.2f}%"
              f" {r['correct_changed_percent']:8.2f}% {un:>8} {rate(r['accent_restoration']['ambiguous']):>7}"
              f" {rate(r['ambiguous_harm']):>6} {sum(c['passed'] for c in r['critical_cases']):5}/{len(r['critical_cases'])}"
              f"   {'PASS' if r['passed'] else 'FAIL'}")
    (out / 'typing-quality.json').write_text(json.dumps(dict(engine='ios-simulator', thresholds=limits, languages=results),
                                                         ensure_ascii=False, indent=2) + '\n')
    return int(any(not r['passed'] for r in results))


def run(out, langs, destination):
    prepare(out, langs)
    for lang in langs:
        env = dict(os.environ, TEST_RUNNER_HEX_TYPING_GATE=str(out), TEST_RUNNER_HEX_TYPING_GATE_LANGUAGE=lang)
        log = out / f'xcodebuild-{lang}.log'
        with log.open('w') as f:
            code = subprocess.call(['xcodebuild', 'test', '-project', str(IOS / 'HEXMobile.xcodeproj'), '-scheme', 'HEXMobile',
                                    '-destination', destination, '-only-testing:HEXMobileTests/TypingGateReplayTests',
                                    '-parallel-testing-enabled', 'NO', 'SWIFT_OPTIMIZATION_LEVEL=-O'],
                                   cwd=IOS, env=env, stdout=f, stderr=subprocess.STDOUT)
        if code or not (out / f'output-{lang}.json').exists():
            raise SystemExit(f'{lang}: xcodebuild failed ({code}); see {log}')
        print(f'{lang}: replayed', file=sys.stderr)
    return score(out, langs)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('command', choices=['run', 'prepare', 'score'])
    parser.add_argument('out', nargs='?', type=Path, default=IOS / 'build/typing-gate')
    parser.add_argument('languages', nargs='*')
    parser.add_argument('--destination', help="xcodebuild destination for run, such as 'id=<SIM_UDID>'")
    args = parser.parse_intermixed_args()
    langs = args.languages or LANGUAGES
    for lang in langs:
        if lang not in LANGUAGES:
            parser.error(f'unknown language {lang}')
    out = args.out.resolve()
    if args.command == 'prepare':
        return prepare(out, langs)
    if args.command == 'score':
        return score(out, langs)
    if not args.destination:
        parser.error('run needs --destination')
    return run(out, langs, args.destination)


if __name__ == '__main__':
    raise SystemExit(main())
