use {
    anyhow::Result as AResult,
    async_trait::async_trait,
    clap::{Parser, ValueEnum},
    fantoccini::{elements::Element, Client, ClientBuilder},
    nom::{
        bytes::complete::{take, take_until},
        sequence::terminated,
        IResult,
    },
    serde_json::value,
    std::{
        fmt::{self, Display, Formatter},
        num::ParseIntError,
        str::FromStr,
    },
};

mod athlinks;
mod chronotrack;
mod its_your_race;
mod ultrasignup;

#[tokio::main]
async fn main() -> AResult<()> {
    use Event::*;

    let opt = Opt::parse();

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
        Rftz | Lt100 | DukeCityMarathon | CorralesDitchRun | KotH | RioGrande => {
            Box::new(athlinks::Params::new(opt)?)
        }
        Moab240 | JJ100 | DPTR | BosqueBigfoot => Box::new(ultrasignup::Params::new(opt)?),
        BMDM => Box::new(its_your_race::Params::new(opt)?),
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

// Nom helper

#[allow(clippy::needless_lifetimes)]
fn take_until_and_consume<'a>(
    tag_to_match: &'a str,
) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    let len = tag_to_match.len();

    terminated(take_until(tag_to_match), take(len))
}

// Command line argument processing

#[derive(Parser, Debug)]
pub(crate) struct Opt {
    /// shiprock, rftz, lt100 or moab240
    #[arg(short, long, default_value = "shiprock", value_enum)]
    pub event: Event,
    /// full, half, relay, 10k, 5k or handcycle
    #[arg(short, long, default_value = "full", value_enum)]
    pub race: Race,
    #[arg(short, long, default_value = "2019")]
    pub year: Year,
    /// See the webpage as results are gathered
    #[arg(short, long)]
    pub display: bool,
    #[arg(short, long)]
    pub participant: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Race {
    Full,
    Half,
    Relay,
    TenK,
    FiveK,
    Handcycle,
    TenKRuck,
    SoloMaleHeavy, // TEMPORARY HACK
}

#[derive(Debug)]
pub struct ParseRaceError;

impl Display for ParseRaceError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "choose \"full\", \"half\", \"relay\", \"10k\", \"5k\" or \"handcycle\" (or \"solo-male-heavy\")"
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
            "10kruck" => Ok(TenKRuck),
            "solo-male-heavy" => Ok(SoloMaleHeavy),
            _ => Err(ParseRaceError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, ValueEnum)]
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
    KotH,
    BMDM,
    RioGrande,
}

#[derive(Debug)]
pub struct ParseEventError;

impl Display for ParseEventError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "choose \"shiprock\", \"rftz\", \"lt100\", \"moab240\", \"dptr\", \"bosque\", \"dcm\", \"ditch\", \"koth\", \"bmdm\" or \"riogrande\"")
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
            "koth" => Ok(KotH),
            "bmdm" => Ok(BMDM),
            "riogrande" => Ok(RioGrande),
            _ => Err(ParseEventError),
        }
    }
}

#[async_trait]
trait Scraper {
    fn url(&self) -> String;
    async fn doit(&self, client: &Client) -> AResult<()>;
}
