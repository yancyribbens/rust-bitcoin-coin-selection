// SPDX-License-Identifier: CC0-1.0
//
//! Bitcoin Branch and Bound Coin Selection.
//!
//! This module introduces the Branch and Bound Coin Selection Algorithm.

use crate::WeightedUtxo;
use bitcoin::amount::CheckedSum;
use bitcoin::Amount;
use bitcoin::FeeRate;
use bitcoin::SignedAmount;

/// Select coins bnb performs a depth first branch and bound search.  The search traverses a
/// binary tree with a maximum depth n where n is the size of the target UTXO pool.
///
/// See also core: <https://github.com/bitcoin/bitcoin/blob/f3bc1a72825fe2b51f4bc20e004cef464f05b965/src/wallet/coinselection.cpp>
///
/// Returns a vector of `WeightedUtxo` that meet or exceed the target `Amount` when summed.
/// The `Amount` returned will not exceed the target by more than target + delta where delta is
/// the cost of producing a change output.
///
/// The vector returned seeks to minimize the excess, which is the difference between the target
/// `Amount` and vector sum.  If no match can be found, None is returned.
///
/// This algorithem is designed to never panic or overflow.  If a panic or overflow would occur,
/// None is returned.  Also, if no match can be found, None is returned.  The semantics may
/// change in the future to give more information about errors encountered.
///
/// # Returns
/// * `Some(Vec<WeightedUtxo>)` where `Vec<WeightedUtxo>` is not empty on match.
/// * `None` No match found or un-expected results.
///
/// # Arguments
/// * target: Target spend `Amount`
/// * cost_of_change: The `Amount` needed to produce a change output
/// * fee_rate: `FeeRate` used to calculate each effective_value output value
/// * weighted_utxos: The candidate Weighted UTXOs from which to choose a selection from

