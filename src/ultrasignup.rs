use {
    crate::{duration_serializer, Event, Opt, Scraper},
    anyhow::{bail, Result as AResult},
    async_trait::async_trait,
    digital_duration_nom::duration::Duration,
    fantoccini::{elements::Element, Client, Locator::Css},
    futures::{
        pin_mut,
        stream::{self, StreamExt},
        TryFutureExt, TryStreamExt,
    },
    serde::Serialize,
    std::{
        convert::TryInto,
        num::{NonZeroU8, ParseIntError},
        str::FromStr,
    },
};

pub struct Params {
    did: u32,
    year: String,
    race: Option<String>,
}

impl Params {
    pub fn new(opt: Opt) -> AResult<Self> {
        use {crate::Year::*, Event::*};

        let mut race = None;
        let did = match opt.event {
            Moab240 => 72701,
            JJ100 => {
                if matches!(opt.year, Y2013 | Y2018) {
                    race = Some("100 Miler".to_string());
                }
                74613
            }
            DPTR => {
                match opt.year {
                    Y2013 => race = Some("50 Miler".to_string()),
                    Y2020 => race = Some("53 Miler".to_string()),
                    _ => {}
                }
                74837
            }
            BosqueBigfoot => {
                race = Some("50K".to_string());
                67798
            }
            e => bail!("{:?} not ultrasignup", e),
        };
        Ok(Self {
            did,
            year: opt.year.to_string(),
            race,
        })
    }

    async fn optionally_click_on_race(&self, client: &Client) -> AResult<()> {
        match &self.race {
            None => Ok(()),
            Some(race) => self.find_and_click("a.event_link", race, client).await,
        }
    }

    async fn find_and_click(&self, css: &str, value: &str, client: &Client) -> AResult<()> {
        let link = client
            .find_all(Css(css))
            .map_err(Into::<anyhow::Error>::into)
            .and_then(|v| async move {
                let stream = stream::iter(v.into_iter()).filter_map(|e| async move {
                    match e.text().await {
                        Err(err) => Some(Err(Into::<anyhow::Error>::into(err))),
                        Ok(t) => {
                            if t == value {
                                Some(Ok(e))
                            } else {
                                None
                            }
                        }
                    }
                });
                pin_mut!(stream);
                stream
                    .next()
                    .await
                    .unwrap_or_else(|| bail!("couldn't find {}", value))
            })
            .await?;

        link.click().await?;
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct Placement {
    place: u16,
    first: String,
    last: String,
    city: Option<String>,
    state: Option<String>,
    age: NonZeroU8,
    gender: String,
    gp: u16,
    #[serde(serialize_with = "duration_serializer")]
    time: Duration,
    rank: f32,
}

#[async_trait]
impl Scraper for Params {
    fn url(&self) -> String {
        format!("https://ultrasignup.com/register.aspx?did={}", self.did)
    }

    async fn doit(&self, client: &Client) -> AResult<()> {
        self.find_and_click("a.year_link", &self.year, client)
            .await?;
        self.optionally_click_on_race(client).await?;

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        let placements_or_statuses = client
            .find_all(Css("table#list tbody tr"))
            .map_err(|e| e.into())
            .and_then(|v| async move {
                stream::iter(v.into_iter())
                    .filter_map(
                        |e| async move { PlacementOrStatus::from_element(e).await.transpose() },
                    )
                    .try_collect::<StatusesWithPlacements>()
                    .await
            })
            .await?;
        println!(
            "{}",
            serde_json::to_string(&placements_or_statuses).unwrap()
        );
        Ok(())
    }
}

#[derive(Debug, Serialize)]
enum Status {
    Finishers = 1,
    DidNotFinish = 2,
    DidNotStart = 3,
    Disqualified = 5,
    UnofficialFinish = 6, // or perhaps that should be 4?
}

#[derive(Debug)]
struct UnknownStatus;

impl FromStr for Status {
    type Err = UnknownStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Status::*;

        match s {
            "Finishers" => Ok(Finishers),
            "Did Not Finish" => Ok(DidNotFinish),
            "Did Not Start" => Ok(DidNotStart),
            "Disqualified" => Ok(Disqualified),
            "Unofficial Finish" => Ok(UnofficialFinish),
            _ => Err(UnknownStatus),
        }
    }
}

#[derive(Debug, Serialize)]
struct StatusWithCount {
    status: Status,
    count: u16,
}

#[derive(Debug)]
enum ParseStatusWithCountError {
    WrongNumberOfPieces,
    NotStatus(UnknownStatus),
    BadCount(ParseIntError),
}

impl FromStr for StatusWithCount {
    type Err = ParseStatusWithCountError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use ParseStatusWithCountError::*;

        let pieces = s.split(" - ").collect::<Vec<_>>();
        if pieces.len() != 2 {
            return Err(WrongNumberOfPieces);
        }
        match (pieces[0].parse(), pieces[1].parse()) {
            (Err(e), _) => Err(NotStatus(e)),
            (_, Err(e)) => Err(BadCount(e)),
            (Ok(status), Ok(count)) => Ok(Self { status, count }),
        }
    }
}

#[derive(Debug, Serialize)]
enum PlacementOrStatus {
    StatusWithCount(StatusWithCount),
    Placement(Placement),
}

impl PlacementOrStatus {
    async fn from_element(e: Element) -> AResult<Option<Self>> {
        e.find_all(Css("td"))
            .map_err(Into::<anyhow::Error>::into)
            .and_then(|v| async move {
                stream::iter(v.into_iter())
                    .filter_map(|e| async move { Some(e.text().await) })
                    .try_collect::<Vec<_>>()
                    .await
                    .map_err(Into::<anyhow::Error>::into)
            })
            .and_then(|v| async move {
                if v.len() == 1 {
                    if let Ok(status) = v[0].parse::<StatusWithCount>() {
                        return Ok(Some(PlacementOrStatus::StatusWithCount(status)));
                    } else {
                        bail!("Not a status: {}", v[0]);
                    }
                } else if v.len() < 11 {
                    return Ok(None);
                }
                // clean up a known glitch in the Moab 240 2019 results
                match v[1].parse() {
                    Err(_) => Ok(None),
                    Ok(place) => {
                        let time = if v[9].trim().is_empty() {
                            Duration::new(0, 0)
                        } else {
                            v[9].parse::<Duration>().or_else(|e| {
                                if v[9] == "101:46:3" {
                                    "101:46:03".parse()
                                } else {
                                    Err(e)
                                }
                            })?
                        };
                        let city = optional_string(&v[4]);
                        let state = optional_string(&v[5]);
                        let age = v[6].parse::<u8>()?.try_into()?;
                        let gp = v[8].parse()?;
                        let rank = v[10].parse()?;
                        Ok(Some(PlacementOrStatus::Placement(Placement {
                            place,
                            first: v[2].to_string(),
                            last: v[3].to_string(),
                            city,
                            state,
                            age,
                            gender: v[7].to_string(),
                            gp,
                            time,
                            rank,
                        })))
                    }
                }
            })
            .await
    }
}

fn optional_string(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

#[derive(Debug, Default, Serialize)]
struct StatusesWithPlacements(Vec<(StatusWithCount, Vec<Placement>)>);

impl Extend<PlacementOrStatus> for StatusesWithPlacements {
    fn extend<T: IntoIterator<Item = PlacementOrStatus>>(&mut self, iter: T) {
        use PlacementOrStatus::*;

        for elem in iter {
            match elem {
                Placement(p) => self.0.last_mut().expect("no status").1.push(p),
                StatusWithCount(s) => self.0.push((s, Vec::new())),
            }
        }
    }
}
