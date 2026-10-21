#!/usr/bin/env python3
"""Parity of conflicted-file sets between an engine and the reference (E6, H3; E7, H7).

    compare.py <engine.tsv> <ref.tsv> [--prefilter prefilter.tsv] [--labels pairs.tsv] [--diffs N]

Both TSVs come from `ckp-spike batch` (ours, theirs, ok, ms, n, paths, kinds, hunks, error).
A path `x~<side>` (Git's unique name for a file/directory conflict) counts as `x`.
Parity is measured over the pairs with a conflict in at least one engine (P7).
"""
import json
import sys
from collections import Counter


def load(path):
    rows = {}
    with open(path) as f:
        for line in f:
            p = line.rstrip("\n").split("\t")
            if len(p) < 9:
                continue
            paths = {x.split("~")[0] for x in p[5].split("|") if x}
            kinds = p[6].split("|") if p[6] else []
            try:
                hunks = {h[0]: h[1] for h in json.loads(p[7]) if h[1]}
            except ValueError:
                hunks = {}
            rows[(p[0], p[1])] = dict(ok=p[2] == "ok", ms=float(p[3]), paths=paths,
                                      kinds=dict(zip([x for x in p[5].split("|") if x], kinds)),
                                      hunks=hunks, err=p[8])
    return rows


def coalesce(blocks, gap):
    out = []
    for b in sorted(blocks):
        if out and b[0] - out[-1][1] - 1 <= gap and b[2] - out[-1][3] - 1 <= gap:
            out[-1] = [out[-1][0], max(out[-1][1], b[1]), out[-1][2], max(out[-1][3], b[3])]
        else:
            out.append(list(b))
    return out


def main():
    args = sys.argv[1:]
    eng, ref = load(args[0]), load(args[1])
    opt = dict(zip(args[2::2], args[3::2]))
    labels = {}
    if "--labels" in opt:
        for line in open(opt["--labels"]):
            p = line.rstrip("\n").split("\t")
            labels[(p[0], p[1])] = p[2] if len(p) > 2 else ""
    pre = {}
    if "--prefilter" in opt:
        for line in open(opt["--prefilter"]):
            p = line.rstrip("\n").split("\t")
            pre[(p[0], p[1])] = p[2]
    ndiff = int(opt.get("--diffs", "20"))

    pairs = [k for k in ref if k in eng]
    errors = [k for k in pairs if not eng[k]["ok"] or not ref[k]["ok"]]
    valid = [k for k in pairs if k not in errors]
    either = [k for k in valid if eng[k]["paths"] or ref[k]["paths"]]
    same = [k for k in either if eng[k]["paths"] == ref[k]["paths"]]
    fn_pairs = [k for k in valid if ref[k]["paths"] and not eng[k]["paths"]]
    fp_pairs = [k for k in valid if eng[k]["paths"] and not ref[k]["paths"]]
    fn_files, fp_files = Counter(), Counter()
    for k in valid:
        for p in ref[k]["paths"] - eng[k]["paths"]:
            fn_files[labels.get(k, "-")] += 1
        for p in eng[k]["paths"] - ref[k]["paths"]:
            kind = eng[k]["kinds"].get(p, "?")
            fp_files[kind] += 1
    print(f"pairs={len(pairs)} errors={len(errors)} valid={len(valid)} "
          f"with_conflict_either={len(either)} parity={len(same)}/{len(either)}"
          f" ({100.0 * len(same) / max(1, len(either)):.1f}%)")
    print(f"false_negative_pairs={len(fn_pairs)} false_positive_pairs={len(fp_pairs)}")
    if fn_files:
        print("missed files by case:", dict(fn_files))
    if fp_files:
        print("extra files by engine kind:", dict(fp_files))
    # Hunks (E7): same ranges on the files both engines report as text conflicts.
    both = [(k, p) for k in valid for p in eng[k]["hunks"] if p in ref[k]["hunks"]]
    hsame = [(k, p) for (k, p) in both if eng[k]["hunks"][p] == ref[k]["hunks"][p]]
    if both:
        print(f"hunks: files={len(both)} same_ranges={len(hsame)} "
              f"({100.0 * len(hsame) / len(both):.1f}%)")
        # Git (XDL_MERGE_ZEALOUS_ALNUM) joins conflict blocks separated by a few lines without
        # letters or digits; joining blocks closer than `gap` lines on both sides compares the
        # conflicted regions rather than how they are split.
        for gap in (3, 10):
            hc = [(k, p) for (k, p) in both
                  if coalesce(eng[k]["hunks"][p], gap) == coalesce(ref[k]["hunks"][p], gap)]
            print(f"hunks joined when the gap is <= {gap} lines: same={len(hc)} "
                  f"({100.0 * len(hc) / len(both):.1f}%)")
    if pre:
        prefn = [k for k in valid if pre.get(k) == "no" and ref[k]["paths"]]
        skipped = [k for k in valid if pre.get(k) == "no"]
        print(f"prefilter: skipped={len(skipped)}/{len(valid)} "
              f"({100.0 * len(skipped) / max(1, len(valid)):.1f}%) false_negatives={len(prefn)}")
        for k in prefn[:ndiff]:
            print("  prefilter FN:", k, sorted(ref[k]["paths"]))
    ms = sorted(eng[k]["ms"] for k in valid)
    if ms:
        q = lambda f: ms[min(len(ms) - 1, int(f * len(ms)))]
        print(f"engine ms: p50={q(0.5):.1f} p95={q(0.95):.1f} p99={q(0.99):.1f} max={ms[-1]:.1f}")
    for k in [k for k in either if k not in same][:ndiff]:
        print("  diff:", labels.get(k, ""), k, "engine=", sorted(eng[k]["paths"]),
              "ref=", sorted(ref[k]["paths"]), eng[k]["kinds"])
    for k in errors[:ndiff]:
        print("  error:", k, eng[k]["err"][:160] or ref[k]["err"][:160])


main()
