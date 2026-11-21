#!/usr/bin/env python3
"""Collect SUMMARY/PHASE/COUNT/LEDGER/COLUMN rows of profile-results/*.txt into profile-table.tsv (machine readable).
Columns: kind scene n shape script cell then the row's fields; see runtime/round3/profile-report.md."""
import glob, os, sys
here = os.path.abspath(sys.argv[1])
hdr = {
 'SUMMARY': 'records layers active uninstr_median uninstr_min bracket_raw intervals corrected_total tiles recs_visited prefix_tiles blocks read_cost',
 'PHASE': 'phase_id phase corrected_per_frame raw_per_frame intervals_per_frame',
 'COUNT': 'counter per_frame',
 'LEDGER': 'store bytes',
 'COLUMN': 'index description',
}
out = open(os.path.join(here, 'profile-table.tsv'), 'w')
out.write('file\tkind\tscene\tn\tshape\tscript\tcell\tfields\n')
for h, f in hdr.items(): out.write(f'#fields\t{h}\t\t\t\t\t\t{f}\n')
for f in sorted(glob.glob(os.path.join(here, 'profile-results', '*.txt'))):
    for line in open(f):
        k = line.split('\t', 1)[0]
        if k in hdr:
            p = line.rstrip('\n').split('\t')
            out.write(os.path.basename(f) + '\t' + '\t'.join(p) + '\n')
