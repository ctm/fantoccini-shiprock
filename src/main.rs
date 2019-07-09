use {
    digital_duration_nom::duration::Duration,
    fantoccini::{
        error::{
            self,
            CmdError::{self, Standard},
        },
        Client, Element,
        Locator::Css,
    },
    futures::future::{
        self, Either, Future,
        Loop::{Break, Continue},
    },
    nom::{
        bytes::complete::{tag, take, take_until},
        character::complete::{multispace0, multispace1},
        combinator::{all_consuming, map, map_res, opt, value},
        multi::many1,
        sequence::{preceded, terminated, tuple},
        IResult,
    },
    serde::{Serialize, Serializer},
    std::{
        collections::HashMap,
        fmt::{self, Display, Formatter},
        num::NonZeroU16,
        str::FromStr,
    },
    structopt::StructOpt,
    webdriver::error::{ErrorStatus::ElementNotInteractable, WebDriverError},
};

fn main() {
    let opt = Opt::from_args();

    let mut caps = serde_json::map::Map::new();

    let firefox_opts = if opt.display {
        serde_json::json!({ "args": [] })
    } else {
        serde_json::json!({ "args": ["--headless"] })
    };
    caps.insert("moz:firefoxOptions".to_string(), firefox_opts);

    let c = Client::with_capabilities("http://localhost:4444", caps);
    let url = opt.year.url();

    tokio::run(
        c.map_err(|e| unimplemented!("failed to connect to WebDriver: {:?}", e))
            .and_then(move |c| c.goto(&url))
            // NOTE: I'm surprised we need the persist, but we do.
            // It's not yet in a released version though, so we have
            // to pull in the crate from GitHub
            .and_then(|mut c| {
                c.persist();
                future::ok(c)
            })
            .and_then(click_the_results_tab)
            .and_then(move |c| choose_the_race(c, opt.race.menu_item()))
            .and_then(choose_100_per_page)
            .and_then(extract_placements)
            .and_then(|mut c| c.close())
            .map_err(|e| {
                panic!("a WebDriver command failed: {:?}", e);
            }),
    );
}

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

// Fantoccini helper

// Click an element and don't stop unless it succeeds or the error is
// something other than it being non-interactable.
fn really_click(e: Element) -> impl Future<Item = Client, Error = error::CmdError> {
    future::loop_fn(e, |this| {
        let e1 = this.clone();
        this.click().map(Break).or_else(move |e| {
            if let Standard(WebDriverError {
                error: ElementNotInteractable,
                ..
            }) = e
            {
                Ok(Continue(e1))
            } else {
                Err(e)
            }
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

fn duration_serializer<S: Serializer>(v: &Duration, s: S) -> Result<S::Ok, S::Error> {
    let duration: std::time::Duration = (*v).into();

    if duration.subsec_micros() != 0 {
        panic!("Unexpected fractional seconds");
    }
    let seconds = duration.as_secs();
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let seconds = seconds % 60;
    let minutes = minutes % 60;
    let string = if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    };
    s.serialize_str(&string)
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

// Nom helper

#[allow(clippy::needless_lifetimes)]
fn take_until_and_consume<'a>(
    tag_to_match: &'a str,
) -> impl Fn(&'a str) -> IResult<&'a str, &'a str> {
    let len = tag_to_match.len();

    terminated(take_until(tag_to_match), take(len))
}

// Command line argument processing

#[derive(StructOpt, Debug)]
#[structopt()]
struct Opt {
    /// full, half, relay, 10k, 5k or handcycle
    #[structopt(short = "r", long = "race", default_value = "full")]
    pub race: Race,
    /// 2017, 2018 or 2019
    #[structopt(short = "y", long = "year", default_value = "2019")]
    pub year: Year,
    /// See the webpage as results are gathered
    #[structopt(short = "d", long = "display")]
    pub display: bool,
}

#[derive(Debug)]
enum Race {
    Full,
    Half,
    Relay,
    TenK,
    FiveK,
    Handcycle,
}

impl Race {
    fn menu_item(&self) -> &'static str {
        use Race::*;

        match self {
            Full => "Shiprock Marathon",
            Half => "Shiprock Half Marathon",
            Relay => "Shiprock Marathon Relay",
            TenK => "Shiprock 10k",
            FiveK => "Shiprock 5k",
            Handcycle => "Shiprock Marathon Handcycle",
        }
    }
}

#[derive(Debug)]
struct ParseRaceError;

impl Display for ParseRaceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "choose \"full\", \"half\", \"relay\", \"10k\", \"5k\" or \"handcycle\""
        )
    }
}

impl FromStr for Race {
    type Err = ParseRaceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Race::*;

        match s {
            "full" => Ok(Full),
            "half" => Ok(Half),
            "relay" => Ok(Relay),
            "10k" => Ok(TenK),
            "5k" => Ok(FiveK),
            // FWIW, handcycle was introduced in 2019, so using it with
            // 2017 or 2018 will result in a panic.
            "handcycle" => Ok(Handcycle),
            _ => Err(ParseRaceError),
        }
    }
}

#[derive(Debug)]
enum Year {
    Y2017,
    Y2018,
    Y2019,
}

impl Year {
    fn url(&self) -> String {
        use Year::*;

        format!(
            "https://results.chronotrack.com/event/results/event/event-{}",
            match self {
                Y2017 => "24236",
                Y2018 => "33304",
                Y2019 => "40479",
            }
        )
    }
}

#[derive(Debug)]
struct ParseYearError;

impl Display for ParseYearError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "choose \"2017\", \"2018\", or \"2019\"")
    }
}

impl FromStr for Year {
    type Err = ParseYearError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Year::*;

        match s {
            "2017" => Ok(Y2017),
            "2018" => Ok(Y2018),
            "2019" => Ok(Y2019),
            _ => Err(ParseYearError),
        }
    }
}
