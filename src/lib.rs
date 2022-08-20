//! # rust-bitcoin-coin-selection
//! Helper functions to select a set of UTXOs from a given UTXO pool to reach
//! a given target amount.
//!

#[cfg(any(test, feature = "rand"))]
use rand::{seq::SliceRandom, thread_rng};
use std::cmp::Reverse;

pub trait Utxo: Clone {
    fn get_value(&self) -> u64;
}

/// Select coins first using BnB algorithm similar to what is done in bitcoin
/// core (see: https://github.com/bitcoin/bitcoin/blob/6b254814c076054eedc4311698d16c8971937814/src/wallet/coinselection.cpp#L21),
/// and falls back on a random UTXO selection. Returns none if the target cannot
/// be reached with the given utxo pool.
/// Requires compilation with the "rand" feature.
#[cfg(any(test, feature = "rand"))]
pub fn select_coins<T: Utxo>(
    target: u64,
    cost_of_change: u64,
    utxo_pool: &mut [T],
) -> Option<Vec<T>> {
    match select_coins_bnb(target, cost_of_change, utxo_pool) {
        Some(res) => Some(res),
        None => select_coins_random(target, utxo_pool),
    }
}

/// Randomly select coins for the given target by shuffling the utxo pool and
/// taking UTXOs until the given target is reached, or returns None if the target
/// cannot be reached with the given utxo pool.
/// Requires compilation with the "rand" feature.
#[cfg(any(test, feature = "rand"))]
pub fn select_coins_random<T: Utxo>(target: u64, utxo_pool: &mut [T]) -> Option<Vec<T>> {
    utxo_pool.shuffle(&mut thread_rng());

    let mut sum = 0;

    let res = utxo_pool
        .iter()
        .take_while(|x| {
            if sum >= target {
                return false;
            }
            sum += x.get_value();
            true
        })
        .cloned()
        .collect();

    if sum >= target {
        return Some(res);
    }

    None
}

/// Select coins using BnB algorithm similar to what is done in bitcoin
/// core (see: https://github.com/bitcoin/bitcoin/blob/6b254814c076054eedc4311698d16c8971937814/src/wallet/coinselection.cpp#L21)
/// Returns None if BnB doesn't find a solution.
pub fn select_coins_bnb<T: Utxo>(
    target: u64,
    cost_of_change: u64,
    utxo_pool: &mut [T],
) -> Option<Vec<T>> {
    let mut coin_selection = Vec::new();
    find_solution(target, cost_of_change, utxo_pool, &mut coin_selection);

    if coin_selection.len() == 0 {
        None
    } else {
        Some(coin_selection)
    }
}

