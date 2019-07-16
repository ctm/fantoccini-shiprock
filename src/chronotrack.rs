use {
    crate::{duration_serializer, really_click, take_until_and_consume, Opt, Race, Scraper, Year},
    digital_duration_nom::duration::Duration,
    fantoccini::{error::CmdError, Client, Locator::Css},
    futures::future::{
        self, Either, Future,
        Loop::{Break, Continue},
    },
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

// Fantoccini futures

fn click_the_results_tab(c: Client) -> impl Future<Item = Client, Error = CmdError> {
    c.wait_for_find(Css("#resultsResultsTab"))
        .and_then(really_click)
}

fn choose_the_race(
    mut c: Client,
    menu_item: &'static str,
) -> impl Future<Item = Client, Error = CmdError> {
    c.find(Css("#bazu-full-results-races"))
        .and_then(move |mut e| {
            e.html(true).and_then(move |html| {
                let value_map = value_map_from_options(&html);

                match value_map.get(menu_item) {
                    None => panic!("Could not find menu item {}", menu_item),
                    Some((value, selected)) => {
                        if !selected {
                            Either::A(e.select_by_value(value))
                        } else {
                            Either::B(future::ok(c))
                        }
                    }
                }
            })
        })
}

fn choose_100_per_page(mut c: Client) -> impl Future<Item = Client, Error = CmdError> {
    c.find(Css("#bazu-full-results-paging"))
        .and_then(|e| e.select_by_value("100"))
}

fn extract_placements(c: Client) -> impl Future<Item = Client, Error = CmdError> {
    future::loop_fn(c, |mut this| {
        let c1 = this.clone();
        this.source()
            .and_then(|text| {
                if let Ok((_, placements)) = placements(&text) {
                    println!("{}", serde_json::to_string(&placements).unwrap());
                }
                future::ok(this)
            })
            .and_then(|mut c| c.find(Css("#bazu-full-results-grid_next")))
            .and_then(|mut e| {
                // Fantoccini doesn't support the IsEnabled command,
                // so we check the class, instead
                e.attr("class")
                    .and_then(move |classes| {
                        let mut done = true;
                        if let Some(classes) = classes {
                            done = classes.contains("ui-state-disabled");
                        }
                        future::ok((e, done))
                    })
                    .and_then(move |(e, done)| {
                        if done {
                            Ok(Break(c1))
                        } else {
                            e.click();
                            Ok(Continue(c1))
                        }
                    })
            })
    })
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
fn parsed_td<'a, O: FromStr>(to_match: &'a str) -> impl Fn(&'a str) -> IResult<&'a str, O> {
    map_res(td(to_match), |string| string.parse())
}

#[allow(clippy::needless_lifetimes)]
fn td<'a>(to_match: &'a str) -> impl Fn(&'a str) -> IResult<&'a str, &'a str> + '_ {
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

impl Scraper for Params {
    fn url(&self) -> String {
        use Year::*;

        format!(
            "https://results.chronotrack.com/event/results/event/event-{}",
            match self.year {
                Y2017 => "24236",
                Y2018 => "33304",
                Y2019 => "40479",
            }
        )
    }

    fn doit(
        &self,
    ) -> Box<Fn(Client) -> Box<Future<Item = Client, Error = CmdError> + Send> + Send> {
        let menu_item = self.menu_item;
        Box::new(move |client| {
            Box::new(
                click_the_results_tab(client)
                    .and_then(move |c| choose_the_race(c, menu_item))
                    .and_then(choose_100_per_page)
                    .and_then(extract_placements),
            )
        })
    }
}
