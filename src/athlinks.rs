use {
    crate::{ElementExt, Event, Opt, Race, Scraper, Year},
    anyhow::{anyhow, bail, Result as AResult},
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{
        elements::Element,
        Client,
        Locator::{Css, XPath},
    },
    futures::stream::{self, StreamExt},
    serde::Serialize,
    serde_json::value,
    std::num::{NonZeroU16, NonZeroU8},
};

const DATE_DIV: &str = "#eventDate";
const RACE_DIV: &str = "#race";

pub struct Params {
    event_id: u32,
    second_id: u32,
    race: Race,
    year: Year,
}

#[derive(Copy, Clone)]
enum Match {
    Contains,
    Exact,
}

impl Params {
    pub(crate) fn new(opt: Opt) -> AResult<Self> {
        use Event::*;

        match opt.event {
            Rftz => Self::new_rtfz(opt),
            Lt100 => Self::new_lt100(opt),
            DukeCityMarathon => Self::new_dcm(opt),
            CorralesDitchRun => Self::new_cdr(opt),
            KotH => Self::new_koth(opt),
            RioGrande => Self::new_rio_grande(opt),
            DoggieDash => Self::new_doggie_dash(opt),
            RioDelLago => Self::new_rdl(opt),
            _ => bail!("{:?} is not athlinks", opt.event),
        }
    }

    fn new_rtfz(opt: Opt) -> AResult<Self> {
        use Race::*;

        match opt.race {
            TenK | Half | FiveK => {}
            _ => bail!("Only Half, 10k and 5k are available"),
        };

        Ok(Self {
            event_id: 34346,
            second_id: 729962,
            race: opt.race,
            year: opt.year,
        })
    }

    fn new_lt100(opt: Opt) -> AResult<Self> {
        use Race::*;

        if let Full = opt.race {
        } else {
            bail!("Only the full is available");
        }

        Ok(Self {
            event_id: 33913,
            second_id: 1064640,
            race: opt.race,
            year: opt.year,
        })
    }

    fn new_dcm(opt: Opt) -> AResult<Self> {
        use Race::*;

        if opt.year == "2022".parse().unwrap() {
            match opt.race {
                Full | Half | FiveK | TenK => {}
                _ => bail!("Only Full, Half, 10k and 5k are available"),
            }
        } else if opt.year == "2023".parse().unwrap() {
            match opt.race {
                Full | Half | FiveK | TenK => {}
                _ => bail!("Only Full, Half, 10k and 5k are available"),
            }
        } else if opt.year == "2024".parse().unwrap() {
            match opt.race {
                Full | Half | FiveK | TenK => {}
                _ => bail!("Only Full, Half, 10k and 5k are available"),
            }
        } else {
            bail!("Only 2022, 2023 or 2024 (for now)");
        };

        Ok(Self {
            event_id: 35398,
            second_id: 1032202,
            race: opt.race,
            year: opt.year,
        })
    }

    fn new_cdr(opt: Opt) -> AResult<Self> {
        use Race::*;

        /*
        2008 All, 10k, 5k
        2011 All, 5k, 10k
        2012 All, 10k
        2015 All, 10k, 5k
        2017 All, Half, 10k, 5k
        2018 All, Half, 10k, 5k, Half-3-person-relay
        2019 All, Half, 10k, 5k, Half-3-person-relay
        2021 All, Half, Kids, Virtual-Half, Virtual 10k, Virtual 5k, 5k, 10k
        2022 All, Kids, 10k, Half, 5k
        2023 All, Half, 10k, 5k, SAR-Technical-Team, Kids
         */

        if opt.year != "2023".parse().unwrap() {
            bail!("Only 2023 (for now?)");
        }

        match opt.race {
            TenK | Half | FiveK => {}
            _ => bail!("Only Half, 10k and 5k are available"),
        };

        Ok(Self {
            event_id: 6398,
            second_id: 1079194,
            race: opt.race,
            year: opt.year,
        })
    }

    fn new_koth(opt: Opt) -> AResult<Self> {
        use Race::*;

        match opt.race {
            FiveK | TenK | TenKRuck | Half => {}
            _ => bail!("Only 5k, 10k, 10kruck and half are available"),
        };

        Ok(Self {
            event_id: 166931,
            second_id: 1064112,
            race: opt.race,
            year: opt.year,
        })
    }

    fn new_rio_grande(opt: Opt) -> AResult<Self> {
        use Race::*;

        match opt.race {
            FiveK | Half => {}
            _ => bail!("Only 5k and half are available"),
        };

        Ok(Self {
            event_id: 11260,
            second_id: 1040305,
            race: opt.race,
            year: opt.year,
        })
    }

