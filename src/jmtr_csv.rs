// jmtr_csv.rs — post-processor for the JMTR 2026 scraper output.
//
// Reads the three JSON files produced by the jmtr scraper and the three
// UltraSignup entrant HTML exports, then writes three UltraSignup-formatted
// CSV files.
//
// Usage:
//   cargo run --bin jmtr_csv -- \
//     --r15m results_15m.json \
//     --r50k results_50k.json \
//     --r50m results_50m.json \
//     --e15m "2026 JMTR 15M Entrants.html" \
//     --e50k "2026 JMTR 50k Entrants.html" \
//     --e50m "2026 JMTR 50M Entrants.html"
//
// UltraSignup status codes:
//   1 = Finished   2 = DNF   4 = Unofficial Finish

use {
    anyhow::{bail, Context, Result as AResult},
    clap::Parser,
    serde::Deserialize,
    std::{collections::HashMap, fs, io::Write, path::PathBuf},
};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct Opt {
    #[arg(long, default_value = "results_15m.json")]
    r15m: PathBuf,
    #[arg(long, default_value = "results_50k.json")]
    r50k: PathBuf,
    #[arg(long, default_value = "results_50m.json")]
    r50m: PathBuf,
    #[arg(long, default_value = "2026 JMTR 15M Entrants.html")]
    e15m: PathBuf,
    #[arg(long, default_value = "2026 JMTR 50k Entrants.html")]
    e50k: PathBuf,
    #[arg(long, default_value = "2026 JMTR 50M Entrants.html")]
    e50m: PathBuf,
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// One row from the scraper JSON output.
#[derive(Deserialize, Debug, Clone)]
struct ScraperResult {
    #[allow(dead_code)]
    place: u32,
    name: String,
    bib: String,
    time: String,
    time_ms: u64,
    #[allow(dead_code)]
    gender: String,
    #[allow(dead_code)]
    age: Option<u32>,
    #[allow(dead_code)]
    city: String,
    #[allow(dead_code)]
    state: String,
}

/// One entrant from the UltraSignup HTML export.
#[derive(Debug, Clone)]
struct Entrant {
    first: String,
    last: String,
    city: String,
    state: String,
    bib: String,
    gender: String,
    age_group: String,
}

/// One row in the output CSV.
#[derive(Debug)]
struct CsvRow {
    place: Option<u32>,
    first: String,
    last: String,
    age: String,
    gender: String,
    city: String,
    state: String,
    bib: String,
    time: String,
    time_ms: u64,
    status: u8, // 1=finish, 2=DNF, 4=unofficial
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> AResult<()> {
    let opt = Opt::parse();

    eprintln!("Loading entrants…");
    let entrants_15m = load_entrants(&opt.e15m)?;
    let entrants_50k = load_entrants(&opt.e50k)?;
    let entrants_50m = load_entrants(&opt.e50m)?;
    eprintln!(
        "  15M={}, 50K={}, 50M={}",
        entrants_15m.len(),
        entrants_50k.len(),
        entrants_50m.len()
    );

    eprintln!("Loading results…");
    let results_15m = load_results(&opt.r15m)?;
    let results_50k = load_results(&opt.r50k)?;
    let results_50m = load_results(&opt.r50m)?;
    eprintln!(
        "  15M={}, 50K={}, 50M={}",
        results_15m.len(),
        results_50k.len(),
        results_50m.len()
    );

    let idx_15m = build_indexes(&results_15m);
    let idx_50k = build_indexes(&results_50k);
    let idx_50m = build_indexes(&results_50m);

    // Classify 50M entrants.
    let mut unofficial_50k: Vec<(Entrant, ScraperResult)> = Vec::new();
    let mut entrants_50m_csv: Vec<Entrant> = Vec::new();

    for e in &entrants_50m {
        let r50m = find_result(e, &idx_50m);
        let r50k = find_result(e, &idx_50k);
        match (r50m, r50k) {
            (Some(_), _) => entrants_50m_csv.push(e.clone()),
            (None, Some(r)) if !r.time.is_empty() => unofficial_50k.push((e.clone(), r.clone())),
            _ => entrants_50m_csv.push(e.clone()),
        }
    }

    eprintln!("\n50M entrants: {} total", entrants_50m.len());
    eprintln!("  → 50M CSV: {}", entrants_50m_csv.len());
    eprintln!("  → 50K CSV (unofficial): {}", unofficial_50k.len());

    eprintln!();
    let rows_15m = build_rows(&entrants_15m, &idx_15m, &[]);
    write_csv(&rows_15m, "JMTR_2026_15Mile_UltraSignup.csv")?;

    let rows_50k = build_rows(&entrants_50k, &idx_50k, &unofficial_50k);
    write_csv(&rows_50k, "JMTR_2026_50k_UltraSignup.csv")?;

    let rows_50m = build_rows(&entrants_50m_csv, &idx_50m, &[]);
    write_csv(&rows_50m, "JMTR_2026_50Mile_UltraSignup.csv")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Result loading
// ---------------------------------------------------------------------------

fn load_results(path: &PathBuf) -> AResult<Vec<ScraperResult>> {
    if !path.exists() {
        eprintln!("[warn] {:?} not found — treating all as DNF", path);
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    let results: Vec<ScraperResult> =
        serde_json::from_str(&text).with_context(|| format!("parsing {:?}", path))?;
    Ok(results)
}

// ---------------------------------------------------------------------------
// Result lookup
// ---------------------------------------------------------------------------

struct ResultIndex<'a> {
    by_bib: HashMap<&'a str, &'a ScraperResult>,
    by_name: HashMap<String, &'a ScraperResult>,
}

fn build_indexes(results: &[ScraperResult]) -> ResultIndex<'_> {
    let mut by_bib = HashMap::new();
    let mut by_name = HashMap::new();
    for r in results {
        if !r.bib.is_empty() {
            by_bib.insert(r.bib.as_str(), r);
        }
        let key = normalise(&r.name);
        if !key.is_empty() {
            by_name.insert(key, r);
        }
    }
    ResultIndex { by_bib, by_name }
}

fn find_result<'a>(entrant: &Entrant, idx: &ResultIndex<'a>) -> Option<&'a ScraperResult> {
    if let Some(r) = idx.by_bib.get(entrant.bib.as_str()) {
        return Some(r);
    }
    let key = normalise(&format!("{} {}", entrant.first, entrant.last));
    idx.by_name.get(&key).copied()
}

