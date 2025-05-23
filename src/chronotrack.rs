use {
    crate::{take_until_and_consume, ElementExt, Opt, Race, Scraper, Year},
    anyhow::{anyhow, bail, Result as AResult},
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{elements::Element, Client, Locator::Css},
    nom::{
        bytes::complete::{tag, take_until},
        character::complete::{multispace0, multispace1},
        combinator::{all_consuming, map, map_res, opt, value},
        error::Error,
        multi::many1,
        sequence::{preceded, terminated},
        IResult, Parser,
    },
    serde::Serialize,
    std::{collections::HashMap, num::NonZeroU16, str::FromStr},
};

async fn click_the_results_tab(c: &Client) -> AResult<()> {
    c.wait()
        .for_element(Css("#resultsResultsTab"))
        .await?
        .really_click(c)
        .await
}

async fn choose_the_race(c: &Client, menu_item: &'static str) -> AResult<()> {
    let element = c.find(Css("#bazu-full-results-races")).await?;
    let html = element.html(true).await?;
    match value_map_from_options(&html)?.get(menu_item) {
        None => bail!("Could not find menu item {}", menu_item),
        Some((value, selected)) => {
            if !selected {
                element.select_by_value(value).await?
            };
            Ok(())
        }
    }
}

async fn choose_100_per_page(c: &Client) -> AResult<()> {
    Ok(c.find(Css("#bazu-full-results-paging"))
        .await?
        .select_by_value("100")
        .await?)
}

async fn print_placements(c: &Client) -> AResult<()> {
    let text = c.source().await?;
    if let Ok((_, placements)) = placements(&text) {
        println!("{}", serde_json::to_string(&placements).unwrap());
    }
    Ok(())
}

async fn next_button(c: &Client) -> AResult<Option<Element>> {
    let element = c.find(Css("#bazu-full-results-grid_next")).await?;
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

#[derive(Serialize)]
struct Placement {
    rank: NonZeroU16,
    name: String,
    bib: String,
    time: Duration,
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
        (
            take_until("<tbody class=\"ui-widget-content\" role=\"alert\""),
            take_until_and_consume(">"),
        ),
        many1(placement),
    )
    .parse(input)
}

fn placement(input: &str) -> IResult<&str, Placement> {
    map(
        (
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
        ),
        Placement::new,
    )
    .parse(input)
}

fn tr(input: &str) -> IResult<&str, ()> {
    value((), (multispace0, tag("<tr "), take_until_and_consume(">"))).parse(input)
}

#[allow(clippy::needless_lifetimes)]
fn parsed_td<'a, O: FromStr>(
    to_match: &'a str,
) -> impl Parser<&'a str, Error = Error<&'a str>, Output = O> {
    map_res(td(to_match), |string| string.parse())
}

#[allow(clippy::needless_lifetimes)]
fn td<'a>(to_match: &'a str) -> impl Parser<&'a str, Error = Error<&'a str>, Output = &'a str> {
    preceded(
        (
            multispace0,
            tag("<td class=\"ui-widget-content bazu-"),
            tag(to_match),
            tag("\">"),
            take_until_and_consume(">"),
        ),
        terminated(
            take_until_and_consume("<"),
            (take_until("</td>"), tag("</td>")),
        ),
    )
}

fn close_tr(input: &str) -> IResult<&str, ()> {
    value((), (multispace0, tag("</tr>"))).parse(input)
}

type ValueMap<'a> = HashMap<&'a str, (&'a str, bool)>;

fn value_map_from_options(input: &str) -> AResult<ValueMap> {
    Ok(map(all_consuming(many1(option)), |v| {
        let mut vm = ValueMap::new();

        for (value, selected, menu_item) in v {
            vm.insert(menu_item, (value, selected));
        }
        vm
    })
    .parse(input)
    .map_err(|_| anyhow!("Could not parse {}", input))?
    .1)
}

fn option(input: &str) -> IResult<&str, (&str, bool, &str)> {
    (
        preceded(
            (multispace0, tag("<option value=\"")),
            take_until_and_consume("\""),
        ),
        map(opt((multispace1, tag("selected=\"\""))), |s| s.is_some()),
        preceded(tag(">"), take_until_and_consume("</option>")),
    )
        .parse(input)
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
        TenKRuck => panic!("No 10k ruck at Shiprock"),
        SoloMaleHeavy => panic!("solo-male-heavey is a temporary hack"),
    }
}

pub struct Params {
    year: Year,
    menu_item: &'static str,
}

impl Params {
    pub(crate) fn new(opt: Opt) -> AResult<Self> {
        let year = opt.year;
        if let Err(e) = url_for_year(&year) {
            bail!("Year {year} is not supported: {e}");
        }
        let menu_item = menu_item_for(&opt.race);
        Ok(Self { year, menu_item })
    }
}

fn url_for_year(year: &Year) -> Result<&'static str, &'static str> {
    match year.0 {
        2017 => Ok("24236"),
        2018 => Ok("33304"),
        2019 => Ok("40479"),
        2020 | 2021 => Err("That year was virtual"),
        year if year >= 2022 => Err("Capture the mhtml and use ctm/runs)"),
        _ => Err("Too early"),
    }
}

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        format!(
            "https://results.chronotrack.com/event/results/event/event-{}",
            url_for_year(&self.year).unwrap()
        )
    }

    async fn doit(&self, client: &Client) -> AResult<()> {
        let menu_item = self.menu_item;

        click_the_results_tab(client).await?;
        choose_the_race(client, menu_item).await?;
        choose_100_per_page(client).await?;
        Ok(extract_placements(client).await?)
    }
}
