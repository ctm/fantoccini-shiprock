use {
    crate::{Event, Opt, Race, Scraper, Year},
    anyhow::{anyhow, bail, Result as AResult},
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{
        elements::Element,
        error::{CmdError, ErrorStatus::NoSuchElement, WebDriver},
        Client,
        Locator::Css,
    },
    futures::stream::{self, StreamExt},
    serde::Serialize,
    std::num::NonZeroU16,
};

pub struct Params {
    event_id: u32,
    race_menus: &'static [&'static str],
    year: Year,
    participant: bool,
}

static SOLO_MALE_HEAVIES: [&str; 2] = [
    "CIVILIAN Male Heavy",            // 2016
    "Individual CIVILIAN Male Heavy", // 2017 - 2019
];

impl Params {
    pub(crate) fn new(opt: Opt) -> AResult<Self> {
        use Event::*;

        match opt.event {
            BMDM => Self::new_bmdm(opt),
            _ => bail!("{:?} is not It's Your Race", opt.event),
        }
    }

    fn new_bmdm(opt: Opt) -> AResult<Self> {
        use Race::*;

        let race_menus = match opt.race {
            SoloMaleHeavy => &SOLO_MALE_HEAVIES[..],
            _ => bail!("Only solo-male-heavy is available"),
        };

        Ok(Self {
            event_id: 6574,
            race_menus,
            year: opt.year,
            participant: opt.participant,
        })
    }
}

const NEXT_LINK_CSS: &str = "#btnNext";

async fn print_placements(c: &Client) -> AResult<()> {
    c.wait().for_element(Css("#ddlPage")).await?;
    // Yes, they really reuse Tr1 in all their trs.
    let placements = stream::iter(c.find_all(Css("tr#Tr1")).await?)
        .filter_map(Placement::from_element)
        .collect::<Vec<_>>()
        .await;
    println!("{}", serde_json::to_string(&placements).unwrap());
    Ok(())
}

async fn print_participants(c: &Client) -> AResult<()> {
    c.wait().for_element(Css("#ddlPage")).await?;
    // Yes, they really reuse Tr1 in all their trs.
    let participants = stream::iter(c.find_all(Css("tr#Tr1")).await?)
        .filter_map(Participant::from_element)
        .collect::<Vec<_>>()
        .await;
    println!("{}", serde_json::to_string(&participants).unwrap());
    Ok(())
}

async fn next_button(c: &Client) -> AResult<Option<Element>> {
    match c.find(Css(NEXT_LINK_CSS)).await {
        Ok(element) => Ok(Some(element)),
        Err(CmdError::Standard(WebDriver {
            error: NoSuchElement,
            ..
        })) => Ok(None),
        Err(err) => bail!(err),
    }
}

async fn extract_placements(c: &Client) -> AResult<()> {
    let mut button;

    while {
        print_placements(c).await?;
        button = next_button(c).await?;
        button.is_some()
    } {
        button.unwrap().click().await?;
    }
    Ok(())
}

// TODO: DRY this with extract_placements.  It's trivial to do with macros,
//       but without being able to have an async fn passed in as a
//       parameter, it's a bit hard without macros (although that may
//       be due to my lack of Rust knowledge).  There's a discussion
//       about emulating async fn pointers on Stack Overflow:
//       https://stackoverflow.com/questions/66769143/rust-passing-async-function-pointers

async fn extract_participants(c: &Client) -> AResult<()> {
    let mut button;

    while {
        print_participants(c).await?;
        button = next_button(c).await?;
        button.is_some()
    } {
        button.unwrap().click().await?;
    }
    Ok(())
}

#[derive(Serialize)]
struct Placement {
    rank: NonZeroU16,
    name: String,
    bib: String,
    time: Duration,
    pace: Duration,
}

macro_rules! element_text {
    ($e:ident, $s:literal) => {
        $e.find(Css($s)).await.ok()?.text().await.ok()
    };
}

macro_rules! parsed_element_text {
    ($e:ident, $s:literal) => {
        element_text!($e, $s)?.parse().ok()
    };
}