    fn new_doggie_dash(opt: Opt) -> AResult<Self> {
        use Race::*;

        match opt.race {
            FiveK => {}
            _ => bail!("Only 5k and half are available"),
        };
        Ok(Self {
            event_id: 68104,
            second_id: 1094706,
            race: opt.race,
            year: opt.year,
        })
    }

    fn new_rdl(opt: Opt) -> AResult<Self> {
        use Race::*;

        if let Full = opt.race {
        } else {
            bail!("Only the full is available");
        }
        Ok(Self {
            event_id: 63638,
            second_id: 991871,
            race: opt.race,
            year: opt.year,
        })
    }

    async fn accept_cookies(&self, c: &Client) -> AResult<()> {
        c.wait()
            .for_element(XPath("//button[text()='okay, got it']"))
            .await?
            .click()
            .await
            .map_err(Into::into)
    }

    async fn click_date_to_bring_up_event_filter(&self, c: &Client) -> AResult<()> {
        const DATE_BUTTON: &str = "div.MuiChip-clickable";

        let e = c.wait().for_element(Css(DATE_BUTTON)).await.map_err(|e| {
            let message = format!("Couldn't find {}: {e:?}", DATE_BUTTON);
            eprintln!("{}", message);
            anyhow!(message)
        })?;
        e.click().await?;
        c.wait().for_element(Css(DATE_DIV)).await.map_err(|e| {
            let message = format!("Couldn't find {}: {e:?}", DATE_DIV);
            eprintln!("{}", message);
            anyhow!(message)
        })?;
        Ok(())
    }

    async fn select_year(&self, c: &Client) -> AResult<()> {
        let year = self.year.to_string();
        pop_up_select(c, DATE_DIV, &[&year[..]], Match::Contains).await
    }

    async fn select_race(&self, c: &Client) -> AResult<()> {
        pop_up_select(c, RACE_DIV, self.race.li_text(), Match::Exact).await
    }
}

async fn click_apply_filter(c: &Client) -> AResult<()> {
    c.wait()
        .for_element(XPath("//span[text()='Apply Filter']"))
        .await?
        .really_click(c)
        .await
}

const BUTTON_CSS: &str = "#pager>div>div>button";

async fn print_placements(c: &Client) -> AResult<()> {
    c.wait().for_element(Css(BUTTON_CSS)).await?;
    let placements = stream::iter(
        c.find_all(Css(".row.mx-0.link-to-irp"))
            .await?
            .into_iter()
            .take(50),
    )
    .filter_map(Placement::from_element)
    .collect::<Vec<_>>()
    .await;
    println!("{}", serde_json::to_string(&placements).unwrap());
    Ok(())
}

async fn next_button(c: &Client) -> AResult<Option<Element>> {
    let buttons = c.find_all(Css(BUTTON_CSS)).await?;
    let e = buttons.last().unwrap();
    let done = e.html(true).await? != "&gt;";
    Ok(if done { None } else { Some(e.clone()) })
}

