// Scraper for the JMTR (Jemez Mountain Trail Runs) 2026 results.
//
// sites.chronotrack.com is a Next.js SPA backed by:
//   https://reignite-api.athlinks.com/azp/ctlive/event/{eventId}/race/{raceId}/division/{divisionId}/results?from=0&limit=50
//
// We navigate to the results page (establishing the correct browser origin
// so CORS is satisfied), then use execute_async() to call the API directly
// with fetch() from within the browser.  This avoids all DOM scraping.
//
// Race / division IDs confirmed via intercepted network calls:
//   15 Mile : raceId=242432  divisionId=2719234
//   50K     : raceId=242431  divisionId=2719231
//   50 Mile : raceId=242430  divisionId=discovered at runtime

use {
    crate::{ClientExt, Opt, Race, Scraper},
    anyhow::{bail, Result as AResult},
    async_trait::async_trait,
    fantoccini::Client,
    serde::Serialize,
    serde_json::Value,
};

const EVENT_ID: u32 = 91454;
const API_BASE: &str = "https://reignite-api.athlinks.com/azp/ctlive";
const PAGE_LIMIT: u32 = 250;

pub struct Params {
    race: JmtrRace,
}

impl Params {
    pub(crate) fn new(opt: Opt) -> AResult<Self> {
        let race = JmtrRace::from_race(&opt.race)?;
        Ok(Self { race })
    }
}

#[derive(Clone, Copy, Debug)]
enum JmtrRace {
    FifteenMile,
    FiftyK,
    FiftyMile,
}

impl JmtrRace {
    fn from_race(r: &Race) -> AResult<Self> {
        use Race::*;
        match r {
            FiveK => Ok(Self::FifteenMile),
            Half => Ok(Self::FiftyK),
            Full => Ok(Self::FiftyMile),
            _ => bail!("For jmtr use: -r 5k (15 Mile), -r half (50K), or -r full (50 Mile)"),
        }
    }

    fn race_id(self) -> u32 {
        match self {
            Self::FifteenMile => 242432,
            Self::FiftyK => 242431,
            Self::FiftyMile => 242430,
        }
    }

    fn division_id(self) -> Option<u32> {
        match self {
            Self::FifteenMile => Some(2719234),
            Self::FiftyK => Some(2719231),
            Self::FiftyMile => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::FifteenMile => "15 Mile",
            Self::FiftyK => "50K",
            Self::FiftyMile => "50 Mile",
        }
    }

    fn page_url(self) -> String {
        let base = format!(
            "https://sites.chronotrack.com/event/{EVENT_ID}/results?raceId={}",
            self.race_id()
        );
        match self.division_id() {
            Some(div) => format!("{}&divisionId={}", base, div),
            None => base,
        }
    }
}

#[derive(Serialize, Debug)]
pub struct Placement {
    pub place: u32,
    pub name: String,
    pub bib: String,
    pub time: String,
    pub time_ms: u64,
    pub gender: String,
    pub age: Option<u32>,
    pub city: String,
    pub state: String,
}

// Async fetch called from inside the browser — avoids CORS entirely.
const FETCH_URL_JS: &str = r#"
const url = arguments[0];
const callback = arguments[arguments.length - 1];
fetch(url, { headers: { 'Accept': 'application/json' } })
    .then(r => r.text())
    .then(t => callback(t))
    .catch(e => callback('{"error":"' + e.toString() + '"}'));
"#;

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        self.race.page_url()
    }

    async fn doit(&self, client: &Client) -> AResult<()> {
        eprintln!("[jmtr] Scraping {}.", self.race.label());

        // Brief pause after navigation for the page to initialise.
        client
            .pause(std::time::Duration::from_secs(2))
            .await
            .unwrap_or_default();

        let division_id = match self.race.division_id() {
            Some(id) => id,
            None => discover_division_id(client, self.race).await?,
        };

        let placements = fetch_all_results(client, self.race, division_id).await?;
        println!("{}", serde_json::to_string(&placements)?);
        eprintln!("[jmtr] {} placements.", placements.len());

        Ok(())
    }
}

