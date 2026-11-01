#!/usr/bin/env python3
"""Synthetic benchmark trees, so the output-heavy and deep-nesting rows are reproducible.

flat100k/  100 dirs x 1,000 empty files `m<i>.txt`: `zf m` matches all 100,000.
deep/      120 nested levels, each with 40 files (every 4th `.rs`) and one `src<i>` dir.

usage: make_trees.py OUT_DIR   (skips a tree that already exists)
"""

import os
import shutil
import sys


def flat(root):
    for d in range(1, 101):
        path = os.path.join(root, f"d{d}")
        os.makedirs(path)
        for i in range(1, 1001):
            open(os.path.join(path, f"m{i}.txt"), "w").close()


def deep(root):
    path = root
    for level in range(120):
        path = os.path.join(path, f"level{level + 1}")
        os.makedirs(path)
        for j in range(40):
            name = f"file{j}.rs" if j % 4 == 0 else f"main{j}.txt"
            open(os.path.join(path, name), "w").close()
        os.makedirs(os.path.join(path, f"src{level}"))


def main():
    out = sys.argv[1]
    for name, build in (("flat100k", flat), ("deep", deep)):
        root = os.path.join(out, name)
        if not os.path.exists(root):
            # Build aside and rename, so an interrupted run never leaves a partial tree that
            # later runs would reuse.
            tmp = root + ".partial"
            shutil.rmtree(tmp, ignore_errors=True)
            os.makedirs(tmp)
            build(tmp)
            os.rename(tmp, root)
        print(root)


if __name__ == "__main__":
    main()