impl Placement {
    async fn from_element(e: Element) -> Option<Self> {
        async fn from_element(e: &Element) -> Option<Placement> {
            let rank = parsed_element_text!(e, ".placeOverall")?;

            let (name, bib) = {
                let name_and_bib = element_text!(e, ".name")?;
                let pieces = name_and_bib.split(" (# ").collect::<Vec<_>>();
                if pieces.len() != 2 {
                    eprintln!("expected two pieces in {name_and_bib}");
                    return None;
                }
                match pieces[1].find(')') {
                    None => {
                        eprintln!("couldn't find closing paren in {}", pieces[1]);
                        return None;
                    }
                    Some(n) => (pieces[0].to_string(), pieces[1][..n].to_string()),
                }
            };
            let time = parsed_element_text!(e, ".chiptime")?;
            let pace = {
                let pace = element_text!(e, ".pace")?;
                match pace.strip_suffix("/mile") {
                    None => {
                        eprintln!("Couldn't find /mile in {pace}");
                        return None;
                    }
                    Some(pace) => pace.parse().ok()?,
                }
            };
            Some(Placement {
                rank,
                name,
                bib,
                time,
                pace,
            })
        }
        let result = from_element(&e).await;
        if result.is_none() {
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
            eprintln!("discarding {text:?}");
        }
        result
    }
}

async fn pop_up_select(c: &Client, selector: &str, matches: &[&str]) -> AResult<()> {
    let e = c.wait().for_element(Css(selector)).await.map_err(|e| {
        let message = format!("Couldn't find {selector}: {e:?}");
        eprintln!("{}", message);
        anyhow!(message)
    })?;

    let mut found = false;
    for label in matches.iter() {
        match e.select_by_label(label).await {
            Ok(_) => {
                found = true;
                break;
            }
            Err(CmdError::Standard(WebDriver {
                error: NoSuchElement,
                ..
            })) => {} // ignore
            Err(e) => {
                dbg!(e);
            } // this is a surprise
        }
    }
    if !found {
        bail!("Couldn't find {matches:?} via {selector}");
    }
    Ok(())
}

async fn select_year(c: &Client, year: Year) -> AResult<()> {
    let year = year.to_string();
    let years = [year.as_ref()];
    pop_up_select(c, "#ddlYear", &years[..]).await
}

async fn select_race(c: &Client, race_menus: &[&str]) -> AResult<()> {
    pop_up_select(c, "#ddlRace", race_menus).await
}

async fn select_participant(c: &Client) -> AResult<()> {
    let link = c.find(Css("#lnkParticipants")).await?;
    link.click().await?;
    Ok(())
}

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        format!(
            "https://www.itsyourrace.com/Results.aspx?id={}",
            self.event_id
        )
    }

    async fn doit(&self, client: &Client) -> AResult<()> {
        if self.participant {
            select_participant(client).await?;
            extract_participants(client).await
        } else {
            select_year(client, self.year).await?;
            select_race(client, self.race_menus).await?;
            extract_placements(client).await
        }
    }
}

#[derive(Serialize)]
struct Participant {
    name: String,
    bib: String,
    hometown: String,
    race: String,
    age_group: String,
}

impl Participant {
    async fn from_element(e: Element) -> Option<Self> {
        let mut tds = e.find_all(Css("td")).await.ok()?.into_iter();
        let _ = tds.next()?;
        let (name, bib, hometown) = {
            let name_bib_hometown = tds.next()?.text().await.ok()?;
            let pieces = name_bib_hometown.split('\n').collect::<Vec<_>>();
            if pieces.len() != 2 {
                eprintln!("expected one newline in {name_bib_hometown}");
                return None;
            }
            let sub_pieces = pieces[0].split(" ( Bib # ").collect::<Vec<_>>();
            match sub_pieces.len() {
                1 => (pieces[0].to_string(), String::new(), pieces[1].to_string()),
                2 => {
                    let end = match sub_pieces[1].find(" )") {
                        None => {
                            eprintln!("Couldn't find closing paren in {name_bib_hometown}");
                            return None;
                        }
                        Some(end) => end,
                    };
                    (
                        sub_pieces[0].to_string(),
                        sub_pieces[1][..end].to_string(),
                        pieces[1].to_string(),
                    )
                }
                _n => {
                    eprintln!("found multiple bibs in {name_bib_hometown}");
                    return None;
                }
            }
        };
        let race = tds.next()?.text().await.ok()?;
        let age_group = tds.next()?.text().await.ok()?;
        Some(Self {
            name,
            bib,
            hometown,
            race,
            age_group,
        })
    }
}
