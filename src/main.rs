use {
    anyhow::Result as AResult,
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{error::CmdError::Standard, Client, Element},
    nom::{
        bytes::complete::{take, take_until},
        sequence::terminated,
        IResult,
    },
    serde::{ser::Error, Serializer},
    std::{
        fmt::{self, Display, Formatter},
        str::FromStr,
    },
    structopt::StructOpt,
    webdriver::error::{ErrorStatus::ElementNotInteractable, WebDriverError},
};

mod athlinks;
mod chronotrack;
mod runsignup;
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

    let mut c = Client::with_capabilities("http://localhost:4444", caps).await?;

    let scraper: Box<dyn Scraper + Sync> = match opt.event {
        Shiprock => Box::new(chronotrack::Params::new(opt)?),
        Rftz | Lt100 => Box::new(athlinks::Params::new(opt)?),
        MtTaylorQuad => Box::new(runsignup::Params::new(opt)?),
        Moab240 | JJ100 | DPTR | BosqueBigfoot => Box::new(ultrasignup::Params::new(opt)?),
    };

    let url = scraper.url();

    c.goto(&url).await?;
    c.persist().await?;
    c = scraper.doit(c).await?;
    c.close().await?;
    Ok(())
}

#[async_trait]
trait ReallyClickable {
    async fn really_click(self) -> AResult<Client>;
}

#[async_trait]
impl ReallyClickable for Element {
    async fn really_click(self) -> AResult<Client> {
        loop {
            let res = self.clone().click().await;
            if let Err(Standard(WebDriverError {
                error: ElementNotInteractable,
                ..
            })) = res
            {
            } else {
                return res.map_err(|e| e.into());
            }
        }
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
pub struct Opt {
    /// shiprock, rftz, lt100 or moab240
    #[structopt(short = "e", long = "event", default_value = "shiprock")]
    pub event: Event,
    /// full, half, relay, 10k, 5k or handcycle
    #[structopt(short = "r", long = "race", default_value = "full")]
    pub race: Race,
    /// 2017, 2018, 2019 or 2020
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

// This is stupid and should be changed.
#[derive(Clone, Copy, Debug)]
pub enum Year {
    Y2010 = 2010,
    Y2011,
    Y2012,
    Y2013,
    Y2014,
    Y2015,
    Y2016,
    Y2017,
    Y2018,
    Y2019,
    Y2020,
}

impl Display for Year {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        (*self as i32).fmt(f)
    }
}

#[derive(Debug)]
pub struct ParseYearError;

impl Display for ParseYearError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "choose \"2010\" through \"2020\"")
    }
}

impl FromStr for Year {
    type Err = ParseYearError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Year::*;

        match s {
            "2010" => Ok(Y2010),
            "2011" => Ok(Y2011),
            "2012" => Ok(Y2012),
            "2013" => Ok(Y2013),
            "2014" => Ok(Y2014),
            "2015" => Ok(Y2015),
            "2016" => Ok(Y2016),
            "2017" => Ok(Y2017),
            "2018" => Ok(Y2018),
            "2019" => Ok(Y2019),
            "2020" => Ok(Y2020),
            _ => Err(ParseYearError),
        }
    }
}

#[derive(Debug)]
pub enum Event {
    Shiprock,
    Rftz,
    Lt100,
    MtTaylorQuad,
    Moab240,
    JJ100,
    DPTR,
    BosqueBigfoot,
}

#[derive(Debug)]
pub struct ParseEventError;

impl Display for ParseEventError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "choose \"shiprock\", \"rftz\", \"lt100\" or \"moab240\"")
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
            "quad" => Ok(MtTaylorQuad),
            "moab240" => Ok(Moab240),
            "jj100" => Ok(JJ100),
            "dptr" => Ok(DPTR),
            "bosque" => Ok(BosqueBigfoot),
            _ => Err(ParseEventError),
        }
    }
}

#[async_trait]
trait Scraper {
    fn url(&self) -> String;
    async fn doit(&self, client: Client) -> AResult<Client>;
}
