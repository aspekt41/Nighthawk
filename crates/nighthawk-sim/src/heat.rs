//! Heat: how warm the air in each tile is (decision D34).
//!
//! Every tile with air in it stores its *heat* as a whole number: the
//! tile's air pressure in pascals times its temperature in millikelvin
//! (thousandths of a degree above absolute zero). Air at a given
//! temperature holds heat in proportion to how much of it there is, so
//! this is a measure of energy, and storing it (rather than a temperature)
//! means heat is only ever *moved*, never created or destroyed by
//! rounding: the same trick as the air (D28). A tile's temperature is
//! worked out when needed: heat ÷ pressure.
//!
//! Each second, heat moves in three ways (see [`crate::Map::spread_heat`]
//! and [`carry`]):
//! - **with moving air**: when air rushes from one tile into another, it
//!   takes its warmth with it;
//! - **between neighbours**: warm air and cold air side by side mix and
//!   even out, slowly, over minutes rather than the single second air
//!   pressure takes;
//! - **out through the hull**: tiles beside the outer hull lose heat to
//!   deep space, about a degree an hour in a big room at room temperature.
//!
//! Vacuum has no temperature at all: no air, no heat.

/// 0 °C in millikelvin. Temperatures are stored above absolute zero so
/// they're never negative.
pub const ZERO_CELSIUS_MK: u32 = 273_150;

/// The temperature decks start at unless their map says otherwise: 20 °C.
pub const ROOM_TEMPERATURE_MK: u32 = 293_150;

/// Deep space, which the hull loses heat to: about 3 K (−270 °C).
pub const SPACE_TEMPERATURE_MK: u32 = 3_000;

/// How fast neighbouring tiles even out, in thousandths of their
/// temperature difference per second. At 100 (10%), two tiles side by side
/// settle in tens of seconds and a 10 m room in a few minutes: slow
/// enough to watch cold creep out of an opened room.
pub const MIXING_PER_MILLE: u64 = 100;

/// How fast a tile loses heat through each face of outer hull beside it,
/// in billionths of its heat above deep space per second. A big room like
/// the stasis bay, with about a quarter of its tiles against the hull,
/// cools by roughly 1 °C an hour from room temperature. (The real ship's
/// walls, insulation and furniture would hold heat too; this rate stands
/// in for all of that.)
pub const HULL_LOSS_PER_BILLION: u64 = 4_200;

/// The coldest temperature a map may ask for, in °C: just above absolute
/// zero, and no colder than deep space.
pub const MIN_CELSIUS: i32 = -270;

/// The hottest temperature a map may ask for, in °C.
pub const MAX_CELSIUS: i32 = 200;

/// Converts whole degrees Celsius to millikelvin.
///
/// It's a `const fn`, so it can be worked out while compiling: that lets
/// tables of temperatures be written in °C, like the heat overlay's bands.
/// (`const fn`s can't use `From` or `TryFrom` yet, hence the `as` casts,
/// which are safe here because of the range checks.)
///
/// # Panics
/// Below absolute zero, or above about 4 million °C. Map files are checked
/// against [`MIN_CELSIUS`] and [`MAX_CELSIUS`] before this is called.
pub const fn celsius_to_mk(celsius: i32) -> u32 {
    let mk = celsius as i64 * 1000 + ZERO_CELSIUS_MK as i64;
    assert!(mk >= 0, "temperature is above absolute zero");
    assert!(mk <= u32::MAX as i64, "temperature fits in u32");
    mk as u32
}

/// Converts millikelvin to tenths of a degree Celsius, rounded to the
/// nearest tenth, for showing: 293_150 → 200 (20.0 °C).
pub fn mk_to_tenths_celsius(mk: u32) -> i32 {
    let above_zero = i64::from(mk) - i64::from(ZERO_CELSIUS_MK);
    // Round half away from zero, the way people round temperatures.
    let tenths = (above_zero + 50 * above_zero.signum()) / 100;
    i32::try_from(tenths).expect("a tile temperature fits in i32")
}

