// SPDX-License-Identifier: CC0-1.0
//
//! Single Random Draw Algorithem.
//!
//! This module introduces the Single Random Draw Coin-Selection Algorithm.

use bitcoin::blockdata::transaction::effective_value;
use bitcoin::{Amount, FeeRate};
use rand::seq::SliceRandom;

use crate::{Return, WeightedUtxo, CHANGE_LOWER};

/// Randomize the input set and select coins until the target is reached.
///
/// # Parameters
///
/// * `target` - target value to send to recipient.  Include the fee to pay for
///   the known parts of the transaction excluding the fee for the inputs.
/// * `fee_rate` - ratio of transaction amount per size.
/// * `weighted_utxos` - Weighted UTXOs from which to sum the target amount.
/// * `rng` - used primarily by tests to make the selection deterministic.
///
/// # Returns
///
/// * `Some((u32, Vec<WeightedUtxo>))` where `Vec<WeightedUtxo>` is empty on no matches found.
///   An empty vec signifies that all possibilities where explored successfully and no match
///   could be found with the given parameters.  The first element of the tuple is a u32 which
///   represents the number of iterations needed to find a solution.
/// * `None` un-expected results OR no match found.  A future implementation may add Error types
///   which will differentiate between an unexpected error and no match found.  Currently, a None
///   type occurs when one or more of the following criteria are met:
///     - Overflow when summing available UTXOs
///     - Not enough potential amount to meet the target
///     - Target Amount is zero (no match possible)
///     - Search was successful yet no match found
pub fn select_coins_srd<'a, R: rand::Rng + ?Sized, Utxo: WeightedUtxo>(
    target: Amount,
    fee_rate: FeeRate,
    weighted_utxos: &'a [Utxo],
    rng: &mut R,
) -> Return<'a, Utxo> {
    if target > Amount::MAX_MONEY {
        return None;
    }

    let mut result: Vec<_> = weighted_utxos.iter().collect();
    let mut origin = result.to_owned();
    origin.shuffle(rng);

    result.clear();

    let threshold = target + CHANGE_LOWER;
    let mut value = Amount::ZERO;

    let mut iteration = 0;
    for w_utxo in origin {
        iteration += 1;
        let utxo_value = w_utxo.value();
        let utxo_weight = w_utxo.satisfaction_weight();
        let effective_value = effective_value(fee_rate, utxo_weight, utxo_value);

        if let Some(e) = effective_value {
            if let Ok(v) = e.to_unsigned() {
                value += v;

                result.push(w_utxo);

                if value >= threshold {
                    return Some((iteration, result));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use arbitrary::Arbitrary;
    use arbtest::arbtest;
    use bitcoin::Amount;
    use rand::rngs::mock::StepRng;

    use super::*;
    use crate::single_random_draw::select_coins_srd;
    use crate::tests::{assert_proptest_srd, assert_ref_eq, parse_fee_rate, UtxoPool};

    #[derive(Debug)]
    pub struct TestSRD<'a> {
        target: &'a str,
        fee_rate: &'a str,
        weighted_utxos: Vec<&'a str>,
    }

    impl TestSRD<'_> {
        fn assert(&self, expected_iterations: u32, expected_inputs_str: Option<&[&str]>) {
            let fee_rate = parse_fee_rate(self.fee_rate);
            let target = Amount::from_str(self.target).unwrap();

            let pool: UtxoPool = UtxoPool::from_str_list(&self.weighted_utxos);
            let result = select_coins_srd(target, fee_rate, &pool.utxos, &mut get_rng());

            if let Some((iterations, inputs)) = result {
                assert_eq!(iterations, expected_iterations);

                let expected: UtxoPool = UtxoPool::from_str_list(expected_inputs_str.unwrap());
                assert_ref_eq(inputs, expected.utxos);
            } else {
                assert!(expected_inputs_str.is_none());
                // Remove this check once iteration count is returned by error
                assert_eq!(0, expected_iterations);
            }
        }
    }

    fn get_rng() -> StepRng {
        // [1, 2]
        // let mut vec: Vec<u32> = (1..3).collect();
        // let mut rng = StepRng::new(0, 0);
        //
        // [2, 1]
        // vec.shuffle(&mut rng);

        // shuffle() will always result in the order described above when a constant
        // is used as the rng.  The first is removed from the beginning and added to
        // the end while the remaining elements keep their order.
        StepRng::new(0, 0)
    }

    fn assert_coin_select(
        target_str: &str,
        expected_iterations: u32,
        expected_inputs_str: &[&str],
    ) {
        TestSRD {
            target: target_str,
            fee_rate: "10 sat/kwu",
            weighted_utxos: vec!["1 cBTC/204 wu", "2 cBTC/204 wu"],
        }
        .assert(expected_iterations, Some(expected_inputs_str));
    }

    #[test]
    fn select_coins_srd_with_solution() { assert_coin_select("1.5 cBTC", 1, &["2 cBTC/204 wu"]); }

    #[test]
    fn select_coins_srd_all_solution() {
        assert_coin_select("2.5 cBTC", 2, &["2 cBTC/204 wu", "1 cBTC/204 wu"]);
    }

    #[test]
    #[should_panic]
    // the target is greater than the sum of available UTXOs.
    // therefore asserting that a selection exists should panic.
    fn select_coins_srd_eleven_invalid_target_should_panic() {
        assert_coin_select("11 cBTC", 8, &["1 cBTC"]);
    }

    #[test]
    #[should_panic]
    fn select_coins_srd_params_invalid_target_should_panic() {
        // the target is greater than the sum of available UTXOs.
        // therefore asserting that a selection exists should panic.
        TestSRD { target: "11 cBTC", fee_rate: "0", weighted_utxos: vec!["1.5 cBTC"] }
            .assert(2, Some(&["1.5 cBTC"]));
    }

    #[test]
    fn select_coins_srd_no_solution() {
        TestSRD { target: "4 cBTC", fee_rate: "0", weighted_utxos: vec!["1 cBTC", "2 cBTC"] }
            .assert(0, None);
    }

    #[test]
    fn select_coins_skip_negative_effective_value() {
        TestSRD {
            target: "1.95 cBTC", // 2 cBTC - CHANGE_LOWER
            fee_rate: "10 sat/kwu",
            weighted_utxos: vec!["1 cBTC", "2 cBTC", "1 sat/204 wu"], // 1 sat @ 204 has negative effective_value
        }
        .assert(3, Some(&["2 cBTC", "1 cBTC"]));
    }

    #[test]
    fn select_coins_srd_fee_rate_error() {
        TestSRD {
            target: "1 cBTC",
            fee_rate: "18446744073709551615 sat/kwu",
            weighted_utxos: vec!["1 cBTC/204 wu", "2 cBTC/204 wu"],
        }
        .assert(0, None);
    }

    #[test]
    fn select_coins_srd_change_output_too_small() {
        TestSRD {
            target: "3 cBTC",
            fee_rate: "10 sat/kwu",
            weighted_utxos: vec!["1 cBTC", "2 cBTC"],
        }
        .assert(0, None);
    }

    #[test]
    fn select_coins_srd_with_high_fee() {
        TestSRD {
            target: "1.99999 cBTC",
            fee_rate: "10 sat/kwu",
            weighted_utxos: vec!["1 cBTC", "2 cBTC"],
        }
        .assert(2, Some(&["2 cBTC", "1 cBTC"]));
    }

    #[test]
    fn select_coins_srd_addition_overflow() {
        TestSRD {
            target: "2 cBTC",
            fee_rate: "10 sat/kwu",
            weighted_utxos: vec!["1 cBTC/18446744073709551615 wu"], // weight= u64::MAX
        }
        .assert(0, None);
    }

    #[test]
    fn select_coins_srd_threshold_overflow() {
        TestSRD {
            target: "18446744073709551615 sat", // u64::MAX
            fee_rate: "10 sat/kwu",
            weighted_utxos: vec!["1 cBTC/18446744073709551615 wu"],
        }
        .assert(0, None);
    }

    #[test]
    fn select_coins_srd_none_effective_value() {
        TestSRD {
            target: ".95 cBTC",
            fee_rate: "0",
            weighted_utxos: vec![
                "1 cBTC",
                "9223372036854775808 sat", //i64::MAX + 1
            ],
        }
        .assert(2, Some(&["1 cBTC"]));
    }

    #[test]
    fn select_srd_match_proptest() {
        arbtest(|u| {
            let pool = UtxoPool::arbitrary(u)?;
            let target = Amount::arbitrary(u)?;
            let fee_rate = FeeRate::arbitrary(u)?;

            let utxos = pool.utxos.clone();
            let result: Option<_> = select_coins_srd(target, fee_rate, &utxos, &mut get_rng());

            assert_proptest_srd(target, fee_rate, pool, result);

            Ok(())
        });
    }
}
