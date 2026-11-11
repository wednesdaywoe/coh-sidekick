#!/usr/bin/env python3
"""Census the field-41 block (arc → chain_delay) and prove it is read in alignment.

Why this exists: the block between `arc` and `chain_delay` has been patched twice,
and the second patch was invisible to every automated check this repository has.
When the Oct-2026 build put a fourth f4 in front of the AreaFactor/PPMMod pair,
reading it as three did NOT garble the record: the word left behind is the ChainEff
count, and on a power with no chain delay that is 0.0 = 0x00000000, so the four
arrays that follow each read the NEXT field's count, get 0, and the read arrives at
`box_offset` one word early — where the `has_field_45b` read the layout probe had
picked in 41a's place absorbs the spare word and the whole tail lines up again.
range, recharge, endurance and every effect past them read correctly, so the only
things visibly wrong were the block itself (`ppm_mod` 0.0 and `chain_delay` 1.0 on
nearly the whole bin, and the 89 records that DO carry a chain delay failing to
parse) and the four arrays that swallowed each other's counts. See worklog
unfiled-101 and the comment at field 41a in parser/_powers.py.

So the check is not "did the numbers move" — it is "did they move in the SHAPE only
a misaligned block produces". This prints the census and the layout it detected:

  py -3 probe_field_41a.py --source homecoming
  py -3 probe_field_41a.py --assets-dir ~/.wine/drive_c/Games/Homecoming/assets/beta

BREAKS THE CLAIM:
  * `ppm_mod` off its 1.0 default across much of the corpus — the neighbouring word
    is the AreaFactor override, which is 0.0 on nearly everything, so a read one
    word early looks like this.
  * `chain_delay` sitting on exactly 1.0 across much of the corpus — PPMMod's
    default wearing its neighbour's label. (An individual power above 1.0 is
    legitimate: NPC records author delays up to 1.8 in the open-beta bin.)
  * the oracle anchors missing: Chain_Lightning 0.3 and Tesla_Cage 0.5.
  * the undecoded field-41a word carrying values on more than a couple of records —
    a field that stays 0.0 forever is reserved; one that starts carrying values is
    authored and needs naming. It is already nonzero on one Archery power of the
    2026-10-27 closed-beta build.
"""

import argparse
import collections
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from bin_crawler.parser._powers import _detect_format, parse_powers
from bin_crawler.parser._pigg import BinResolver
from bin_crawler.parser._reader import open_parse7
from bin_crawler.assets_dir import add_source_arguments, resolve_export_source

# Leaf name → the ChainDelay the authored `.powers` defs state. Powers whose leaf
# name is shared across archetypes are matched on the full name where one is
# named here, otherwise every record of that leaf is reported.
CHAIN_ORACLE = {
    'chain_lightning': 0.3,
    'tesla_cage': 0.5,
}

# A delay at or above this is not a chain delay: the authored range is sub-second.
CHAIN_DELAY_MAX = 0.9


def main():
    ap = argparse.ArgumentParser()
    add_source_arguments(ap)
    args = ap.parse_args()
    assets = resolve_export_source(args)
    resolver = BinResolver(assets)
    print(f'source: {resolver.source_description}')

    raw = resolver.read('powers.bin')
    powers = parse_powers(raw)
    print(f'{len(powers)} powers parsed\n')

    reader = open_parse7(raw)
    reader.read_u4()  # block size
    reader.read_u4()  # record count — _detect_format reads records from here
    has_41b, has_41a, has_45b, is_tspy, is_ver = _detect_format(reader)
    print(f'detected layout: 41b={has_41b} 41a={has_41a} 45b={has_45b} '
          f'thunderspy={is_tspy} veracity={is_ver}\n')

    override = collections.Counter(round(p.area_factor_override, 4) for p in powers
                                   if p.area_factor_override)
    ppm = collections.Counter(round(p.ppm_mod, 4) for p in powers if p.ppm_mod != 1.0)
    delay = collections.Counter(round(p.chain_delay, 4) for p in powers if p.chain_delay)
    field41a = [(p.full_name, p._field41a) for p in powers if p._field41a]

    print(f'field-41a (undecoded): nonzero on {len(field41a)} record(s) {field41a[:5]}')
    print(f'area_factor_override (41b word 1): {dict(override.most_common(6))}')
    print(f'ppm_mod              (41b word 2): off-default on {sum(ppm.values())} → {dict(ppm.most_common(6))}')
    print(f'chain_delay          (field 41)   : nonzero on {sum(delay.values())} → {dict(delay.most_common(8))}')

    print('\noracle anchors (the `.powers` defs value, sought across every record '
          'that shares the leaf name — NPC copies of a player power are their own '
          'defs and legitimately carry their own values):')
    missing = []
    for leaf, want in CHAIN_ORACLE.items():
        hits = [p for p in powers if p.full_name.rsplit('.', 1)[-1].lower() == leaf]
        exact = [p for p in hits if abs(p.chain_delay - want) < 1e-6]
        if not hits:
            missing.append(f'{leaf} is not in this shard at all')
            print(f'   absent  {leaf} (defs {want}) — reworked out of this build?')
        elif not exact:
            seen = sorted({round(p.chain_delay, 4) for p in hits})
            missing.append(f'no {leaf} record carries ChainDelay {want} (seen: {seen})')
            print(f'   MISSING {leaf}: defs say {want}, this bin has {seen}')
        else:
            others = sorted({round(p.chain_delay, 4) for p in hits if p not in exact})
            print(f'   ok      {exact[0].full_name} = {want}'
                  f'{f"  ({len(exact)} records; others: {others})" if others else ""}')

    # The signatures of a misaligned block, rather than any particular value: the
    # delay field is free to author 1.8 on an NPC chain power, but a whole corpus
    # sitting on PPMMod's default is only ever a read that landed one word early.
    off_ppm = [p.full_name for p in powers if p.ppm_mod != 1.0]
    at_ppm_default = sum(1 for p in powers if p.chain_delay == 1.0)
    late_ppm = len(off_ppm) > max(5, len(powers) // 1000)
    fat_delay = at_ppm_default > max(5, len(powers) // 100)
    print()
    if late_ppm:
        print(f'FAIL: {len(off_ppm)} powers off the PPMMod 1.0 default (e.g. '
              f'{off_ppm[:3]}) — that is what reading the block one word early '
              'looks like')
    if fat_delay:
        print(f'FAIL: {at_ppm_default} powers carry chain_delay exactly 1.0 — the PPMMod '
              'default is landing in the delay slot')
    if len(field41a) > 5:
        print(f'FAIL: field-41a carries values on {len(field41a)} records — it is authored, '
              'go name it')
    if missing:
        print('FAIL: ' + '; '.join(missing))
    if not any((late_ppm, fat_delay, len(field41a) > 5, missing)):
        print('ok: the block reads in alignment on this shard')
    print('(warnings only — this probe prints, it does not gate)')


if __name__ == '__main__':
    main()