async fn discover_division_id(client: &Client, race: JmtrRace) -> AResult<u32> {
    // Fetch one result and read divisions.overall from the first athlete.
    let url = format!(
        "{API_BASE}/event/{EVENT_ID}/race/{}/division/0/results?from=0&limit=1",
        race.race_id()
    );
    eprintln!("[jmtr] Probing for division ID: {url}");
    let body = browser_fetch(client, &url).await.unwrap_or_default();
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);

    if let Some(intervals) = v["intervals"].as_array() {
        for interval in intervals {
            if interval["full"].as_bool() != Some(true) {
                continue;
            }
            if let Some(results) = interval["results"].as_array() {
                if let Some(first) = results.first() {
                    if let Some(id) = first["divisions"]["overall"].as_u64() {
                        eprintln!("[jmtr] Division ID = {id}");
                        return Ok(id as u32);
                    }
                }
            }
        }
    }

    // Try the awards endpoint as a fallback.
    let url2 = format!("{API_BASE}/event/{EVENT_ID}/race/{}/awards", race.race_id());
    eprintln!("[jmtr] Trying awards endpoint: {url2}");
    let body2 = browser_fetch(client, &url2).await?;
    let v2: Value = serde_json::from_str(&body2).unwrap_or(Value::Null);

    if let Some(arr) = v2.as_array() {
        for item in arr {
            let name = item["division"]["name"]
                .as_str()
                .or_else(|| item["name"].as_str())
                .unwrap_or("");
            if name.to_lowercase().contains("overall") {
                if let Some(id) = item["division"]["id"]
                    .as_u64()
                    .or_else(|| item["id"].as_u64())
                {
                    eprintln!("[jmtr] Division ID = {id} (from awards)");
                    return Ok(id as u32);
                }
            }
        }
    }

    bail!(
        "Could not discover division ID for {}. Run with -d and check the network tab.",
        race.label()
    );
}

async fn fetch_all_results(
    client: &Client,
    race: JmtrRace,
    division_id: u32,
) -> AResult<Vec<Placement>> {
    let mut all: Vec<Placement> = Vec::new();
    let mut from: u32 = 0;

    loop {
        let url = format!(
            "{API_BASE}/event/{EVENT_ID}/race/{}/division/{division_id}/results?from={from}&limit={PAGE_LIMIT}",
            race.race_id()
        );
        eprintln!("[jmtr] GET {url}");

        let body = browser_fetch(client, &url).await?;
        let v: Value = serde_json::from_str(&body).map_err(|e| {
            anyhow::anyhow!(
                "JSON parse error: {e}\nBody start: {}",
                &body[..body.len().min(300)]
            )
        })?;

        if let Some(err) = v["error"].as_str() {
            bail!("API error: {err}");
        }

        let intervals = match v["intervals"].as_array() {
            Some(a) => a,
            None => bail!(
                "No 'intervals' in response. Keys: {:?}",
                v.as_object().map(|o| o.keys().collect::<Vec<_>>())
            ),
        };

        let mut page_count = 0u32;
        for interval in intervals {
            if interval["full"].as_bool() != Some(true) {
                continue;
            }
            if let Some(results) = interval["results"].as_array() {
                for r in results {
                    if let Some(p) = parse_result(r) {
                        all.push(p);
                        page_count += 1;
                    }
                }
            }
        }

        eprintln!("[jmtr]   {page_count} results (from={from})");

        if page_count < PAGE_LIMIT {
            break;
        }
        from += PAGE_LIMIT;
        if from > 10_000 {
            eprintln!("[jmtr] Safety limit; stopping.");
            break;
        }
    }

    Ok(all)
}

fn parse_result(r: &Value) -> Option<Placement> {
    let name = r["displayName"].as_str()?;
    if name.is_empty() {
        return None;
    }

    let bib = r["bib"].as_str().unwrap_or("").to_string();
    let time_ms = r["chipTimeInMillis"].as_u64().unwrap_or(0);
    let time = if time_ms > 0 {
        millis_to_hms(time_ms)
    } else {
        String::new()
    };
    let place = r["rankings"]["overall"].as_u64().unwrap_or(0) as u32;
    let gender = r["gender"].as_str().unwrap_or("").to_string();
    let age = r["age"].as_u64().map(|a| a as u32);
    let city = r["location"]["locality"].as_str().unwrap_or("").to_string();
    let state = r["location"]["region"].as_str().unwrap_or("").to_string();

    Some(Placement {
        place,
        name: name.to_string(),
        bib,
        time,
        time_ms,
        gender,
        age,
        city,
        state,
    })
}

fn millis_to_hms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

async fn browser_fetch(client: &Client, url: &str) -> AResult<String> {
    let result = client
        .execute_async(FETCH_URL_JS, vec![serde_json::json!(url)])
        .await?;
    let body = result.as_str().unwrap_or("{}").to_string();
    if body.is_empty() {
        bail!("Empty response from {url}");
    }
    Ok(body)
}
