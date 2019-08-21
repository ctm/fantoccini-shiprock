# fantoccini-shiprock

A little web scraper that can extract the results of the Shiprock Marathon
races from 2017, 2018 and 2019.  It also knows how to extract results from
a few other races.

### Usage

Install [geckodriver](https://github.com/mozilla/geckodriver) and run it.
Then you have these options:

```
USAGE:
    fantoccini_shiprock [FLAGS] [OPTIONS]

FLAGS:
    -d, --display    See the webpage as results are gathered
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -e, --event <event>    shiprock, rftz or lt100 [default: shiprock]
    -r, --race <race>      full, half, relay, 10k, 5k or handcycle [default: full]
    -y, --year <year>      2017, 2018 or 2019 [default: 2019]
```

### Caveat Emptor

I wrote this code primarily to experiment with
[Fantoccini](https://crates.io/crates/fantoccini), in part because it
would allow me to play with
[Futures](https://crates.io/crates/futures), and
[Tokio](https://crates.io/crates/tokio).  I'm new to all three of
those crates and do not claim that my code follows best practices,
although I haven't yet found better examples.

The Leadville Trail 100 (lt100) support is unfinished.  Athlinks
provides overall ranks to people who appear to have not finished.  I
don't know if that's deliberate or if there was some sort of problem
that may be fixed in the next few days.  If I remember, I'll revisit
this issue in a couple of weeks.

I've decided to make a few of my toy projects publicly available, in
part because doing so makes me nervous and I like to get out of my
comfort zone now and then.

## Public Domain

fantoccini-shiprock has been released into the public domain, per the
[UNLICENSE](UNLICENSE).