// This search explores a binary tree.  The left branch of each node is the inclusion branch and
// the right branch is the exclusion branch.
//      o
//     / \
//    I   E
//
// If the UTXO set consist of a list: [4,3,2,1], and the target is 5, the selection process works
// as follows:
//
// Add 4 to the inclusion branch.  The current total is 4 which is less than our target of 5,
// therefore the search routine continues.  The next UTXO 3 is added to the inclusion branch.
//      o
//     /
//    4
//   /
//  3
//
// At this point, the total sums to 7 (4 + 3) exceeding the target of 5.  7 may be recorded as a
// solution with an excess of 2 (7 - 5). 3 is removed from the left branch and it becomes
// the right branch since 3 is now excluded.
//      o
//     /
//    4
//     \
//      3
//
// We next try add 2 to the inclusion branch.
//      o
//     /
//    4
//     \
//      3
//     /
//    2
//
// The sum of the left inclusion branch is now 6 (4 + 2).  Once again, we find the total
// exceeds 5, so we record 6 as a solution with an excess of 1, our best solution so far.
// Once again, we add 2 to the exclusion branch.
//      o
//     /
//    4
//     \
//      3
//       \
//        2
//
// Finally, we add 1 to the inclusion branch.  This ends our depth first search by matching two
// conditions, it is both the leaf node (no more available value) and matches our search criteria of
// 5 with the smallest possible excess (0).  Both 4 and 1 are on the left inclusion branch.
//
//      o
//     / \
//    4
//     \
//      3
//       \
//        2
//       /
//      1
//
// The search continues because it is possible to do better than 0 (more on that later).
// We next try excluding 4 by adding 4 to the exclusion branch, then we begin a new search
// tree by adding 3 to the inclusion branch.
//      o
//       \
//        4
//       /
//      3
//
// 3 is less than our target, so we next add 2 to our inclusion branch.
//      o
//       \
//        4
//       /
//      3
//     /
//    2
//
// We now stop our search again noticing that 3 and 2 equals our target as 5, and since this
// solution was found last, [3, 2] overwrites the previously found solution [4, 1].  We haven't
// tried combinations including 1 at this point, however adding 1 to [3, 2, 1] would be a worse
// solution since it overshoots the target of 5, so the combination is dismissed.  Furthermore,
// removing 2 would not leave enough available value [3, 1] to make it to our target, therefore
// the search routine has exhausted all possibilities using 3 as the root. We next backtrack and
// exclude our root node of this tree 3.  Since our new sub tree starting at 2 doesn't have enough
// value left to meet the target, we conclude our search at [3, 2].
//
// * Addendum on Waste Calculation Optimization *
// Waste, like accumulated value, is a bound used to track when a search path is no longer
// advantageous.  The waste total is accumulated and stored in a variable called current_waste.
// Besides the difference between amount and target, current_waste stores the difference between
// utxo fee and utxo_long_term_fee.
//
// If the iteration adds a new node to the inclusion branch, besides incrementing the accumulated
// value for the node, the waste is also added to the current_waste.  Note that unlike value,
// waste can actually be negative.  This happens if there is a low fee environment such that
// fee is less than long_term_fee.  Therefore, the only case where a solution becomes more
// wasteful, and we may bound our search because a better waste score is no longer possible is:
//
//  1) We have already found a solution that matchs the target and the next solution has a
//  higher waste score.
//
//  2) It's a high fee environment such that adding more utxos will increase current_waste.
//
// If either 1 or 2 is true, we consider the current search path no longer viable to continue.  In
// such a case, backtrack to start a new search path.
pub fn select_coins_bnb(
    target: Amount,
    cost_of_change: Amount,
    fee_rate: FeeRate,
    long_term_fee_rate: FeeRate,
    weighted_utxos: &mut Vec<WeightedUtxo>,
) -> Option<usize> {
    // Total_Tries in Core:
    // https://github.com/bitcoin/bitcoin/blob/1d9da8da309d1dbf9aef15eb8dc43b4a2dc3d309/src/wallet/coinselection.cpp#L74
    const ITERATION_LIMIT: usize = 100_000;

    let mut iteration: usize= 0;
    let mut index = 0;
    let mut backtrack;

    let mut value = Amount::ZERO;

    let mut current_waste: SignedAmount = SignedAmount::ZERO;
    let mut best_waste = SignedAmount::MAX_MONEY;

    let mut index_selection: Vec<(usize, &WeightedUtxo)> = vec![];
    let mut best_selection: Option<Vec<(usize, &WeightedUtxo)>> = None;

    let upper_bound = target.checked_add(cost_of_change)?;

    // Creates a tuple of (effective_value, waste, weighted_utxo)
    let mut w_utxos: Vec<(Amount, SignedAmount, &WeightedUtxo)> = weighted_utxos
        .iter()
        // calculate effective_value and waste for each w_utxo.
        .map(|wu| (wu.effective_value(fee_rate), wu.waste(fee_rate, long_term_fee_rate), wu))
        // remove utxos that either had an error in the effective_value or waste calculation.
        .filter(|(eff_val, waste, _)| eff_val.is_some() && waste.is_some())
        // unwrap the option type since we know they are not None (see previous step).
        .map(|(eff_val, waste, wu)| (eff_val.unwrap(), waste.unwrap(), wu))
        // filter out all effective_values that are negative.
        .filter(|(eff_val, _, _)| eff_val.is_positive())
        // all utxo effective_values are now positive (see previous step) - cast to unsigned.
        .map(|(eff_val, waste, wu)| (eff_val.to_unsigned().unwrap(), waste, wu))
        .collect();

    w_utxos.sort_by_key(|u| u.0);
    w_utxos.reverse();

    let mut available_value = w_utxos.clone().into_iter().map(|(ev, _, _)| ev).checked_sum()?;

    if available_value < target {
        return None;
    }

    weighted_utxos.to_vec().clear();
    while iteration < ITERATION_LIMIT {
        backtrack = false;

        // * If any of the conditions are met, backtrack.
        //
        // unchecked_add is used here for performance.  Before entering the search loop, all
        // utxos are summed and checked for overflow.  Since there was no overflow then, any
        // subset of addition will not overflow.
        if available_value.unchecked_add(value) < target
            // Provides an upper bound on the excess value that is permissible.
            // Since value is lost when we create a change output due to increasing the size of the
            // transaction by an output (the change output), we accept solutions that may be
            // larger than the target.  The excess is added to the solutions waste score.
            // However, values greater than value + cost_of_change are not considered.
            //
            // This creates a range of possible solutions where;
            // range = (target, target + cost_of_change]
            //
            // That is, the range includes solutions that exactly equal the target up to but not
            // including values greater than target + cost_of_change.
            || value > upper_bound
            // if current_waste > best_waste, then backtrack.  However, only backtrack if
            // it's high fee_rate environment.  During low fee environments, a utxo may
            // have negative waste, therefore adding more utxos in such an environment
            // may still result in reduced waste.
            || current_waste > best_waste && fee_rate > long_term_fee_rate
        {
            backtrack = true;
        }
        // * value meets or exceeds the target.
        //   Record the solution and the waste then continue.
        else if value >= target {
            backtrack = true;

            let v = value.to_signed().ok()?;
            let t = target.to_signed().ok()?;
            let waste: SignedAmount = v.checked_sub(t)?;
            current_waste = current_waste.checked_add(waste)?;

            // Check if index_selection is better than the previous known best, and
            // update best_selection accordingly.
            if current_waste <= best_waste {
                best_selection = Some(index_selection.clone());
                best_waste = current_waste;
            }

            current_waste = current_waste.checked_sub(waste)?;
        }
        // * Backtrack
        if backtrack {
            if index_selection.is_empty() {
                //TODO remove unwrap()
                *weighted_utxos = best_selection.unwrap().into_iter().map(|(_, w)| w.clone()).collect();
                return Some(iteration);
            }

            loop {
                index -= 1;

                let (i, _) = index_selection.last().unwrap();
                if index <= *i {
                    break;
                }

                let (eff_value, _, _) = w_utxos[index];
                available_value += eff_value;
            }

            let (i, _) = index_selection.last().unwrap();
            assert_eq!(index, *i);

            let (eff_value, utxo_waste, _) = w_utxos[index];
            current_waste = current_waste.checked_sub(utxo_waste)?;
            value = value.checked_sub(eff_value)?;
            index_selection.pop().unwrap();
        }
        // * Add next node to the inclusion branch.
        else {
            let (eff_value, utxo_waste, w_utxo) = w_utxos[index];
            current_waste = current_waste.checked_add(utxo_waste)?;

            index_selection.push((index, w_utxo));

            // unchecked add is used here for performance.  Since the sum of all utxo values
            // did not overflow, then any positive subset of the sum will not overflow.
            value = value.unchecked_add(eff_value);

            // unchecked sub is used her for performance.
            // The bounds for available_value are at most the sum of utxos
            // and at least zero.
            available_value = available_value.unchecked_sub(eff_value);
        }

        // no overflow is possible since the iteration count is bounded.
        index += 1;
        iteration += 1;
    }

    // TODO remove unwrap()
    *weighted_utxos = best_selection.unwrap().into_iter().map(|(_, w)| w.clone()).collect();
    return Some(iteration);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WeightedUtxo;
    use bitcoin::Amount;
    use bitcoin::ScriptBuf;
    use bitcoin::SignedAmount;
    use bitcoin::TxOut;
    use bitcoin::Weight;
    use core::str::FromStr;
    use std::iter::once;
    use std::iter::zip;

    fn create_weighted_utxos(fee: Amount) -> Vec<WeightedUtxo> {
        let amts = [
            Amount::from_str("1 cBTC").unwrap() + fee,
            Amount::from_str("2 cBTC").unwrap() + fee,
            Amount::from_str("3 cBTC").unwrap() + fee,
            Amount::from_str("4 cBTC").unwrap() + fee,
        ];

        amts.iter()
            .map(|amt| WeightedUtxo {
                satisfaction_weight: Weight::ZERO,
                utxo: TxOut { value: *amt, script_pubkey: ScriptBuf::new() },
            })
            .collect()
    }

    fn create_weighted_utxos_from_values(values: Vec<Amount>) -> Vec<WeightedUtxo> {
        values
            .iter()
            .map(|amt| WeightedUtxo {
                satisfaction_weight: Weight::ZERO,
                utxo: TxOut { value: *amt, script_pubkey: ScriptBuf::new() },
            })
            .collect()
    }

    #[test]
    fn select_coins_bnb_one() {
        let target = Amount::from_str("1 cBTC").unwrap();
        let mut weighted_utxos = create_weighted_utxos(Amount::ZERO);

        let list = select_coins_bnb(target, Amount::ZERO, FeeRate::ZERO, FeeRate::ZERO, &mut weighted_utxos);
        assert_eq!(weighted_utxos.len(), 1);
        //assert_eq!(result.unwrap(), 0);
    }
}