async fn extract_placements(c: &Client) -> AResult<()> {
    let mut button;

    while {
        print_placements(c).await?;
        button = next_button(c).await?;
        button.is_some()
    } {
        button.unwrap().click().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
    Ok(())
}

#[derive(Serialize)]
struct Placement {
    name: String,
    sex: String,
    age: Option<NonZeroU8>,
    bib: String,
    hometown: String,
    rank: NonZeroU16,
    gender_rank: Option<NonZeroU16>,
    division_rank: NonZeroU16,
    pace: Duration,
    time: Duration,
}

macro_rules! element_text {
    ($e:ident, $s:literal) => {
        $e.find(Css($s)).await?.text().await
    };
}

macro_rules! elements_text {
    ($e:ident) => {
        $e.next()
            .ok_or_else(|| anyhow!("no element for text"))?
            .text()
            .await
    };
}

macro_rules! parsed_elements_text {
    ($e:ident) => {
        elements_text!($e)?.parse()
    };
}

fn extract_sex_et_al(pieces: &[&str]) -> AResult<(String, Option<NonZeroU8>, String, String)> {
    let new_pieces;
    let pieces = if pieces.len() == 2 {
        new_pieces = [pieces[0], "", pieces[1]];
        &new_pieces
    } else {
        pieces
    };
    if pieces.len() != 3 {
        bail!("expected three pieces in {pieces:?}")
    }
    let sub_pieces = pieces[0].split(' ').collect::<Vec<_>>();
    let sub_pieces = match sub_pieces.len() {
        0 => ["", ""],
        1 => [sub_pieces[0], ""],
        2 => [sub_pieces[0], sub_pieces[1]],
        _ => {
            bail!("don't know what to do with sub_pieces: {sub_pieces:?}");
        }
    };
    let sex = sub_pieces[0].to_string();
    let age = sub_pieces[1].parse().ok();
    let bib = pieces[1].strip_prefix("Bib ").unwrap_or("").to_string();
    let hometown = pieces[2].to_string();
    Ok((sex, age, bib, hometown))
}

impl Placement {
    async fn from_element(e: Element) -> Option<Self> {
        async fn from_element(e: &Element) -> AResult<Placement> {
            let name = element_text!(e, ".athName")?;

            let (sex, age, bib, hometown) = {
                let text = element_text!(e, ".col-12")?;
                let pieces = text.split('\n').collect::<Vec<_>>();
                extract_sex_et_al(&pieces)?
            };
            let mut es = e.find_all(Css(".px-0")).await?.into_iter();
            let rank = parsed_elements_text!(es)?;
            let gender_rank = parsed_elements_text!(es).ok();
            let division_rank = parsed_elements_text!(es)?;
            let pace = elements_text!(es)?.split('\n').next().unwrap().parse()?;
            let time = parsed_elements_text!(es)?;
            Ok(Placement {
                name,
                sex,
                age,
                bib,
                hometown,
                rank,
                gender_rank,
                division_rank,
                pace,
                time,
            })
        }
        let result = from_element(&e).await;
        if let Err(err) = &result {
            // If this line is being discarded, we want to dump enough
            // info to figure out why.  We know we're going to ignore
            // column headings and DNFs.
            let text = e.text().await;
            if let Ok(text) = text.as_ref() {
                if let Some(last) = text.split('\n').next_back() {
                    if last == "DNF" || last == "TIME" {
                        return None;
                    }
                }
            }
            eprintln!("discarding {text:?}, {err:?}");
        }
        result.ok()
    }
}

// This should choose Event Date and Race. Division and Split will default
// to the values we need ("Overall" and "All Splits").
// I've verified this with each of the Athlinks events we currently scrape.
// filter options.

async fn pop_up_select(c: &Client, selector: &str, containing: &[&str], m: Match) -> AResult<()> {
    let e = c.wait().for_element(Css(selector)).await.map_err(|e| {
        let message = format!("Couldn't find {selector}: {e:?}");
        eprintln!("{}", message);
        anyhow!(message)
    })?;

    if let Some(class) = e.attr("class").await? {
        if class.contains("Mui-disabled") {
            return Ok(());
        }
    }

    c.execute(
        "arguments[0].scrollIntoView({ block: \"center\" })",
        vec![value::to_value(&e)?],
    )
    .await?;
    e.obscured_click(c).await?;

    let e = c
        .wait()
        .for_element(Css("div.MuiPopover-paper ul"))
        .await
        .map_err(|e| {
            let message = format!("Couldn't find popover {selector}: {e:?}");
            eprintln!("{}", message);
            anyhow!(message)
        })?;

    let mut stream = stream::iter(e.find_all(Css("li")).await?);
    let mut found = None;
    while {
        let e;
        (e, stream) = stream.into_future().await;
        match e {
            None => false,
            Some(e) => match e.text().await {
                Err(e) => {
                    eprintln!("trouble with stream: {e:?}");
                    false
                }
                Ok(t) => {
                    // NOTE: contains won't work for race.  It's fine for year,
                    // but "Half Marathon" contains "Marathon"
                    if containing.iter().any(|c| match m {
                        Match::Contains => t.contains(c),
                        Match::Exact => t == *c,
                    }) {
                        found = Some(e);
                        false
                    } else {
                        true
                    }
                }
            },
        }
    } {}
    match found {
        None => bail!("Could not find {selector} {:?}", containing),
        Some(e) => e.click().await?,
    }
    Ok(())
}

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        format!(
            "https://www.athlinks.com/event/{}/results/Event/{}/Results",
            self.event_id, self.second_id
        )
    }

    async fn doit(&self, client: &Client) -> AResult<()> {
        self.accept_cookies(client).await?;
        self.click_date_to_bring_up_event_filter(client).await?;
        self.select_year(client).await?;
        self.select_race(client).await?;
        click_apply_filter(client).await?;
        extract_placements(client).await
    }
}

trait RaceExt {
    fn li_text(&self) -> &'static [&'static str];
}

impl RaceExt for Race {
    fn li_text(&self) -> &'static [&'static str] {
        use Race::*;
        match self {
            Full => &["Marathon", "100 Mile Endurance Run"],
            Half => &["Half Marathon", "Lovelace Rio Grande Half Marathon"],
            Relay => unreachable!(),
            TenK => &["10K Timed", "10K Run", "10k"],
            FiveK => &[
                "5K Timed",
                "5K Run",
                "5K Run/Walk",
                "5k",
                "Garrity Group 5k Run/Walk",
                "One Medal 5k Run/Walk",
                "Doggie Dash",
            ],
            Handcycle => unreachable!(),
            TenKRuck => &["10k Rucksack"],
            SoloMaleHeavy => unreachable!(),
        }
    }
}
