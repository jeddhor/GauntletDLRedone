#!/usr/bin/env python3
"""The MPEG-2 decoder's VLC and scan tables against PCSX2's.

  mpeg2_tables_check.py [--pcsx2 pcsx2] [--out FILE]

crates/gdl_formats/src/mpeg2.rs holds the standard's Tables B.1 to B.15 as
(code, value) lists. PCSX2's IPU decoder (pcsx2/IPU/mpeg2_vlc.h, derived from
libmpeg2) holds them as lookup tables indexed by the next bits. For every
16-bit pattern the script decodes both ways: PCSX2's tables through the
indexing of IPU_MultiISA.cpp, the decoder's by prefix. They have to agree
wherever the decoder accepts a code. Codes only PCSX2 accepts are listed:
its tables give values to codes the standard leaves unused (B.15 shares
B.14's long codes; motion codes 0000 0000 xx and 0000 0010 xx). The decoder
accepts one code PCSX2 refuses in MPEG-2: macroblock_stuffing, which FFmpeg
skips too. The zigzag and alternate scans and the non-linear quantiser
scale are compared as arrays. An upstream comparison; research only.
"""
import argparse
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
NAMES = {'INTRA': 1, 'PATTERN': 2, 'BWD': 4, 'FWD': 8, 'QUANT': 16, 'MBA_ESCAPE': 34, 'MBA_STUFFING': 35, 'EOB': 64, 'ESCAPE': 65}


def rust_tables(src):
    tables = {}
    for m in re.finditer(r'const (\w+): &\[\(&str, [^\]]+\)\] = &\[(.*?)\];', src, re.S):
        tables[m.group(1)] = {e.group(1).replace(' ', ''): tuple(eval(v, {}, NAMES) for v in e.group(2).split(','))
                              for e in re.finditer(r'\("([01 ]+)",\s*([^)]*)\)', m.group(2))}
    arrays = {m.group(1): [int(v) for v in m.group(2).split(',') if v.strip()]
              for m in re.finditer(r'const (\w+): \[u8; \d+\] = \[(.*?)\];', src, re.S)}
    return tables, arrays


def c_tables(src):
    names = {'INTRA': 1, 'QUANT': 16, 'MC': 8, 'CODED': 2, 'FWD': 8, 'BWD': 4, 'INTER': 12}
    src = re.sub(r'/\*.*?\*/|//[^\n]*', '', src, flags=re.S)

    def tuples(body):
        return [tuple(eval(v.replace('(s8)', ''), {}, names) for v in t.split(',')) for t in re.findall(r'\{([^{}]+)\}', body)]
    out = {}
    for m in re.finditer(r'static constexpr (?:VLC_ALIGNED16 )?(\w+) (\w+)\[\] = \{(.*?)\};', src, re.S):
        out[m.group(2)] = tuples(m.group(3))
    for struct, var in (('MBAtabSet', 'MBA'), ('DCtabSet', 'DCtable'), ('DCTtabSet', 'DCT')):
        fields = re.search(r'struct %s\s*\{(.*?)\};' % struct, src, re.S).group(1)
        sizes = [(f, eval(n)) for _, f, n in re.findall(r'(\w+)\s+(\w+)\[([^\]]+)\];', fields)]
        body = re.search(r'static constexpr VLC_ALIGNED16 %s %s\s*=\s*\{(.*)\};' % (struct, var), src, re.S).group(1)
        body = body[:body.index('};')] if '};' in body else body
        flat = tuples(body)
        at = 0
        for f, n in sizes:
            out[f'{var}.{f}'] = flat[at:at + n]
            at += n
        assert at == len(flat), (var, at, len(flat))
    return out


def c_array(src, name):
    m = re.search(r'%s\[\d*\]\s*=\s*\{(.*?)\};' % name, src, re.S)
    return [int(v) for v in re.sub(r'/\*.*?\*/', '', m.group(1), flags=re.S).split(',') if v.strip()]


