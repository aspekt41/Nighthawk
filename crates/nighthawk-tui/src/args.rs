//! Command-line options.
//!
//! ```text
//! nighthawk-tui [--seed <number>]
//! ```
//!
//! Parsing is a plain function from a list of strings, so it can be tested
//! without starting the program.

/// What the player asked for on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Replay a specific ship. Without it, a seed is picked from the clock.
    pub seed: Option<u64>,
}

/// The result of reading the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parsed {
    Run(Options),
    Help,
}

pub const USAGE: &str = "\
usage: nighthawk-tui [--seed <number>]

  --seed <number>   start from a specific seed (0 to 18446744073709551615);
                    the same seed and the same moves give the same game
  -h, --help        show this help";

/// Reads the arguments that follow the program name.
pub fn parse_args<I>(args: I) -> Result<Parsed, String>
where
    I: IntoIterator<Item = String>,
{
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        // `split_once` handles the `--seed=42` spelling as well as `--seed 42`.
        let (flag, inline_value) = match arg.split_once('=') {
            Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
            None => (arg, None),
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--seed" => {
                let value = inline_value
                    .or_else(|| args.next())
                    .ok_or("--seed needs a number")?;
                options.seed = Some(parse_seed(&value)?);
            }
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(Parsed::Run(options))
}

fn parse_seed(value: &str) -> Result<u64, String> {
    value.parse().map_err(|_| {
        format!(
            "--seed must be a whole number from 0 to {}, not {value:?}",
            u64::MAX
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Parsed, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_means_run_with_a_random_seed() {
        assert_eq!(parse(&[]), Ok(Parsed::Run(Options { seed: None })));
    }

    #[test]
    fn seed_can_be_given_two_ways() {
        let want = Ok(Parsed::Run(Options { seed: Some(42) }));
        assert_eq!(parse(&["--seed", "42"]), want);
        assert_eq!(parse(&["--seed=42"]), want);
    }

    #[test]
    fn the_largest_seed_is_accepted() {
        let max = u64::MAX.to_string();
        assert_eq!(
            parse(&["--seed", &max]),
            Ok(Parsed::Run(Options {
                seed: Some(u64::MAX)
            }))
        );
    }

    #[test]
    fn help_wins_wherever_it_appears() {
        assert_eq!(parse(&["--help"]), Ok(Parsed::Help));
        assert_eq!(parse(&["--seed", "1", "-h"]), Ok(Parsed::Help));
    }

    #[test]
    fn bad_seeds_are_explained() {
        for bad in ["abc", "-1", "1.5", "18446744073709551616", ""] {
            let err = parse(&["--seed", bad]).unwrap_err();
            assert!(err.contains("whole number"), "{bad:?} gave {err:?}");
        }
    }

    #[test]
    fn seed_without_a_value_is_an_error() {
        assert_eq!(parse(&["--seed"]), Err("--seed needs a number".to_string()));
    }

    #[test]
    fn unknown_options_are_errors() {
        assert!(parse(&["--sed", "1"]).unwrap_err().contains("--sed"));
    }

    #[test]
    fn usage_mentions_every_option() {
        assert!(USAGE.contains("--seed") && USAGE.contains("--help"));
    }
}
