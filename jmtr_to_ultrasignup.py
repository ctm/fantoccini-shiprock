#!/usr/bin/env python3
"""
jmtr_to_ultrasignup.py
======================
Reads the JSON output from the fantoccini-shiprock jmtr scraper and the
entrant HTML files exported from UltraSignup, then produces three
UltraSignup-formatted CSV files.

Usage:
    geckodriver &
    cargo run -- -e jmtr -r 5k   > results_15m.json
    cargo run -- -e jmtr -r half > results_50k.json
    cargo run -- -e jmtr -r full > results_50m.json

    python3 jmtr_to_ultrasignup.py \\
        --r15m  results_15m.json \\
        --r50k  results_50k.json \\
        --r50m  results_50m.json \\
        --e15m  "2026 JMTR 15M Entrants.html" \\
        --e50k  "2026 JMTR 50k Entrants.html" \\
        --e50m  "2026 JMTR 50M Entrants.html"

    # Outputs:
    #   JMTR_2026_15Mile_UltraSignup.csv
    #   JMTR_2026_50k_UltraSignup.csv
    #   JMTR_2026_50Mile_UltraSignup.csv

UltraSignup status codes:
    1 = Finished
    2 = DNF
    4 = Unofficial Finish
"""

import argparse
import csv
import json
import re
import sys
from pathlib import Path
from typing import Optional


# ---------------------------------------------------------------------------
# HTML entrant parsing
# ---------------------------------------------------------------------------

def load_entrants(html_path: str) -> list[dict]:
    from bs4 import BeautifulSoup

    with open(html_path, 'r', encoding='utf-8') as f:
        soup = BeautifulSoup(f.read(), 'html.parser')

    tables = soup.find_all('table')
    if len(tables) < 3:
        raise ValueError(f"Expected at least 3 tables in {html_path}, got {len(tables)}")

    t = tables[2]
    rows = t.find_all('tr')
    headers = [td.get_text(strip=True) for td in rows[0].find_all(['th', 'td'])]

    entrants = []
    for row in rows[1:]:
        cols = [td.get_text(strip=True) for td in row.find_all(['th', 'td'])]
        if len(cols) < 7:
            continue
        d = {h: (cols[i] if i < len(cols) else '') for i, h in enumerate(headers)}
        age_val = d.get('Age', '')
        gender = 'M' if age_val.startswith('M') else ('F' if age_val.startswith('F') else '')
        age_group = age_val[1:] if age_val and age_val[0] in 'MF' else age_val
        entrants.append({
            'first':     d.get('First', '').strip(),
            'last':      d.get('Last', '').strip(),
            'city':      d.get('City', '').strip(),
            'state':     d.get('Loc', '').strip(),
            'bib':       d.get('Bib', '').strip(),
            'gender':    gender,
            'age_group': age_group,
        })
    return entrants


# ---------------------------------------------------------------------------
# Result JSON loading
# ---------------------------------------------------------------------------

def load_results(json_path: str) -> list[dict]:
    if not Path(json_path).exists():
        print(f"[warn] {json_path} not found — treating all as DNF", file=sys.stderr)
        return []

    with open(json_path, 'r', encoding='utf-8') as f:
        content = f.read().strip()

    if not content:
        return []

    results = []
    decoder = json.JSONDecoder()
    pos = 0
    while pos < len(content):
        while pos < len(content) and content[pos].isspace():
            pos += 1
        if pos >= len(content):
            break
        try:
            obj, end = decoder.raw_decode(content, pos)
            if isinstance(obj, list):
                results.extend(obj)
            elif isinstance(obj, dict):
                results.append(obj)
            pos = end
        except json.JSONDecodeError:
            break

    return results


# ---------------------------------------------------------------------------
# Lookup helpers
# ---------------------------------------------------------------------------

def normalise(s: str) -> str:
    return re.sub(r'\s+', ' ', s.strip().lower())


def build_result_indexes(results: list[dict]) -> tuple[dict, dict]:
    by_bib: dict[str, dict] = {}
    by_name: dict[str, dict] = {}
    for r in results:
        bib = str(r.get('bib', '')).strip()
        if bib:
            by_bib[bib] = r
        name = normalise(str(r.get('name', '')))
        if name:
            by_name[name] = r
    return by_bib, by_name


def find_result(entrant: dict, by_bib: dict, by_name: dict):
    r = by_bib.get(entrant['bib'])
    if r:
        return r
    return by_name.get(normalise(f"{entrant['first']} {entrant['last']}"))


# ---------------------------------------------------------------------------
# Age group midpoint
# ---------------------------------------------------------------------------

