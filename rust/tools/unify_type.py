#!/usr/bin/env python3
"""Collapse a duplicated encoder type onto one canonical module.

usage: unify.py <TypeName> <canonical-module-basename>

Deletes the struct/enum definition plus any `impl T` / `impl Default for T`
blocks from every other encoder module that declares it, and inserts a
`pub use crate::encoder::<canonical>::T;` in their place so existing paths keep
resolving. Reports what it removed so divergent impls can be reviewed.
"""
import re, sys, glob, os

T, CANON = sys.argv[1], sys.argv[2]
SRC = 'encoder'

struct_re = re.compile(
    r'(?:^#\[[^\n]*\]\n)*^pub (?:struct|enum) %s\b[^\n]*\{.*?^\}\n' % T, re.M | re.S)
struct_unit_re = re.compile(
    r'(?:^#\[[^\n]*\]\n)*^pub (?:struct|enum) %s\b[^\n]*;\n' % T, re.M)
impl_re = re.compile(r'^impl(?:<[^>]*>)? (?:\w+(?:<[^>]*>)? for )?%s\b[^\n]*\{.*?^\}\n' % T,
                     re.M | re.S)

for f in sorted(glob.glob(os.path.join(SRC, '*.rs'))):
    base = os.path.splitext(os.path.basename(f))[0]
    if base == CANON:
        continue
    s = orig = open(f).read()
    if not re.search(r'^pub (?:struct|enum) %s\b' % T, s, re.M):
        continue

    impls = [m.group(0) for m in impl_re.finditer(s)]
    s = impl_re.sub('', s)
    s = struct_re.sub('', s, count=1)
    s = struct_unit_re.sub('', s, count=1)

    use_line = 'pub use crate::encoder::%s::%s;\n' % (CANON, T)
    if use_line not in s:
        # Place it after the last top-level `use` so imports stay grouped. A `use`
        # may span lines (`use crate::{\n  A, B,\n};`), so match through to the
        # terminating semicolon rather than to the first newline -- otherwise the
        # insertion lands *inside* a braced group and the file no longer parses.
        uses = list(re.finditer(r'^(?:pub )?use [^;]*;\n', s, re.M | re.S))
        at = uses[-1].end() if uses else 0
        s = s[:at] + use_line + s[at:]

    open(f, 'w').write(s)
    print('  %-42s removed struct + %d impl(s)' % (f, len(impls)))
    for i in impls:
        head = i.split('\n')[0]
        fns = re.findall(r'fn (\w+)', i)
        print('        %-50s %s' % (head, fns))