def canonical(table):
    """Decode a 16-bit pattern by prefix: (values, length) or None."""
    def dec(code):
        bits = format(code, '016b')
        for n in range(1, 17):
            if bits[:n] in table:
                return table[bits[:n]], n
        return None
    return dec


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--pcsx2', default=str(ROOT / 'pcsx2'))
    ap.add_argument('--out')
    a = ap.parse_args()
    ipu = pathlib.Path(a.pcsx2) / 'pcsx2/IPU'
    mine, arrays = rust_tables((ROOT / 'crates/gdl_formats/src/mpeg2.rs').read_text())
    c = c_tables((ipu / 'mpeg2_vlc.h').read_text())
    multi = (ipu / 'IPU_MultiISA.cpp').read_text()
    ipu_cpp = (ipu / 'IPU.cpp').read_text()

    def ent(t, i):
        e = c[t][i]
        return e if e[-1] > 0 else None

    # PCSX2's lookups (IPU_MultiISA.cpp) on a 16-bit pattern: (values, length).
    def mba(code):
        if code >= 4096:
            e = ent('MBA.mba5', (code >> 11) - 2)
        elif code >= 768:
            e = ent('MBA.mba11', (code >> 5) - 24)
        else:
            return ((34,), 11) if code >> 5 == 8 else None  # stuffing: MPEG-1 only
        return ((e[0] + 1,), e[1]) if e else None

    def mb(kind):
        def f(code):
            if kind == 'I':
                u = code >> 14
                e = None if u == 0 else ent('MB_I', u >> 1)
            else:
                u = code >> 10
                e = None if u == 0 else ent('MB_P', u >> 1) if kind == 'P' else ent('MB_B', u)
            return ((e[0],), e[1]) if e else None
        return f

    def cbp(code):
        e = ent('CBP_7', (code >> 9) - 16) if code >= 0x2000 else ent('CBP_9', code >> 7)
        return ((e[0],), e[1]) if e else None

    def motion(code):
        # The magnitude; the sign bit follows (PCSX2 counts it apart).
        if code & 0x8000:
            return ((0,), 1)
        e = ent('MV_4', code >> 12) if (code & 0xf000) or (code & 0xfc00) == 0x0c00 else ent('MV_10', code >> 6)
        return ((e[0] + 1,), e[1]) if e else None

    def dc(kind):
        def f(code):
            if code >> 11 < 31:
                e = ent(f'DCtable.{kind}0', code >> 11)
            else:
                e = ent(f'DCtable.{kind}1', (code >> 7) - 0x1f0 if kind == 'lum' else (code >> 6) - 0x3e0)
            return ((e[0],), e[1]) if e else None
        return f

    def dct(variant):
        def f(code):
            ivf = variant == 'b15'
            if code >= 16384 and not ivf:
                e = ent('DCT.first' if variant == 'first' else 'DCT.next', (code >> 12) - 4)
            elif code >= 1024:
                e = ent('DCT.tab0a' if ivf else 'DCT.tab0', (code >> 8) - 4)
            elif code >= 512:
                e = ent('DCT.tab1a' if ivf else 'DCT.tab1', (code >> 6) - 8)
            elif code >= 16:
                for lim, sh, t in ((256, 4, 'tab2'), (128, 3, 'tab3'), (64, 2, 'tab4'), (32, 1, 'tab5'), (16, 0, 'tab6')):
                    if code >= lim:
                        e = ent(f'DCT.{t}', (code >> sh) - 16)
                        break
            else:
                return None
            return ((e[0], e[1]), e[2]) if e else None
        return f

    b14 = dict(mine['DCT_B14'], **mine['DCT_COMMON'])
    b14_first = {k: v for k, v in b14.items() if k not in ('10', '11')}
    b14_first['1'] = (0, 1)
    b15 = dict(mine['DCT_B15'], **mine['DCT_COMMON'])
    checks = [('B.1 macroblock_address_increment', mine['MBA'], mba), ('B.2 macroblock_type I', mine['MB_I'], mb('I')),
              ('B.3 macroblock_type P', mine['MB_P'], mb('P')), ('B.4 macroblock_type B', mine['MB_B'], mb('B')),
              ('B.9 coded_block_pattern', mine['CBP'], cbp), ('B.10 motion_code', mine['MOTION_CODE'], motion),
              ('B.12 dct_dc_size_luminance', mine['DC_LUMA'], dc('lum')), ('B.13 dct_dc_size_chrominance', mine['DC_CHROMA'], dc('chrom')),
              ('B.14 first coefficient', b14_first, dct('first')), ('B.14', b14, dct('next')), ('B.15', b15, dct('b15'))]
    report, fail = [], []
    for name, table, theirs in checks:
        dec = canonical(table)
        disagree, only_theirs, only_mine = [], set(), set()
        for code in range(1 << 16):
            x, y = dec(code), theirs(code)
            if x and y and x != y:
                disagree.append((format(code, '016b'), x, y))
            elif y and not x:
                only_theirs.add((format(code, '016b')[:y[1]], y[0]))
            elif x and not y:
                only_mine.add((format(code, '016b')[:x[1]], x[0]))
        row = {'table': name, 'codes': len(table), 'disagreements': disagree[:10], 'pcsx2_only': sorted(only_theirs), 'decoder_only': sorted(only_mine)}
        report.append(row)
        if disagree:
            fail.append(f'{name}: {len(disagree)} patterns disagree, e.g. {disagree[0]}')
        if only_mine and not (name.startswith('B.1 ') and only_mine == {('00000001111', (35,))}):
            fail.append(f'{name}: codes only the decoder accepts: {sorted(only_mine)[:5]}')
        print(f'{name}: {len(table)} codes, {len(disagree)} disagreements, {len(only_theirs)} PCSX2-only, {len(only_mine)} decoder-only')
    for mine_name, theirs in (('ZIGZAG', c_array(multi, 'mpeg2_scan_norm')), ('ALTERNATE', c_array(multi, 'mpeg2_scan_alt')),
                              ('NON_LINEAR_Q', c_array(ipu_cpp, 'non_linear_quantizer_scale'))):
        same = arrays[mine_name] == theirs
        print(f'{mine_name}: {"equal" if same else "DIFFERENT"}')
        report.append({'array': mine_name, 'equal': same})
        if not same:
            fail.append(f'{mine_name} differs')
    result = {'pass': not fail, 'failures': fail, 'tables': report}
    if a.out:
        pathlib.Path(a.out).write_text(json.dumps(result, indent=1) + '\n')
    print('PASS' if not fail else f'FAIL {fail}')
    return 0 if not fail else 1


if __name__ == '__main__':
    sys.exit(main())