def age_midpoint(ag: str) -> str:
    if not ag:
        return ''
    if ag == '<20':
        return '18'
    if ag == '70+':
        return '72'
    ag = ag.lstrip('MFX')
    parts = ag.split('-')
    if len(parts) == 2:
        try:
            return str((int(parts[0]) + int(parts[1])) // 2)
        except ValueError:
            pass
    return ''


# ---------------------------------------------------------------------------
# CSV building
# ---------------------------------------------------------------------------

FIELDNAMES = ['place', 'first', 'last', 'age', 'gender', 'city', 'state',
              'bib', 'time', 'status']

STATUS_ORDER = {'1': 0, '4': 1, '2': 2}


def sort_key(row: dict) -> tuple:
    s = STATUS_ORDER.get(row['status'], 9)
    # Sort finishers and unofficials by time_ms (exact integer milliseconds).
    # This is correct even when hours differ in digit count (e.g. 9:06 vs 10:14).
    # DNFs have time_ms=0; they sort together after finishers.
    return (s, row.get('time_ms', 0))


def build_csv_rows(
    entrants: list,
    by_bib: dict,
    by_name: dict,
    extra_finishers: Optional[list] = None,
) -> list:
    rows = []

    for e in entrants:
        r = find_result(e, by_bib, by_name)
        has_time = r and r.get('time')
        rows.append({
            'place':   '',
            'first':   e['first'],
            'last':    e['last'],
            'age':     age_midpoint(e['age_group']),
            'gender':  e['gender'],
            'city':    e['city'],
            'state':   e['state'],
            'bib':     e['bib'],
            'time':    r['time'] if has_time else '',
            'time_ms': r.get('time_ms', 0) if r else 0,
            'status':  '1' if has_time else '2',
        })

    for item in (extra_finishers or []):
        e = item['entrant']
        r = item['result']
        rows.append({
            'place':   '',
            'first':   e['first'],
            'last':    e['last'],
            'age':     age_midpoint(e['age_group']),
            'gender':  e['gender'],
            'city':    e['city'],
            'state':   e['state'],
            'bib':     e['bib'],
            'time':    r.get('time', ''),
            'time_ms': r.get('time_ms', 0),
            'status':  '4',
        })

    rows.sort(key=sort_key)

    place = 0
    for row in rows:
        if row['status'] == '1':
            place += 1
            row['place'] = str(place)

    return rows


def write_csv(rows: list[dict], path: str) -> None:
    with open(path, 'w', newline='', encoding='utf-8') as f:
        writer = csv.DictWriter(f, fieldnames=FIELDNAMES, extrasaction='ignore')
        writer.writeheader()
        writer.writerows(rows)
    finished   = sum(1 for r in rows if r['status'] == '1')
    unofficial = sum(1 for r in rows if r['status'] == '4')
    dnf        = sum(1 for r in rows if r['status'] == '2')
    parts = [f"{finished} finishers"]
    if unofficial:
        parts.append(f"{unofficial} unofficial")
    parts.append(f"{dnf} DNF")
    print(f"Wrote {path}  ({', '.join(parts)})")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--r15m', default='results_15m.json')
    ap.add_argument('--r50k', default='results_50k.json')
    ap.add_argument('--r50m', default='results_50m.json')
    ap.add_argument('--e15m', default='2026 JMTR 15M Entrants.html')
    ap.add_argument('--e50k', default='2026 JMTR 50k Entrants.html')
    ap.add_argument('--e50m', default='2026 JMTR 50M Entrants.html')
    args = ap.parse_args()

    print("Loading entrants…")
    entrants_15m = load_entrants(args.e15m)
    entrants_50k = load_entrants(args.e50k)
    entrants_50m = load_entrants(args.e50m)
    print(f"  15M={len(entrants_15m)}, 50K={len(entrants_50k)}, 50M={len(entrants_50m)}")

    print("Loading results…")
    results_15m = load_results(args.r15m)
    results_50k = load_results(args.r50k)
    results_50m = load_results(args.r50m)
    print(f"  15M={len(results_15m)}, 50K={len(results_50k)}, 50M={len(results_50m)}")

    bib_15m, name_15m = build_result_indexes(results_15m)
    bib_50k, name_50k = build_result_indexes(results_50k)
    bib_50m, name_50m = build_result_indexes(results_50m)

    # Classify 50M entrants:
    #   finished 50M  → 50M CSV, status 1
    #   finished 50K  → 50K CSV, status 4 (unofficial)
    #   neither       → 50M CSV, status 2 (DNF)
    unofficial_50k: list[dict] = []
    entrants_50m_for_csv: list[dict] = []

    for e in entrants_50m:
        r50m = find_result(e, bib_50m, name_50m)
        r50k = find_result(e, bib_50k, name_50k)

        if r50m and r50m.get('time'):
            entrants_50m_for_csv.append(e)
        elif r50k and r50k.get('time'):
            unofficial_50k.append({'entrant': e, 'result': r50k})
        else:
            entrants_50m_for_csv.append(e)

    print(f"\n50M entrants: {len(entrants_50m)} total")
    print(f"  → 50M CSV: {len(entrants_50m_for_csv)}")
    print(f"  → 50K CSV (unofficial): {len(unofficial_50k)}")

    print()
    rows_15m = build_csv_rows(entrants_15m, bib_15m, name_15m)
    write_csv(rows_15m, 'JMTR_2026_15Mile_UltraSignup.csv')

    rows_50k = build_csv_rows(entrants_50k, bib_50k, name_50k, extra_finishers=unofficial_50k)
    write_csv(rows_50k, 'JMTR_2026_50k_UltraSignup.csv')

    rows_50m = build_csv_rows(entrants_50m_for_csv, bib_50m, name_50m)
    write_csv(rows_50m, 'JMTR_2026_50Mile_UltraSignup.csv')


if __name__ == '__main__':
    main()