/// The temperature (mK) of air holding `heat` at `pressure` pascals, or
/// `None` for vacuum.
pub fn temperature(heat: u64, pressure: u64) -> Option<u32> {
    let mk = heat.checked_div(pressure)?;
    Some(u32::try_from(mk).expect("a tile temperature fits in u32"))
}

/// Moves heat along with air, after one second of air evening out in a
/// connected `space` (indexes into `heat`). `before` and `after` are the
/// pressure of each of those tiles before and after, in the same order.
///
/// Tiles that lost air give up the same share of their heat, so the air
/// that leaves takes its temperature with it. Some of that air went to
/// space through hull damage, and takes its share of the heat. The rest
/// is shared out to the tiles that gained air, in proportion to how much
/// each gained. Returns the heat lost to space.
///
/// Like the air itself, every unit of heat is accounted for: what's
/// left over after dividing goes one unit each to the first gaining tiles.
pub(crate) fn carry(heat: &mut [u64], space: &[usize], before: &[u64], after: &[u64]) -> u64 {
    let (pool_heat, pool_air) = take_outflow(heat, space, before, after);
    let vented_air = before.iter().sum::<u64>() - after.iter().sum::<u64>();
    // Everything that left a tile either reached another tile or went to
    // space, so `vented_air` is at most `pool_air`.
    let lost = share(pool_heat, vented_air, pool_air);
    give_inflow(
        heat,
        space,
        before,
        after,
        (pool_heat - lost, pool_air - vented_air),
    );
    lost
}

/// `part` / `whole` of `amount`, rounded down; nothing if `whole` is 0.
/// Worked in 128 bits because heat × pressure overflows 64.
fn share(amount: u64, part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    let exact = u128::from(amount) * u128::from(part) / u128::from(whole);
    u64::try_from(exact).expect("a share is never more than the amount")
}

/// Takes the heat that leaves with the air from every tile that lost air.
/// Returns the heat taken and the air (Pa) it left with.
fn take_outflow(heat: &mut [u64], space: &[usize], before: &[u64], after: &[u64]) -> (u64, u64) {
    let mut pool = (0, 0);
    for (k, &i) in space.iter().enumerate() {
        if after[k] < before[k] {
            let left = before[k] - after[k];
            // A tile left with no air keeps no heat at all.
            let given = if after[k] == 0 {
                heat[i]
            } else {
                share(heat[i], left, before[k])
            };
            heat[i] -= given;
            pool = (pool.0 + given, pool.1 + left);
        }
    }
    pool
}

/// Shares `pool` (heat, and the air carrying it) among the tiles that
/// gained air, in proportion to their gain, with the leftover units one
/// each to the first of them.
fn give_inflow(heat: &mut [u64], space: &[usize], before: &[u64], after: &[u64], pool: (u64, u64)) {
    let (pool_heat, pool_air) = pool;
    let gainers: Vec<(usize, u64)> = space
        .iter()
        .enumerate()
        .filter(|&(k, _)| after[k] > before[k])
        .map(|(k, &i)| (i, after[k] - before[k]))
        .collect();
    let mut handed_out = 0;
    for &(i, gained) in &gainers {
        let given = share(pool_heat, gained, pool_air);
        heat[i] += given;
        handed_out += given;
    }
    let leftover = (pool_heat - handed_out) as usize;
    for &(i, _) in gainers.iter().take(leftover) {
        heat[i] += 1;
    }
}

/// Lets neighbouring tiles even out their temperatures a little, for one
/// second. `pressure` is each tile's air pressure, and `links` are the
/// pairs of tiles air can move between (see `Map::air_links`).
///
/// Every flow is worked out from the temperatures at the start of the
/// second, then all are applied, so the order of `links` doesn't matter.
pub(crate) fn exchange(heat: &mut [u64], pressure: &[u64], links: &[(usize, usize)]) {
    let flows: Vec<(usize, usize, u64)> = links
        .iter()
        .filter_map(|&(a, b)| flow(heat, pressure, a, b))
        .collect();
    for (from, to, amount) in flows {
        heat[from] -= amount;
        heat[to] += amount;
    }
}

