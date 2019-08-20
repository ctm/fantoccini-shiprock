use {
    crate::{
        duration_serializer, take_until_and_consume, Opt, Race, ReallyClickable, Scraper, Year,
    },
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{error::CmdError, Client, Element, Locator::Css},
    nom::{
        bytes::complete::{tag, take, take_until},
        combinator::{map, map_res, opt, value},
        multi::{many1, many_m_n},
        sequence::{preceded, tuple},
        IResult,
    },
    serde::Serialize,
    std::{
        num::{NonZeroU16, NonZeroU8},
        str::FromStr,
    },
};

pub struct Params {
    url: &'static str,
    race_index: usize,
}

impl Params {
    pub fn new(opt: Opt) -> Self {
        use {Race::*, Year::*};

        let url = match opt.year {
            Y2019 => "https://www.athlinks.com/event/34346/results/Event/729962/Results",
            _ => panic!("We currently only scrape Run for the Zoo 2019"),
        };

        let race_index = match opt.race {
            TenK => 0,
            Half => 1,
            FiveK => 2,
            _ => panic!("Only Half, 10kl and 5k are available"),
        };

        Self { url, race_index }
    }
}

async fn click_view_all(mut c: Client, index: usize) -> Result<Client, CmdError> {
    c = c
        .wait_for_find(Css("div.col-md-3.col-12>button"))
        .await?
        .click()
        .await?;

    c.clone().wait_for_find(Css(".view-all-results")).await?;

    Ok(c.find_all(Css(".view-all-results"))
        .await?
        .remove(index)
        .really_click()
        .await?)
}

const BUTTON_CSS: &str = "#pager>div>div>button";

async fn print_placements(mut c: Client) -> Result<(), CmdError> {
    c.clone().wait_for_find(Css(BUTTON_CSS)).await?;
    let text = c.source().await?;
    if let Ok((_, placements)) = placements(&text) {
        println!("{}", serde_json::to_string(&placements).unwrap());
    }
    Ok(())
}

async fn next_button(mut c: Client) -> Result<Option<Element>, CmdError> {
    let buttons = c.find_all(Css(BUTTON_CSS)).await?;
    let mut e = buttons.last().unwrap().clone();
    let done = e.html(true).await? != "&gt;";
    Ok(if done { None } else { Some(e) })
}

async fn extract_placements(c: Client) -> Result<Client, CmdError> {
    let mut button;

    while {
        print_placements(c.clone()).await?;
        button = next_button(c.clone()).await?;
        button.is_some()
    } {
        button.unwrap().click().await?;
    }
    Ok(c)
}

#[derive(Serialize)]
struct Placement {
    name: String,
    sex: String,
    age: Option<NonZeroU8>,
    bib: String,
    hometown: String,
    rank: NonZeroU16,
    gender_rank: NonZeroU16,
    division_rank: NonZeroU16,
    #[serde(serialize_with = "duration_serializer")]
    pace: Duration,
    #[serde(serialize_with = "duration_serializer")]
    time: Duration,
}

impl Placement {
    // new takes its arguments as a tuple so that it has a single argument and
    // hence can be used as the second argument to map.
    #[allow(clippy::type_complexity)]
    fn new<'a>(
        (name, sex, age, bib, hometown, rank, gender_rank, division_rank, pace, time): (
            &'a str,
            &'a str,
            Option<NonZeroU8>,
            &'a str,
            &'a str,
            NonZeroU16,
            NonZeroU16,
            NonZeroU16,
            Duration,
            Duration,
        ),
    ) -> Self {
        let name = name.to_string();
        let sex = sex.to_string();
        let bib = bib.to_string();
        let hometown = hometown.to_string();
        Self {
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
        }
    }
}

// Placement parsers

fn placements(input: &str) -> IResult<&str, Vec<Placement>> {
    many1(placement)(input)
}

fn placement(input: &str) -> IResult<&str, Placement> {
    map(
        tuple((
            preceded(
                // name
                tuple((
                    take_until("<div class=\"athName\""),
                    take_until_and_consume(">"),
                )),
                take_until("<"),
            ),
            preceded(close_elem(4), take(1usize)), // sex
            opt(
                // age
                map_res(preceded(tag(" "), take_until("<")), |age: &str| age.parse()),
            ),
            preceded(take_until_and_consume("Bib "), take_until("<")), // bib
            after_n_close_elements(3),                                 // city
            parsed_after_n_close_elements(7),                          // rank
            parsed_after_n_close_elements(2),                          // gender_rank
            parsed_after_n_close_elements(2),                          // division_rank
            parsed_after_n_close_elements(4),                          // pace
            map_res(
                preceded(
                    // time
                    take_until_and_consume("-->"),
                    take_until("<"),
                ),
                |time| time.parse(),
            ),
        )),
        Placement::new,
    )(input)
}

fn close_elem<'a>(count: usize) -> impl Fn(&'a str) -> IResult<&'a str, ()> {
    value((), many_m_n(count, count, take_until_and_consume(">")))
}

fn after_n_close_elements<'a>(count: usize) -> impl Fn(&'a str) -> IResult<&'a str, &'a str> {
    preceded(close_elem(count), take_until("<"))
}

fn parsed_after_n_close_elements<'a, O: FromStr>(
    count: usize,
) -> impl Fn(&'a str) -> IResult<&'a str, O> {
    map_res(after_n_close_elements(count), |string| string.parse())
}

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        self.url.to_string()
    }

    async fn doit(&self, mut client: Client) -> Result<Client, CmdError> {
        let race_index = self.race_index;

        client = click_view_all(client, race_index).await?;
        client = extract_placements(client).await?;
        Ok(client)
    }
}
