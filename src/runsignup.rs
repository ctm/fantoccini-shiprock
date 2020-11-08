// Now that I have three of these, I really should refactor.  OTOH, this is
// super low-priority and I just want to get numbers to Ken today.

use {
    crate::{duration_serializer, take_until_and_consume, Opt, Scraper, Year},
    anyhow::{bail, Result as AResult},
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::Client,
    nom::{
        bytes::complete::take_until,
        combinator::{map, map_res},
        multi::many1,
        sequence::{preceded, tuple},
        IResult,
    },
    serde::Serialize,
};

pub struct Params;

impl Params {
    pub fn new(opt: Opt) -> AResult<Self> {
        match opt.year {
            Year::Y2020 => {}
            _ => bail!("We currently only scrape The Quad in 2020"),
        };
        Ok(Self)
    }
}

async fn extract_placements(c: Client) -> AResult<Client> {
    print_placements(c.clone()).await?;
    Ok(c)
}

#[derive(Serialize)]
struct Placement {
    place: String,
    // bib: String,
    name: String,
    // city: String,
    #[serde(serialize_with = "duration_serializer")]
    clock_time: Duration,
    // ...
}

impl Placement {
    // new takes its arguments as a tuple so that it has a single argument and
    // hence can be used as the second argument to map.
    #[allow(clippy::type_complexity)]
    fn new<'a>((place, name, clock_time): (&'a str, &'a str, Duration)) -> Self {
        let place = place.to_string();
        let name = name.replace("<b>", "").replace("</b>", "");
        Self {
            place,
            name,
            clock_time,
        }
    }
}

async fn print_placements(mut c: Client) -> AResult<()> {
    let text = c.source().await?;
    if let Ok((_, placements)) = placements(&text) {
        println!("{}", serde_json::to_string(&placements).unwrap());
    }
    Ok(())
}

fn placements(input: &str) -> IResult<&str, Vec<Placement>> {
    many1(placement)(input)
}

// <tr data-result-url="/Race/Results/84435/IndividualResult/xFHC?resultSetId=189088#U43100135"><td class="place">1</td><td style="text-align: right;" class="bib">77</td><td class="name">Rickey <b>Gates</b></td><td>Boulder</td><td style="text-align: right;" class="time">3:56:44</td><td>Overall Male Soloists</td><td>1</td><td></td><td>1:30:14</td><td>2:06:05</td><td>2:25:42</td><td>2:33:54</td><td>2:50:15</td><td>3:25:59</td></tr>

fn placement(input: &str) -> IResult<&str, Placement> {
    map(
        tuple((
            preceded(
                // place
                tuple((
                    take_until("<tr data-result-url=\""),
                    take_until_and_consume("<td class=\"place\">"),
                )),
                take_until("<"),
            ),
            preceded(
                // name
                take_until_and_consume("<td class=\"name\">"),
                take_until("</td>"),
            ),
            map_res(
                preceded(take_until_and_consume("class=\"time\">"), take_until("<")),
                |time| time.parse(),
            ),
        )),
        Placement::new,
    )(input)
}

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        "https://runsignup.com/Race/Results/84435/#resultSetId-189088;perpage:5000".to_string()
    }

    async fn doit(&self, client: Client) -> AResult<Client> {
        Ok(extract_placements(client).await?)
    }
}
