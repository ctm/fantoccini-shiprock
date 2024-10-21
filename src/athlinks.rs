use {
    crate::{Event, Opt, Race, ReallyClickable, Scraper, Year},
    anyhow::{anyhow, bail, Result as AResult},
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{elements::Element, Client, Locator::Css},
    futures::stream::{self, StreamExt},
    serde::Serialize,
    serde_json::value,
    std::num::{NonZeroU16, NonZeroU8},
};

pub struct Params {
    event_id: u32,
    race_index: usize,
    year: Year,
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
            _ => bail!("{:?} is not athlinks", opt.event),
        }
    }

    fn new_rtfz(opt: Opt) -> AResult<Self> {
        use Race::*;

        // UGH! See issue #8
        let race_index = match opt.race {
            TenK => {
                if opt.year.0 == 2022 {
                    0
                } else {
                    1
                }
            }
            Half => {
                if opt.year.0 == 2022 {
                    1
                } else {
                    0
                }
            }
            FiveK => 2,
            _ => bail!("Only Half, 10k and 5k are available"),
        };

        Ok(Self {
            event_id: 34346,
            race_index,
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
            race_index: 0,
            year: opt.year,
        })
    }

    fn new_dcm(opt: Opt) -> AResult<Self> {
        use Race::*;

        let race_index = if opt.year == "2022".parse().unwrap() {
            match opt.race {
                Full => 0,
                Half => 3,
                FiveK => 4,
                TenK => 7,
                _ => bail!("Only Full, Half, 10k and 5k are available"),
            }
        } else if opt.year == "2023".parse().unwrap() {
            match opt.race {
                Full => 0,
                Half => 4,
                FiveK => 5,
                TenK => 8,
                _ => bail!("Only Full, Half, 10k and 5k are available"),
            }
        } else if opt.year == "2024".parse().unwrap() {
            match opt.race {
                Full => 0,
                Half => 5,
                FiveK => 6,
                TenK => 9,
                _ => bail!("Only Full, Half, 10k and 5k are available"),
            }
        } else {
            panic!("Only 2022, 2023 or 2024 (for now)");
        };

        Ok(Self {
            event_id: 35398,
            race_index,
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

        let race_index = match opt.race {
            TenK => 1,
            Half => 0,
            FiveK => 2,
            _ => bail!("Only Half, 10k and 5k are available"),
        };

        Ok(Self {
            event_id: 6398,
            race_index,
            year: opt.year,
        })
    }

    fn new_koth(opt: Opt) -> AResult<Self> {
        use Race::*;

        let race_index = match opt.race {
            FiveK => 0,
            TenK => 1,
            TenKRuck => 2,
            Half => 3,
            _ => bail!("Only 5k, 10k, 10kruck and half are available"),
        };

        Ok(Self {
            event_id: 166931,
            race_index,
            year: opt.year,
        })
    }

    fn new_rio_grande(opt: Opt) -> AResult<Self> {
        use Race::*;

        let race_index = match opt.race {
            FiveK => 1,
            Half => 0,
            _ => bail!("Only 5k and half are available"),
        };

        Ok(Self {
            event_id: 11260,
            race_index,
            year: opt.year,
        })
    }
}

async fn click_view_all(c: &Client, index: usize) -> AResult<()> {
    c.wait()
        .for_element(Css("div.col-md-3.col-12>button"))
        .await?
        .really_click(c)
        .await?;

    c.wait().for_element(Css(".view-all-results")).await?;

    c.find_all(Css(".view-all-results"))
        .await?
        .remove(index)
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
                if let Some(last) = text.split('\n').last() {
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

async fn pop_up_select(c: &Client, selector: &str, containing: &[&str]) -> AResult<()> {
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
    e.click().await?;

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
                    if containing.iter().any(|c| t.contains(c)) {
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
        // Some(e) => e.click().await?,
        Some(e) => {
            e.click().await?
        }
    }
    Ok(())
}

async fn select_year(c: &Client, year: Year) -> AResult<()> {
    let year = year.to_string();
    pop_up_select(c, "#eventDate", &[&year[..]]).await
}

async fn select_full_course(c: &Client) -> AResult<()> {
    pop_up_select(c, "#split", &["Full Course", "Finish"]).await
}

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        format!("https://www.athlinks.com/event/{}/results", self.event_id)
    }

    async fn doit(&self, client: &Client) -> AResult<()> {
        select_year(client, self.year).await?;
        click_view_all(client, self.race_index).await?;
        select_full_course(client).await?;
        extract_placements(client).await
    }
}