/// The heat that flows between tiles `a` and `b` in one second, as
/// `(from, to, amount)`, warmer to colder. `None` if either is vacuum or
/// they're the same temperature.
///
/// The flow is [`MIXING_PER_MILLE`] of the temperature difference, times
/// the pair's combined heat capacity `pa × pb / (pa + pb)`. That weight
/// means a thin wisp of air next to a full room takes on the room's
/// temperature, not the other way round. It is also at most the smaller
/// pressure, so a tile can never give away more than 10% of its heat per
/// neighbour.
fn flow(heat: &[u64], pressure: &[u64], a: usize, b: usize) -> Option<(usize, usize, u64)> {
    let (pa, pb) = (pressure[a], pressure[b]);
    let (ta, tb) = (temperature(heat[a], pa)?, temperature(heat[b], pb)?);
    let (from, to) = if ta >= tb { (a, b) } else { (b, a) };
    let difference = u128::from(ta.abs_diff(tb));
    let capacity = u128::from(pa) * u128::from(pb) / u128::from(pa + pb);
    let amount = difference * capacity * u128::from(MIXING_PER_MILLE) / 1000;
    let amount = u64::try_from(amount).expect("a flow is less than a tile's heat");
    (amount > 0).then_some((from, to, amount))
}

/// The heat a tile holding `heat` at `pressure` loses in one second
/// through `hull_faces` faces of outer hull. Air already colder than deep
/// space loses nothing (and never gains: space doesn't warm the ship).
pub(crate) fn hull_loss(heat: u64, pressure: u64, hull_faces: u64) -> u64 {
    let floor = pressure * u64::from(SPACE_TEMPERATURE_MK);
    let above_space = heat.saturating_sub(floor);
    share(
        above_space,
        HULL_LOSS_PER_BILLION * hull_faces,
        1_000_000_000,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Heat for `pressure` pascals of air at `celsius`.
    fn heat_at(pressure: u64, celsius: i32) -> u64 {
        pressure * u64::from(celsius_to_mk(celsius))
    }

    fn celsius_of(heat: u64, pressure: u64) -> i32 {
        mk_to_tenths_celsius(temperature(heat, pressure).unwrap()) / 10
    }

    #[test]
    fn celsius_converts_to_millikelvin_and_back() {
        assert_eq!(celsius_to_mk(0), ZERO_CELSIUS_MK);
        assert_eq!(celsius_to_mk(20), ROOM_TEMPERATURE_MK);
        assert_eq!(celsius_to_mk(-270), 3_150);
        assert_eq!(mk_to_tenths_celsius(ROOM_TEMPERATURE_MK), 200);
        assert_eq!(mk_to_tenths_celsius(celsius_to_mk(-20)), -200);
    }

    #[test]
    fn display_tenths_round_to_the_nearest_either_side_of_zero() {
        assert_eq!(mk_to_tenths_celsius(ZERO_CELSIUS_MK + 49), 0);
        assert_eq!(mk_to_tenths_celsius(ZERO_CELSIUS_MK + 50), 1);
        assert_eq!(mk_to_tenths_celsius(ZERO_CELSIUS_MK - 50), -1);
        assert_eq!(mk_to_tenths_celsius(ZERO_CELSIUS_MK - 49), 0);
        assert_eq!(mk_to_tenths_celsius(0), -2732, "absolute zero");
    }

    #[test]
    #[should_panic(expected = "above absolute zero")]
    fn nothing_is_colder_than_absolute_zero() {
        celsius_to_mk(-274);
    }

    #[test]
    #[should_panic(expected = "fits in u32")]
    fn absurd_heat_is_refused() {
        celsius_to_mk(i32::MAX);
    }

    #[test]
    fn temperature_is_heat_over_pressure_and_vacuum_has_none() {
        assert_eq!(temperature(heat_at(1000, 20), 1000), Some(293_150));
        assert_eq!(temperature(0, 0), None);
    }

    #[test]
    fn a_still_space_keeps_its_heat_where_it_is() {
        // Same pressure before and after: no air moved, so no heat moves.
        let mut heat = vec![heat_at(100, -20), heat_at(100, 20)];
        let start = heat.clone();
        let lost = carry(&mut heat, &[0, 1], &[100, 100], &[100, 100]);
        assert_eq!((heat, lost), (start, 0));
    }

    #[test]
    fn air_rushing_into_vacuum_takes_its_temperature_with_it() {
        // 200 Pa at -20 °C next to vacuum: it evens out to 100 Pa each,
        // both at -20 °C.
        let mut heat = vec![heat_at(200, -20), 0];
        carry(&mut heat, &[0, 1], &[200, 0], &[100, 100]);
        assert_eq!(heat, vec![heat_at(100, -20), heat_at(100, -20)]);
    }

    #[test]
    fn incoming_air_mixes_with_the_air_already_there() {
        // A full warm room (200 Pa, 20 °C) opens onto a thin cold one
        // (100 Pa, -40 °C). The cold room ends with its own air plus the
        // warm room's: 100 Pa at -40 °C plus 50 Pa at 20 °C is -20 °C. The
        // warm room only lost air, so it's still 20 °C.
        let mut heat = vec![heat_at(200, 20), heat_at(100, -40)];
        carry(&mut heat, &[0, 1], &[200, 100], &[150, 150]);
        assert_eq!(celsius_of(heat[0], 150), 20);
        assert_eq!(celsius_of(heat[1], 150), -20);
    }

    #[test]
    fn carrying_heat_never_creates_or_destroys_any() {
        let mut heat = vec![heat_at(301, 25), heat_at(7, -100), 0, heat_at(13, 3)];
        let total: u64 = heat.iter().sum();
        let lost = carry(
            &mut heat,
            &[0, 1, 2, 3],
            &[301, 7, 0, 13],
            &[81, 80, 80, 80],
        );
        assert_eq!(lost, 0);
        assert_eq!(heat.iter().sum::<u64>(), total);
    }

    #[test]
    fn vented_air_takes_its_share_of_the_heat_to_space() {
        // Two tiles of 100 Pa at 20 °C; half the air vents to space.
        let mut heat = vec![heat_at(100, 20), heat_at(100, 20)];
        let total: u64 = heat.iter().sum();
        let lost = carry(&mut heat, &[0, 1], &[100, 100], &[50, 50]);
        assert_eq!(lost, total / 2);
        assert_eq!(heat.iter().sum::<u64>() + lost, total);
        assert_eq!(celsius_of(heat[0], 50), 20, "venting doesn't cool the rest");
    }

    #[test]
    fn a_fully_vented_space_has_no_heat_left() {
        let mut heat = vec![heat_at(100, 20), heat_at(3, 20)];
        let total: u64 = heat.iter().sum();
        let lost = carry(&mut heat, &[0, 1], &[100, 3], &[0, 0]);
        assert_eq!((heat, lost), (vec![0, 0], total));
    }

    #[test]
    fn leftover_heat_goes_one_unit_each_to_the_first_gainers() {
        // 10 units of heat leaving with 3 Pa, into three tiles gaining 1 Pa
        // each: 3 each, and the leftover unit to the first.
        let mut heat = vec![10, 0, 0, 0];
        carry(&mut heat, &[0, 1, 2, 3], &[3, 0, 0, 0], &[0, 1, 1, 1]);
        assert_eq!(heat, vec![0, 4, 3, 3]);
    }

    #[test]
    fn neighbours_even_out_a_little_each_second() {
        let mut heat = vec![heat_at(100, 20), heat_at(100, -20)];
        let total: u64 = heat.iter().sum();
        exchange(&mut heat, &[100, 100], &[(0, 1)]);
        assert_eq!(heat.iter().sum::<u64>(), total, "moved, not lost");
        // 10% of the 40 K gap times half the pressure moves: each side
        // shifts by 2 K.
        assert_eq!(celsius_of(heat[0], 100), 18);
        assert_eq!(celsius_of(heat[1], 100), -18);
    }

    #[test]
    fn mixing_is_the_same_whichever_way_round_a_link_is_listed() {
        let start = vec![heat_at(100, 20), heat_at(100, -20)];
        let (mut forward, mut backward) = (start.clone(), start);
        exchange(&mut forward, &[100, 100], &[(0, 1)]);
        exchange(&mut backward, &[100, 100], &[(1, 0)]);
        assert_eq!(forward, backward);
    }

    #[test]
    fn a_thin_wisp_takes_on_a_full_rooms_temperature() {
        // 10 Pa of cold air beside 1000 Pa of warm: the wisp warms a lot,
        // the room barely cools.
        let mut heat = vec![heat_at(1000, 20), heat_at(10, -20)];
        for _ in 0..30 {
            exchange(&mut heat, &[1000, 10], &[(0, 1)]);
        }
        assert_eq!(celsius_of(heat[0], 1000), 19);
        assert!(celsius_of(heat[1], 10) > 15, "{}", celsius_of(heat[1], 10));
    }

    #[test]
    fn vacuum_and_equal_temperatures_exchange_nothing() {
        let mut heat = vec![heat_at(100, 20), 0, heat_at(50, 20)];
        let start = heat.clone();
        exchange(&mut heat, &[100, 0, 50], &[(0, 1), (0, 2)]);
        assert_eq!(heat, start);
    }

    #[test]
    fn a_tile_with_many_cold_neighbours_never_runs_out_of_heat() {
        // The worst case: a warm tile linked to five much colder ones.
        let mut heat = vec![heat_at(100, 200), 1, 1, 1, 1, 1];
        let links: Vec<(usize, usize)> = (1..6).map(|n| (0, n)).collect();
        for _ in 0..100 {
            exchange(&mut heat, &[100, 100, 100, 100, 100, 100], &links);
        }
        assert!(heat.iter().all(|&h| h > 0));
    }

    #[test]
    fn the_hull_leaks_about_a_degree_an_hour_from_a_big_room() {
        // One room tile in four against the hull, as in the stasis bay.
        // Mixing keeps the room at one temperature, so follow four tiles as
        // one lump that loses heat through the one hull tile's face.
        let pressure = 4 * 101_325;
        let mut heat = heat_at(pressure, 20);
        for _ in 0..3600 {
            heat -= hull_loss(heat / 4, pressure / 4, 1);
        }
        let tenths = mk_to_tenths_celsius(temperature(heat, pressure).unwrap());
        assert!((185..=192).contains(&tenths), "{tenths} tenths of a degree");
    }

    #[test]
    fn more_hull_loses_more_and_no_hull_loses_nothing() {
        let heat = heat_at(1000, 20);
        assert_eq!(hull_loss(heat, 1000, 0), 0);
        assert!(hull_loss(heat, 1000, 2) > hull_loss(heat, 1000, 1));
    }

    #[test]
    fn air_colder_than_space_loses_nothing_to_it() {
        assert_eq!(hull_loss(1000, 1000, 4), 0, "1 K is below deep space");
        assert_eq!(hull_loss(0, 0, 4), 0, "vacuum");
    }

    #[test]
    fn shares_of_nothing_are_nothing() {
        assert_eq!(share(100, 5, 0), 0);
        assert_eq!(share(u64::MAX, 3, 3), u64::MAX, "no 64-bit overflow");
    }
}
