#!/usr/bin/env python3
"""R-HOS (E10): hostile pairs, written as a `git fast-import` stream to stdout.

    mk-hostile.py | git -C <new repo> fast-import --quiet

Branches (each case has <case>/ours and <case>/theirs from its own base):
  big-blob     a 100 MB text file modified by both sides at the same line
  many         10,000 files in conflict
  deep         a file 200 directories deep, in conflict
  renames      5,000 files renamed with a change on one side, modified on the other
  dense        a 7.5 MB file with a change every 2nd (ours) and 3rd (theirs) line
"""
import sys

out = sys.stdout.buffer
mark = [0]


def blob(data):
    mark[0] += 1
    out.write(b"blob\nmark :%d\ndata %d\n" % (mark[0], len(data)))
    out.write(data)
    out.write(b"\n")
    return mark[0]


def commit(branch, msg, files, parent=None, deletes=()):
    out.write(b"commit refs/heads/%s\n" % branch.encode())
    out.write(b"author h <h@example.com> 1700000000 +0000\ncommitter h <h@example.com> 1700000000 +0000\n")
    m = msg.encode()
    out.write(b"data %d\n%s\n" % (len(m), m))
    if parent:
        out.write(b"from refs/heads/%s\n" % parent.encode())
    for p in deletes:
        out.write(b"D %s\n" % p.encode())
    for path, mk in files:
        out.write(b"M 100644 :%d %s\n" % (mk, path.encode()))


def text(n, tag):
    return b"".join(b"%s line %d with some text to make it longer\n" % (tag, i) for i in range(n))


def case(name, base, ours, theirs, ours_del=(), theirs_del=()):
    commit(f"{name}/base", f"{name} base", base)
    commit(f"{name}/ours", f"{name} ours", ours, parent=f"{name}/base", deletes=ours_del)
    commit(f"{name}/theirs", f"{name} theirs", theirs, parent=f"{name}/base", deletes=theirs_del)


# big-blob: ~100 MB (2.2M lines).
lines = 2_200_000
big = text(lines, b"big")
case("big-blob",
     [("big.txt", blob(big))],
     [("big.txt", blob(big.replace(b"big line 7 with", b"big line 7 OURS with", 1)))],
     [("big.txt", blob(big.replace(b"big line 7 with", b"big line 7 THEIRS with", 1)))])
del big

# many: 10,000 small files, all in conflict.
base, ours, theirs = [], [], []
for i in range(10_000):
    p = f"many/d{i % 100}/f{i}.txt"
    t = text(10, b"m%d" % i)
    base.append((p, blob(t)))
    ours.append((p, blob(t.replace(b"line 3 ", b"line 3 OURS ", 1))))
    theirs.append((p, blob(t.replace(b"line 3 ", b"line 3 THEIRS ", 1))))
case("many", base, ours, theirs)

# deep: 200 levels.
deep = "/".join(["d"] * 200) + "/f.txt"
t = text(20, b"deep")
case("deep", [(deep, blob(t))],
     [(deep, blob(t.replace(b"line 5 ", b"line 5 OURS ", 1)))],
     [(deep, blob(t.replace(b"line 5 ", b"line 5 THEIRS ", 1)))])

# renames: ours renames 5,000 files (with a small change, so detection is by similarity),
# theirs modifies the same 5,000 files at another line.
base, ours, theirs, gone = [], [], [], []
for i in range(5_000):
    p, q = f"old/d{i % 50}/f{i}.txt", f"new/d{i % 50}/g{i}.txt"
    t = text(40, b"r%d" % i)
    base.append((p, blob(t)))
    ours.append((q, blob(t.replace(b"line 1 ", b"line 1 OURS ", 1))))
    gone.append(p)
    theirs.append((p, blob(t.replace(b"line 30 ", b"line 30 THEIRS ", 1))))
case("renames", base, ours, theirs, ours_del=gone)

# dense: a 7.5 MB file (under the 8 MiB blob cap) where ours changes every 2nd line and
# theirs every 3rd: the worst single-blob text merge the cap still lets through.
n = 170_000
base_lines = [b"dense line %d with some text to make it longer\n" % i for i in range(n)]
o = [l.replace(b"dense", b"OURS", 1) if i % 2 == 0 else l for i, l in enumerate(base_lines)]
t = [l.replace(b"dense", b"THEIRS", 1) if i % 3 == 0 else l for i, l in enumerate(base_lines)]
case("dense", [("dense.txt", blob(b"".join(base_lines)))],
     [("dense.txt", blob(b"".join(o)))], [("dense.txt", blob(b"".join(t)))])
