use {
    crate::{
        duration_serializer, take_until_and_consume, Opt, Race, ReallyClickable, Scraper, Year,
    },
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{error::CmdError, Client, Element, Locator::Css},
    nom::{
        bytes::complete::{tag, take_until},
        character::complete::{multispace0, multispace1},
        combinator::{all_consuming, map, map_res, opt, value},
        multi::many1,
        sequence::{preceded, terminated, tuple},
        IResult,
    },
    serde::Serialize,
    std::{collections::HashMap, num::NonZeroU16, str::FromStr},
};

async fn click_the_results_tab(mut c: Client) -> Result<Client, CmdError> {
    Ok(c.wait_for_find(Css("#resultsResultsTab"))
        .await?
        .really_click()
        .await?)
}

async fn choose_the_race(mut c: Client, menu_item: &'static str) -> Result<Client, CmdError> {
    let mut element = c.find(Css("#bazu-full-results-races")).await?;
    let html = element.html(true).await?;
    match value_map_from_options(&html).get(menu_item) {
        None => panic!("Could not find menu item {}", menu_item),
        Some((value, selected)) => Ok(if !selected {
            element.select_by_value(value).await?
        } else {
            c
        }),
    }
}

async fn choose_100_per_page(mut c: Client) -> Result<Client, CmdError> {
    Ok(c.find(Css("#bazu-full-results-paging"))
        .await?
        .select_by_value("100")
        .await?)
}

async fn print_placements(mut c: Client) -> Result<(), CmdError> {
    let text = c.source().await?;
    if let Ok((_, placements)) = placements(&text) {
        println!("{}", serde_json::to_string(&placements).unwrap());
    }
    Ok(())
}

async fn next_button(mut c: Client) -> Result<Option<Element>, CmdError> {
    let mut element = c.find(Css("#bazu-full-results-grid_next")).await?;
    Ok(if let Some(classes) = element.attr("class").await? {
        if classes.contains("ui-state-disabled") {
            None
        } else {
            Some(element)
        }
    } else {
        None
    })
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
    rank: NonZeroU16,
    name: String,
    bib: String,
    #[serde(serialize_with = "duration_serializer")]
    time: Duration,
    #[serde(serialize_with = "duration_serializer")]
    pace: Duration,
    hometown: String,
    age: Option<u8>,
    sex: Option<String>,
    division: String,
    division_rank: NonZeroU16,
}

impl Placement {
    // new takes its arguments as a tuple so that it has a single argument and
    // hence can be used as the second argument to map.
    #[allow(clippy::type_complexity)]
    fn new<'a>(
        (rank, name, bib, time, pace, hometown, age, sex, division, division_rank): (
            NonZeroU16,
            &'a str,
            &'a str,
            Duration,
            Duration,
            &'a str,
            Option<u8>,
            Option<&'a str>,
            &'a str,
            NonZeroU16,
        ),
    ) -> Self {
        let name = name.to_string();
        let bib = bib.to_string();
        let hometown = hometown.to_string();
        let sex = sex.map(|sex| sex.to_string());
        let division = division.to_string();
        Self {
            rank,
            name,
            bib,
            time,
            pace,
            hometown,
            age,
            sex,
            division,
            division_rank,
        }
    }
}

// Placement parsers

fn placements(input: &str) -> IResult<&str, Vec<Placement>> {
    preceded(
        tuple((
            take_until("<tbody class=\"ui-widget-content\" role=\"alert\""),
            take_until_and_consume(">"),
        )),
        many1(placement),
    )(input)
}

fn placement(input: &str) -> IResult<&str, Placement> {
    map(
        tuple((
            preceded(tr, map_res(td("rank"), |rank| rank.parse())),
            td("name"),
            td("bib"),
            parsed_td("time"),
            parsed_td("pace"),
            td("hometown"),
            opt(parsed_td("age")),
            opt(td("sex")),
            td("agroup"),
            terminated(parsed_td("agrank"), close_tr),
        )),
        Placement::new,
    )(input)
}

fn tr(input: &str) -> IResult<&str, ()> {
    value(
        (),
        tuple((multispace0, tag("<tr "), take_until_and_consume(">"))),
    )(input)
}

#[allow(clippy::needless_lifetimes)]
fn parsed_td<'a, O: FromStr>(to_match: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, O> {
    map_res(td(to_match), |string| string.parse())
}

#[allow(clippy::needless_lifetimes)]
fn td<'a>(to_match: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> + '_ {
    preceded(
        tuple((
            multispace0,
            tag("<td class=\"ui-widget-content bazu-"),
            tag(to_match),
            tag("\">"),
            take_until_and_consume(">"),
        )),
        terminated(
            take_until_and_consume("<"),
            tuple((take_until("</td>"), tag("</td>"))),
        ),
    )
}

fn close_tr(input: &str) -> IResult<&str, ()> {
    value((), tuple((multispace0, tag("</tr>"))))(input)
}

type ValueMap<'a> = HashMap<&'a str, (&'a str, bool)>;

fn value_map_from_options(input: &str) -> ValueMap {
    map(all_consuming(many1(option)), |v| {
        let mut vm = ValueMap::new();

        for (value, selected, menu_item) in v {
            vm.insert(menu_item, (value, selected));
        }
        vm
    })(input)
    .unwrap_or_else(|_| panic!("Could not parse {}", input))
    .1
}

fn option(input: &str) -> IResult<&str, (&str, bool, &str)> {
    tuple((
        preceded(
            tuple((multispace0, tag("<option value=\""))),
            take_until_and_consume("\""),
        ),
        map(opt(tuple((multispace1, tag("selected=\"\"")))), |s| {
            s.is_some()
        }),
        preceded(tag(">"), take_until_and_consume("</option>")),
    ))(input)
}

fn menu_item_for(race: &Race) -> &'static str {
    use Race::*;

    match race {
        Full => "Shiprock Marathon",
        Half => "Shiprock Half Marathon",
        Relay => "Shiprock Marathon Relay",
        TenK => "Shiprock 10k",
        FiveK => "Shiprock 5k",
        Handcycle => "Shiprock Marathon Handcycle",
    }
}

pub struct Params {
    year: Year,
    menu_item: &'static str,
}

impl Params {
    pub fn new(opt: Opt) -> Self {
        let year = opt.year;
        let menu_item = menu_item_for(&opt.race);

        Self { year, menu_item }
    }
}

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        use Year::*;

        format!(
            "https://results.chronotrack.com/event/results/event/event-{}",
            match self.year {
                Y2017 => "24236",
                Y2018 => "33304",
                Y2019 => "40479",
                Y2020 => panic!("2020 not available yet"),
            }
        )
    }

    async fn doit(&self, mut client: Client) -> Result<Client, CmdError> {
        let menu_item = self.menu_item;

        client = click_the_results_tab(client).await?;
        client = choose_the_race(client, menu_item).await?;
        client = choose_100_per_page(client).await?;
        Ok(extract_placements(client).await?)
    }
}