pub fn find_solution<T: Utxo>(
    target: u64,
    cost_of_change: u64,
    utxo_pool: &mut [T],
    coin_selection: &mut Vec<T>,
) {
    let utxo_sum = utxo_pool.iter().fold(0u64, |mut s, u| {
        s += u.get_value();
        s
    });

    if utxo_sum < target {
        return;
    }

    utxo_pool.sort_by_key(|u| Reverse(u.get_value()));

    let mut remainder = utxo_sum;

    let lower_bound = target;
    let upper_bound = cost_of_change + lower_bound;

    let mut curr_sum = 0;
    for utxo in utxo_pool {
        if remainder + curr_sum < lower_bound {
            break;
        }

        let utxo_value = utxo.get_value();

        curr_sum += utxo_value;
        coin_selection.push(utxo.clone());

        if curr_sum == lower_bound {
            return;
        }

        if curr_sum > lower_bound {
            if curr_sum > upper_bound {
                coin_selection.pop();
                curr_sum -= utxo_value;
            }
        }

        remainder -= utxo_value;
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    const ONE_BTC: u64 = 100_000_000;
    const TWO_BTC: u64 = 2 * ONE_BTC;
    const THREE_BTC: u64 = 3 * ONE_BTC;
    const FOUR_BTC: u64 = 4 * ONE_BTC;

    const UTXO_POOL: [MinimalUtxo; 4] = [
        MinimalUtxo { value: ONE_BTC },
        MinimalUtxo { value: TWO_BTC },
        MinimalUtxo { value: THREE_BTC },
        MinimalUtxo { value: FOUR_BTC },
    ];

    const COST_OF_CHANGE: u64 = 50_000_000;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct MinimalUtxo {
        value: u64,
    }

    impl Utxo for MinimalUtxo {
        fn get_value(&self) -> u64 {
            self.value
        }
    }

    #[test]
    fn find_solution_1_btc() {
        let utxo_match = select_coins_bnb(ONE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(1, utxo_match.len());
        assert_eq!(ONE_BTC, utxo_match[0].get_value());
    }

    #[test]
    fn find_solution_2_btc() {
        let utxo_match = select_coins_bnb(TWO_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(1, utxo_match.len());
        assert_eq!(TWO_BTC, utxo_match[0].get_value());
    }

    #[test]
    fn find_solution_3_btc() {
        let utxo_match =
            select_coins_bnb(THREE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(1, utxo_match.len());
        assert_eq!(THREE_BTC, utxo_match[0].get_value());
    }

    #[test]
    fn find_solution_4_btc() {
        let utxo_match =
            select_coins_bnb(FOUR_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(1, utxo_match.len());
        assert_eq!(FOUR_BTC, utxo_match[0].get_value());
    }

    #[test]
    fn find_solution_5_btc() {
        let utxo_match =
            select_coins_bnb(5 * ONE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(2, utxo_match.len());
        assert_eq!(FOUR_BTC, utxo_match[0].get_value());
        assert_eq!(ONE_BTC, utxo_match[1].get_value());
    }

    #[test]
    fn find_solution_6_btc() {
        let utxo_match =
            select_coins_bnb(6 * ONE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(2, utxo_match.len());
        assert_eq!(FOUR_BTC, utxo_match[0].get_value());
        assert_eq!(TWO_BTC, utxo_match[1].get_value());
    }

    #[test]
    fn find_solution_7_btc() {
        let utxo_match =
            select_coins_bnb(7 * ONE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(2, utxo_match.len());
        assert_eq!(FOUR_BTC, utxo_match[0].get_value());
        assert_eq!(THREE_BTC, utxo_match[1].get_value());
    }

    #[test]
    fn find_solution_8_btc() {
        let utxo_match =
            select_coins_bnb(8 * ONE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(3, utxo_match.len());

        assert_eq!(FOUR_BTC, utxo_match[0].get_value());
        assert_eq!(THREE_BTC, utxo_match[1].get_value());
        assert_eq!(ONE_BTC, utxo_match[2].get_value());
    }

    #[test]
    fn find_solution_9_btc() {
        let utxo_match =
            select_coins_bnb(9 * ONE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(3, utxo_match.len());
        assert_eq!(FOUR_BTC, utxo_match[0].get_value());
        assert_eq!(THREE_BTC, utxo_match[1].get_value());
        assert_eq!(TWO_BTC, utxo_match[2].get_value());
    }

    #[test]
    fn find_solution_10_btc() {
        let utxo_match =
            select_coins_bnb(10 * ONE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(4, utxo_match.len());
        assert_eq!(FOUR_BTC, utxo_match[0].get_value());
        assert_eq!(THREE_BTC, utxo_match[1].get_value());
        assert_eq!(TWO_BTC, utxo_match[2].get_value());
        assert_eq!(ONE_BTC, utxo_match[3].get_value());
    }

    #[test]
    fn find_solution_11_btc_not_possible() {
        let utxo_match = select_coins_bnb(11 * ONE_BTC, COST_OF_CHANGE, &mut UTXO_POOL.clone());
        assert_eq!(None, utxo_match);
    }

    #[test]
    fn find_solution_with_large_cost_of_change() {
        let utxo_match =
            select_coins_bnb(ONE_BTC * 9 / 10, COST_OF_CHANGE, &mut UTXO_POOL.clone()).unwrap();

        assert_eq!(1, utxo_match.len());
        assert_eq!(ONE_BTC, utxo_match[0].get_value());
    }

    #[test]
    fn select_coins_no_cost_of_change_and_no_match() {
        let utxo_match = select_coins_bnb(ONE_BTC * 9 / 10, 0, &mut UTXO_POOL.clone());
        assert_eq!(None, utxo_match);
    }

    #[test]
    fn select_coins_with_no_match_too_large() {
        let utxo_match = select_coins_bnb(ONE_BTC + 1, COST_OF_CHANGE, &mut UTXO_POOL.clone());
        assert_eq!(None, utxo_match);
    }

    #[test]
    fn select_coins_with_no_match_too_small() {
        let utxo_match = select_coins_bnb(1, COST_OF_CHANGE, &mut UTXO_POOL.clone());
        assert_eq!(None, utxo_match);
    }

    #[test]
    fn select_coins_random_draw_with_solution() {
        let utxo_match = select_coins_random(ONE_BTC, &mut UTXO_POOL.clone());
        utxo_match.expect("Did not properly randomly select coins");
    }

    #[test]
    fn select_coins_random_draw_no_solution() {
        let utxo_match = select_coins_random(11 * ONE_BTC, &mut UTXO_POOL.clone());
        assert!(utxo_match.is_none());
    }

    #[test]
    fn select_coins_bnb_match_with_random() {
        let utxo_match = select_coins(1, COST_OF_CHANGE, &mut UTXO_POOL.clone());
        utxo_match.expect("Did not use random selection");
    }

    #[test]
    fn select_coins_bnb_that_requires_backtrack() {
        let mut utxo_pool: [MinimalUtxo; 3] = [
            MinimalUtxo { value: TWO_BTC + 1 },
            MinimalUtxo { value: TWO_BTC },
            MinimalUtxo { value: TWO_BTC - 1},
        ];

        let utxo_match = select_coins_bnb(TWO_BTC + TWO_BTC - 1, 0, &mut utxo_pool).unwrap();

        // The most optiomal solution is selectiong the last two utxos, which requires
        // the first utxo to be discarded.
        assert_eq!(2, utxo_match.len());
        assert_eq!(TWO_BTC, utxo_match[0].get_value());
        assert_eq!(TWO_BTC - 1, utxo_match[1].get_value());
    }

    #[test]
    fn select_coins_from_large_utxo_pool() {
        let utxo_range = ONE_BTC..ONE_BTC + 100_000;
        let mut utxo_pool: Vec<_> = utxo_range.map(|v| MinimalUtxo { value: v }).collect();
        let utxo_match = select_coins_bnb(ONE_BTC + 1, 20, &mut utxo_pool).unwrap();
        assert_eq!(1, utxo_match.len());
    }

    #[test]
    fn select_coins_random_test() {
        let mut test_utxo_pool = vec![MinimalUtxo {
            value: 5_000_000_000,
        }];

        let utxo_match =
            select_coins(100_000_358, 20, &mut test_utxo_pool).expect("Did not find match");

        assert_eq!(1, utxo_match.len());
    }

    #[test]
    fn select_coins_random_fail_test() {
        let mut test_utxo_pool = vec![MinimalUtxo {
            value: 5_000_000_000,
        }];

        let utxo_match = select_coins(5_000_000_358, 20, &mut test_utxo_pool);

        assert!(utxo_match.is_none());
    }
}