fn normalise(s: &str) -> String {
    // lowercase, collapse whitespace
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// ---------------------------------------------------------------------------
// CSV row building and writing
// ---------------------------------------------------------------------------

fn build_rows(
    entrants: &[Entrant],
    idx: &ResultIndex<'_>,
    extra_unofficial: &[(Entrant, ScraperResult)],
) -> Vec<CsvRow> {
    let mut rows: Vec<CsvRow> = Vec::new();

    for e in entrants {
        let r = find_result(e, idx);
        let has_time = r.is_some_and(|res| !res.time.is_empty());
        rows.push(CsvRow {
            place: None,
            first: e.first.clone(),
            last: e.last.clone(),
            age: age_midpoint(&e.age_group),
            gender: e.gender.clone(),
            city: e.city.clone(),
            state: e.state.clone(),
            bib: e.bib.clone(),
            time: r
                .filter(|_| has_time)
                .map_or(String::new(), |res| res.time.clone()),
            time_ms: r.filter(|_| has_time).map_or(0, |res| res.time_ms),
            status: if has_time { 1 } else { 2 },
        });
    }

    for (e, r) in extra_unofficial {
        rows.push(CsvRow {
            place: None,
            first: e.first.clone(),
            last: e.last.clone(),
            age: age_midpoint(&e.age_group),
            gender: e.gender.clone(),
            city: e.city.clone(),
            state: e.state.clone(),
            bib: e.bib.clone(),
            time: r.time.clone(),
            time_ms: r.time_ms,
            status: 4,
        });
    }

    // Sort: finishers first by time_ms, then unofficial by time_ms, then DNF.
    rows.sort_by_key(|r| (status_order(r.status), r.time_ms));

    // Assign place numbers to official finishers only.
    let mut place = 0u32;
    for row in &mut rows {
        if row.status == 1 {
            place += 1;
            row.place = Some(place);
        }
    }

    rows
}

fn status_order(status: u8) -> u8 {
    match status {
        1 => 0,
        4 => 1,
        _ => 2,
    }
}

fn write_csv(rows: &[CsvRow], path: &str) -> AResult<()> {
    let mut f = fs::File::create(path).with_context(|| format!("creating {path}"))?;

    writeln!(f, "place,first,last,age,gender,city,state,bib,time,status")?;

    for row in rows {
        let place = row.place.map_or(String::new(), |p| p.to_string());
        writeln!(
            f,
            "{},{},{},{},{},{},{},{},{},{}",
            place,
            csv_field(&row.first),
            csv_field(&row.last),
            row.age,
            row.gender,
            csv_field(&row.city),
            csv_field(&row.state),
            row.bib,
            row.time,
            row.status,
        )?;
    }

    let finished = rows.iter().filter(|r| r.status == 1).count();
    let unofficial = rows.iter().filter(|r| r.status == 4).count();
    let dnf = rows.iter().filter(|r| r.status == 2).count();
    let mut parts = vec![format!("{finished} finishers")];
    if unofficial > 0 {
        parts.push(format!("{unofficial} unofficial"));
    }
    parts.push(format!("{dnf} DNF"));
    eprintln!("Wrote {path}  ({})", parts.join(", "));

    Ok(())
}

/// Wrap a field in quotes if it contains a comma.
fn csv_field(s: &str) -> String {
    if s.contains(',') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Age group → midpoint age string
// ---------------------------------------------------------------------------

fn age_midpoint(ag: &str) -> String {
    if ag.is_empty() {
        return String::new();
    }
    if ag == "<20" {
        return "18".to_string();
    }
    if ag == "70+" {
        return "72".to_string();
    }
    // Strip leading gender char or 'X'
    let ag = ag.trim_start_matches(['M', 'F', 'X']);
    let parts: Vec<&str> = ag.splitn(2, '-').collect();
    if parts.len() == 2 {
        if let (Ok(lo), Ok(hi)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
            return ((lo + hi) / 2).to_string();
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// HTML entrant parsing
// ---------------------------------------------------------------------------
//
// The UltraSignup entrant export is a standard ASP.NET GridView table.
// Each data row has exactly 14 <td> cells in this order (0-based):
//   0  rank%       1  age_rank%    2  results_count   3  target_time
//   4  age_group   5  (empty)      6  first_name      7  last_name
//   8  city        9  state        10 (empty)          11 bib
//   12 finishes    13 "Results"
//
// We skip rows that don't have exactly 14 <td> elements.

fn load_entrants(path: &PathBuf) -> AResult<Vec<Entrant>> {
    let html = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;

    let mut entrants = Vec::new();

    // Iterate over <tr> blocks.
    let mut pos = 0;
    while let Some(tr_start) = find_tag(&html, pos, "<tr") {
        let tr_end = match html[tr_start..].find("</tr>") {
            Some(o) => tr_start + o + 5,
            None => break,
        };
        let row_html = &html[tr_start..tr_end];
        pos = tr_end;

        // Collect all <td>…</td> cell texts.
        let cells = collect_td_texts(row_html);
        if cells.len() < 12 {
            continue; // header or filter row
        }

        let age_val = &cells[4]; // e.g. "M30-39" or "F50-59"
        let gender = if age_val.starts_with('M') {
            "M"
        } else if age_val.starts_with('F') {
            "F"
        } else {
            ""
        };
        let age_group = age_val.trim_start_matches(['M', 'F']);

        entrants.push(Entrant {
            first: cells[6].clone(),
            last: cells[7].clone(),
            city: cells[8].clone(),
            state: cells[9].clone(),
            bib: cells[11].clone(),
            gender: gender.to_string(),
            age_group: age_group.to_string(),
        });
    }

    if entrants.is_empty() {
        bail!("No entrants found in {:?}", path);
    }

    Ok(entrants)
}

/// Find the start of the next `<tr` tag at or after `from`.
fn find_tag(html: &str, from: usize, tag: &str) -> Option<usize> {
    html[from..].find(tag).map(|o| from + o)
}

/// Extract inner text of all <td>…</td> cells in `row_html`, stripped of
/// HTML tags and with &nbsp; replaced by a space.
fn collect_td_texts(row_html: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut pos = 0;

    while let Some(td_start) = row_html[pos..].find("<td") {
        let td_start = pos + td_start;
        // Skip to end of opening tag.
        let content_start = match row_html[td_start..].find('>') {
            Some(o) => td_start + o + 1,
            None => break,
        };
        // Find closing tag.
        let td_end = match row_html[content_start..].find("</td>") {
            Some(o) => content_start + o,
            None => break,
        };

        let inner = &row_html[content_start..td_end];
        cells.push(strip_html(inner));
        pos = td_end + 5; // skip past </td>
    }

    cells
}

/// Remove all HTML tags and decode &nbsp; → space, then trim.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut i = 0;
    let bytes = s.as_bytes();

    while i < bytes.len() {
        match bytes[i] {
            b'<' => {
                in_tag = true;
                i += 1;
            }
            b'>' => {
                in_tag = false;
                i += 1;
            }
            b'&' if !in_tag => {
                // Decode common entities.
                if s[i..].starts_with("&nbsp;") {
                    out.push(' ');
                    i += 6;
                } else if s[i..].starts_with("&amp;") {
                    out.push('&');
                    i += 5;
                } else if s[i..].starts_with("&lt;") {
                    out.push('<');
                    i += 4;
                } else if s[i..].starts_with("&gt;") {
                    out.push('>');
                    i += 4;
                } else {
                    out.push('&');
                    i += 1;
                }
            }
            c if !in_tag => {
                out.push(c as char);
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    // Collapse whitespace and trim, matching Python's str.strip().
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
