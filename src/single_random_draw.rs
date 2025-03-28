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
///    the known parts of the transaction excluding the fee for the inputs.
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
    use crate::tests::{assert_ref_eq, UtxoPool};

    #[derive(Debug)]
    pub struct ParamsStr<'a> {
        target: &'a str,
        fee_rate: &'a str,
        weighted_utxos: Vec<&'a str>,
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
}
