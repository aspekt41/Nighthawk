//! The player's body: the breath meter (decision D33) and the warmth
//! meter (decision D34).
//!
//! Each meter is a whole number from 0 to "full", and moves once per
//! simulated second. Breath follows the oxygen in the air the player is
//! standing in; warmth follows the air's temperature. Either reaching 0
//! means the player blacks out.
//!
//! These are pure functions of a meter and the air, so they're easy to test
//! on their own; [`crate::World`] calls them each second and keeps the
//! meters.

// --- Breath -----------------------------------------------------------------

/// A full breath meter, shown as 100%.
pub const BREATH_FULL: u32 = 1000;
/// Oxygen partial pressure (Pa) below which breath runs down. Normal air
/// has about 21,300 Pa; 14,000 Pa is like being 3 km up a mountain, where
/// people get breathless but don't pass out.
pub const SAFE_O2_PA: u32 = 14_000;
/// How fast breath runs down: each second it falls by the oxygen shortfall
/// (Pa) divided by this. In vacuum that's 28 a second, so a full meter
/// lasts about 36 seconds; in thin air it lasts minutes.
const BREATH_LOSS_DIVISOR: u32 = 500;
/// How much breath comes back each second in good air: full in 20 seconds.
pub(crate) const BREATH_RECOVERY: u32 = 50;
/// The warning levels as the meter falls, highest first.
const BREATH_WARNINGS: [(u32, BreathLevel); 3] = [
    (750, BreathLevel::Short),
    (500, BreathLevel::Struggling),
    (250, BreathLevel::Fading),
];

/// Breath levels the player is warned about as their breath runs down,
/// plus recovering from being short of breath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreathLevel {
    /// Fell to 75%.
    Short,
    /// Fell to 50%.
    Struggling,
    /// Fell to 25%.
    Fading,
    /// Climbed back above 75%.
    Recovered,
}

/// The breath meter after one second breathing air with `o2_pa` pascals of
/// oxygen: up in good air, down by the shortfall in thin air.
pub(crate) fn breath_after(breath: u32, o2_pa: u32) -> u32 {
    if o2_pa >= SAFE_O2_PA {
        (breath + BREATH_RECOVERY).min(BREATH_FULL)
    } else {
        breath.saturating_sub(((SAFE_O2_PA - o2_pa) / BREATH_LOSS_DIVISOR).max(1))
    }
}

/// The warnings for breath going from `before` to `after` in one second.
pub(crate) fn breath_levels(before: u32, after: u32) -> Vec<BreathLevel> {
    levels_crossed(before, after, &BREATH_WARNINGS, BreathLevel::Recovered)
}

// --- Warmth -----------------------------------------------------------------

/// A full warmth meter, shown as 100%. It counts in millionths because it
/// moves slowly: in freezing air it can take hours to run down, which is
/// less than one thousandth a second.
pub const WARMTH_FULL: u32 = 1_000_000;
/// The air temperature (mK) below which warmth runs down: 10 °C. Colder
/// than that, in ordinary clothes, the body loses heat faster than it
/// makes it.
pub const COMFORT_MK: u32 = 283_150;
/// What vacuum feels like, in mK: as cold as 0 °C air. Vacuum doesn't
/// conduct heat, but the body still radiates it away and sweat boils off.
pub const VACUUM_FEELS_LIKE_MK: u32 = 273_150;
/// How fast warmth runs down: each second it falls by how far below
/// [`COMFORT_MK`] the air is (in mK) divided by this. At −20 °C that's 277
/// a second, so a full meter lasts about an hour; at 0 °C about three
/// hours; at −80 °C about twenty minutes.
const WARMTH_LOSS_DIVISOR: u32 = 108;
/// How much warmth comes back each second in comfortable air: full from
/// empty in a little over eight minutes.
pub(crate) const WARMTH_RECOVERY: u32 = 2_000;
/// The warning levels as the meter falls, highest first.
const WARMTH_WARNINGS: [(u32, WarmthLevel); 3] = [
    (750_000, WarmthLevel::Chilled),
    (500_000, WarmthLevel::Numb),
    (250_000, WarmthLevel::Freezing),
];

/// Warmth levels the player is warned about as they get colder, plus
/// warming back up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmthLevel {
    /// Fell to 75%.
    Chilled,
    /// Fell to 50%.
    Numb,
    /// Fell to 25%.
    Freezing,
    /// Climbed back above 75%.
    Recovered,
}

/// The warmth meter after one second in air at `air_mk` millikelvin, or
/// in vacuum (`None`).
pub(crate) fn warmth_after(warmth: u32, air_mk: Option<u32>) -> u32 {
    let feels_like = air_mk.unwrap_or(VACUUM_FEELS_LIKE_MK);
    if feels_like >= COMFORT_MK {
        (warmth + WARMTH_RECOVERY).min(WARMTH_FULL)
    } else {
        warmth.saturating_sub(((COMFORT_MK - feels_like) / WARMTH_LOSS_DIVISOR).max(1))
    }
}

