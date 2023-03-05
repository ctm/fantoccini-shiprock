use {
    anyhow::Result as AResult,
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{elements::Element, Client, ClientBuilder},
    nom::{
        bytes::complete::{take, take_until},
        sequence::terminated,
        IResult,
    },
    serde::{ser::Error, Serializer},
    serde_json::value,
    std::{
        fmt::{self, Display, Formatter},
        num::ParseIntError,
        str::FromStr,
    },
    structopt::StructOpt,
};

mod athlinks;
mod chronotrack;
mod ultrasignup;

#[tokio::main]
async fn main() -> AResult<()> {
    use Event::*;

    let opt = Opt::from_args();

    let mut caps = serde_json::map::Map::new();

    let firefox_opts = if opt.display {
        serde_json::json!({ "args": [] })
    } else {
        serde_json::json!({ "args": ["--headless"] })
    };
    caps.insert("moz:firefoxOptions".to_string(), firefox_opts);

    // let mut c = Client::with_capabilities("http://localhost:4444", caps).await?;

    let c = ClientBuilder::native()
        .capabilities(caps)
        .connect("http://localhost:4444")
        .await?;

    let scraper: Box<dyn Scraper + Sync> = match opt.event {
        Shiprock => Box::new(chronotrack::Params::new(opt)?),
        Rftz | Lt100 | DukeCityMarathon | CorralesDitchRun => Box::new(athlinks::Params::new(opt)?),
        Moab240 | JJ100 | DPTR | BosqueBigfoot => Box::new(ultrasignup::Params::new(opt)?),
    };

    let url = scraper.url();

    c.goto(&url).await?;
    c.persist().await?;
    scraper.doit(&c).await?;
    c.close().await?;
    Ok(())
}

#[async_trait]
trait ReallyClickable {
    async fn really_click(&self, client: &Client) -> AResult<()>;
}

#[async_trait]
impl ReallyClickable for Element {
    async fn really_click(&self, client: &Client) -> AResult<()> {
        client
            .execute("arguments[0].click()", vec![value::to_value(self)?])
            .await?;
        Ok(())
    }
}

fn duration_serializer<S: Serializer>(v: &Duration, s: S) -> Result<S::Ok, S::Error> {
    let duration: std::time::Duration = (*v).into();

    if duration.subsec_micros() != 0 {
        return Err(S::Error::custom("Unserializable fractional seconds"));
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

// Nom helper

#[allow(clippy::needless_lifetimes)]
fn take_until_and_consume<'a>(
    tag_to_match: &'a str,
) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    let len = tag_to_match.len();

    terminated(take_until(tag_to_match), take(len))
}

// Command line argument processing

#[derive(StructOpt, Debug)]
#[structopt()]
pub(crate) struct Opt {
    /// shiprock, rftz, lt100 or moab240
    #[structopt(short = "e", long = "event", default_value = "shiprock")]
    pub event: Event,
    /// full, half, relay, 10k, 5k or handcycle
    #[structopt(short = "r", long = "race", default_value = "full")]
    pub race: Race,
    #[structopt(short = "y", long = "year", default_value = "2019")]
    pub year: Year,
    /// See the webpage as results are gathered
    #[structopt(short = "d", long = "display")]
    pub display: bool,
}

#[derive(Debug)]
pub enum Race {
    Full,
    Half,
    Relay,
    TenK,
    FiveK,
    Handcycle,
}

#[derive(Debug)]
pub struct ParseRaceError;

impl Display for ParseRaceError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
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
            "handcycle" => Ok(Handcycle),
            _ => Err(ParseRaceError),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Year(u16);

impl Display for Year {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Year {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

#[derive(Debug)]
pub enum Event {
    Shiprock,
    Rftz,
    Lt100,
    Moab240,
    JJ100,
    DPTR,
    BosqueBigfoot,
    DukeCityMarathon,
    CorralesDitchRun,
}

#[derive(Debug)]
pub struct ParseEventError;

impl Display for ParseEventError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "choose \"shiprock\", \"rftz\", \"lt100\", \"moab240\", \"dptr\", \"bosque\", \"dcm\", or \"ditch\"")
    }
}

impl FromStr for Event {
    type Err = ParseEventError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Event::*;

        match s {
            "shiprock" => Ok(Shiprock),
            "rftz" => Ok(Rftz),
            "lt100" => Ok(Lt100),
            "moab240" => Ok(Moab240),
            "jj100" => Ok(JJ100),
            "dptr" => Ok(DPTR),
            "bosque" => Ok(BosqueBigfoot),
            "dcm" => Ok(DukeCityMarathon),
            "ditch" => Ok(CorralesDitchRun),
            _ => Err(ParseEventError),
        }
    }
}

#[async_trait]
trait Scraper {
    fn url(&self) -> String;
    async fn doit(&self, client: &Client) -> AResult<()>;
}
