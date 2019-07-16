use {
    crate::{duration_serializer, really_click, take_until_and_consume, Opt, Race, Scraper, Year},
    digital_duration_nom::duration::Duration,
    fantoccini::{error::CmdError, Client, Locator::Css},
    futures::future::{
        self, Future,
        Loop::{Break, Continue},
    },
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

fn click_view_all(c: Client, index: usize) -> impl Future<Item = Client, Error = CmdError> {
    let mut c1 = c.clone();

    c.wait_for_find(Css("div.col-md-3.col-12>button"))
        .and_then(|e| e.click())
        .and_then(|c| c.wait_for_find(Css(".view-all-results")))
        .and_then(move |_| {
            c1.find_all(Css(".view-all-results"))
                .and_then(move |v| really_click(v[index].clone()))
        })
}

fn extract_placements(c: Client) -> impl Future<Item = Client, Error = CmdError> {
    const BUTTON_CSS: &str = "#pager>div>div>button";

    future::loop_fn(c, |mut this| {
        let c1 = this.clone();
        let c2 = this.clone();
        c2.wait_for_find(Css(BUTTON_CSS)).and_then(|_| {
            this.source()
                .and_then(|text| {
                    if let Ok((_, placements)) = placements(&text) {
                        println!("{}", serde_json::to_string(&placements).unwrap());
                    }
                    future::ok(this)
                })
                .and_then(|mut c| c.find_all(Css(BUTTON_CSS)))
                .and_then(|v| {
                    // NOTE: The unwrap here is safe, because we already
                    //       did a wait_for_find on BUTTON_CSS.  However,
                    //       it won't work if we refactor and want to
                    //       support single page results.
                    let mut e = v.last().unwrap().clone();

                    e.html(true).and_then(move |html| {
                        if html == "&gt;" {
                            e.click();
                            Ok(Continue(c1))
                        } else {
                            Ok(Break(c1))
                        }
                    })
                })
        })
    })
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

impl Scraper for Params {
    fn url(&self) -> String {
        self.url.to_string()
    }

    fn doit(
        &self,
    ) -> Box<Fn(Client) -> Box<Future<Item = Client, Error = CmdError> + Send> + Send> {
        let race_index = self.race_index;

        Box::new(move |client| {
            Box::new(click_view_all(client, race_index).and_then(extract_placements))
        })
    }
}