/// The warnings for warmth going from `before` to `after` in one second.
pub(crate) fn warmth_levels(before: u32, after: u32) -> Vec<WarmthLevel> {
    levels_crossed(before, after, &WARMTH_WARNINGS, WarmthLevel::Recovered)
}

// --- Shared -----------------------------------------------------------------

/// What made the player black out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// Breath ran out: not enough oxygen.
    Air,
    /// Warmth ran out: too cold.
    Cold,
}

/// The levels a meter passed going from `before` to `after`: each warning
/// level it fell to, then `recovered` if it climbed back above the first
/// (highest) level.
///
/// It's generic (`<L: Copy>`) so breath and warmth share it, each with its
/// own kind of level. `Copy` lets it hand out copies of the levels in the
/// table.
fn levels_crossed<L: Copy>(before: u32, after: u32, warnings: &[(u32, L)], recovered: L) -> Vec<L> {
    let mut levels: Vec<L> = warnings
        .iter()
        .filter(|&&(at, _)| before > at && after <= at)
        .map(|&(_, level)| level)
        .collect();
    if let Some(&(first, _)) = warnings.first()
        && before <= first
        && after > first
    {
        levels.push(recovered);
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heat::celsius_to_mk;

    /// Seconds for a full warmth meter to run out in air at `air` mK (or
    /// in vacuum, for `None`).
    fn seconds_to_freeze(air: Option<u32>) -> u32 {
        let mut warmth = WARMTH_FULL;
        let mut seconds = 0;
        while warmth > 0 {
            warmth = warmth_after(warmth, air);
            seconds += 1;
        }
        seconds
    }

    #[test]
    fn breath_runs_down_in_thin_air_and_comes_back_in_good_air() {
        assert_eq!(breath_after(1000, SAFE_O2_PA), 1000);
        assert_eq!(breath_after(900, 21_000), 950);
        assert_eq!(breath_after(1000, 0), 972, "vacuum: 28 a second");
        assert_eq!(breath_after(1000, SAFE_O2_PA - 1), 999, "at least 1");
        assert_eq!(breath_after(10, 0), 0, "never below empty");
    }

    #[test]
    fn breath_levels_mark_each_level_once() {
        use BreathLevel::*;
        assert_eq!(breath_levels(1000, 760), vec![]);
        assert_eq!(breath_levels(760, 750), vec![Short]);
        assert_eq!(breath_levels(800, 400), vec![Short, Struggling]);
        assert_eq!(breath_levels(740, 790), vec![Recovered]);
        assert_eq!(breath_levels(800, 1000), vec![], "already above");
    }

    #[test]
    fn warmth_lasts_about_an_hour_at_minus_20() {
        let seconds = seconds_to_freeze(Some(celsius_to_mk(-20)));
        assert!((3500..=3700).contains(&seconds), "{seconds} s");
    }

    #[test]
    fn colder_air_freezes_you_faster() {
        let freezing = seconds_to_freeze(Some(celsius_to_mk(0)));
        let bitter = seconds_to_freeze(Some(celsius_to_mk(-80)));
        assert!(
            (10_000..=12_000).contains(&freezing),
            "{freezing} s at 0 °C"
        );
        assert!((1_100..=1_300).contains(&bitter), "{bitter} s at -80 °C");
    }

    #[test]
    fn vacuum_chills_you_like_freezing_air() {
        let zero_celsius = Some(crate::heat::ZERO_CELSIUS_MK);
        assert_eq!(seconds_to_freeze(None), seconds_to_freeze(zero_celsius));
    }

    #[test]
    fn warmth_comes_back_in_comfortable_air() {
        assert_eq!(warmth_after(0, Some(COMFORT_MK)), WARMTH_RECOVERY);
        assert_eq!(
            warmth_after(WARMTH_FULL - 1, Some(celsius_to_mk(20))),
            WARMTH_FULL
        );
        assert_eq!(warmth_after(500, Some(COMFORT_MK - 1)), 499, "at least 1");
    }

    #[test]
    fn warmth_levels_mark_each_level_once() {
        use WarmthLevel::*;
        assert_eq!(warmth_levels(800_000, 750_000), vec![Chilled]);
        assert_eq!(
            warmth_levels(800_000, 200_000),
            vec![Chilled, Numb, Freezing]
        );
        assert_eq!(warmth_levels(700_000, 760_000), vec![Recovered]);
        assert_eq!(warmth_levels(900_000, 899_000), vec![]);
    }

    #[test]
    fn a_meter_with_no_warning_levels_never_reports_any() {
        let none: [(u32, Cause); 0] = [];
        assert_eq!(levels_crossed(10, 0, &none, Cause::Air), vec![]);
    }
}
