// SPDX-License-Identifier: CC0-1.0
//
//! Bitcoin Branch and Bound Coin Selection.
//!
//! This module introduces the Branch and Bound Coin-Selection Algorithm.

use bitcoin::amount::CheckedSum;
use bitcoin::{Amount, FeeRate, SignedAmount};

use crate::{Return, WeightedUtxo};

/// Performs a deterministic depth first branch and bound search for a changeless solution.
///
/// A changeless solution is one that exceeds the target amount and is less than target amount plus
/// cost of creating change.  In other words, a changeless solution is a solution where it is less expensive
/// to discard the excess amount (amount over the target) than it is to create a new output
/// containing the change.
///
/// This algorithm is designed to never panic or overflow.  If a panic or overflow would occur,
/// None is returned.  Also, if no match can be found, None is returned.  The semantics may
/// change in the future to give more information about errors encountered.
///
/// # Parameters
///
/// * target: Target spend `Amount`
/// * cost_of_change: The `Amount` needed to produce a change output
/// * fee_rate: `FeeRate` used to calculate each effective_value output value
/// * long_term_fee_rate: Needed to estimate the future effective_value of an output.
/// * weighted_utxos: The candidate Weighted UTXOs from which to choose a selection from
///
/// # Returns
///
/// * `Some((u32, Vec<WeightedUtxo>))` where `Vec<WeightedUtxo>` is non-empty and where u32 is the
///    iteration count.  The search result succeeded and a match was found.
/// * `None` un-expected results OR no match found.  A future implementation can add Error types
///   which will differentiate between an unexpected error and no match found.  Currently, a None
///   type occurs when one or more of the following criteria are met:
///     - Iteration limit hit
///     - Overflow when summing the UTXO space
///     - Not enough potential amount to meet the target, etc
///     - Target Amount is zero (no match possible)
///     - UTXO space was searched successfully however no match was found
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
//  1) We have already found a solution that matches the target and the next solution has a
//  higher waste score.
//
//  2) It's a high fee environment such that adding more utxos will increase current_waste.
//
// If either 1 or 2 is true, we consider the current search path no longer viable to continue.  In
// such a case, backtrack to start a new search path.
pub fn select_coins_bnb<Utxo: WeightedUtxo>(
    target: Amount,
    cost_of_change: Amount,
    fee_rate: FeeRate,
    long_term_fee_rate: FeeRate,
    weighted_utxos: &[Utxo],
) -> Return<Utxo> {
    // Total_Tries in Core:
    // https://github.com/bitcoin/bitcoin/blob/1d9da8da309d1dbf9aef15eb8dc43b4a2dc3d309/src/wallet/coinselection.cpp#L74
    const ITERATION_LIMIT: u32 = 100_000;

    let mut iteration = 0;
    let mut index = 0;
    let mut backtrack;

    let mut value = Amount::ZERO;

    let mut current_waste: SignedAmount = SignedAmount::ZERO;
    let mut best_waste = SignedAmount::MAX_MONEY;

    let mut index_selection: Vec<usize> = vec![];
    let mut best_selection: Vec<usize> = vec![];

    let upper_bound = target.checked_add(cost_of_change)?;

    // Creates a tuple of (effective_value, waste, weighted_utxo)
    let mut w_utxos: Vec<(Amount, SignedAmount, &Utxo)> = weighted_utxos
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

    if available_value < target || target == Amount::ZERO {
        return None;
    }

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
                best_selection = index_selection.clone();
                best_waste = current_waste;
            }

            current_waste = current_waste.checked_sub(waste)?;
        }
        // * Backtrack
        if backtrack {
            if index_selection.is_empty() {
                return index_to_utxo_list(iteration, best_selection, w_utxos);
            }

            loop {
                index -= 1;

                if index <= *index_selection.last().unwrap() {
                    break;
                }

                let (eff_value, _, _) = w_utxos[index];
                available_value += eff_value;
            }

            assert_eq!(index, *index_selection.last().unwrap());
            let (eff_value, utxo_waste, _) = w_utxos[index];
            current_waste = current_waste.checked_sub(utxo_waste)?;
            value = value.checked_sub(eff_value)?;
            index_selection.pop().unwrap();
        }
        // * Add next node to the inclusion branch.
        else {
            let (eff_value, utxo_waste, _) = w_utxos[index];

            // unchecked sub is used her for performance.
            // The bounds for available_value are at most the sum of utxos
            // and at least zero.
            available_value = available_value.unchecked_sub(eff_value);

            // Check if we can omit the currently selected depending on if the last
            // was omitted.  Therefore, check if index_selection has a previous one.
            if index_selection.is_empty()
                // Check if the previous UTXO was included.
                || index - 1 == *index_selection.last().unwrap()
                // Check if the previous UTXO has the same value has the previous one.
                || w_utxos[index].0 != w_utxos[index - 1].0
            {
                index_selection.push(index);
                current_waste = current_waste.checked_add(utxo_waste)?;

                // unchecked add is used here for performance.  Since the sum of all utxo values
                // did not overflow, then any positive subset of the sum will not overflow.
                value = value.unchecked_add(eff_value);
            }
        }

        // no overflow is possible since the iteration count is bounded.
        index += 1;
        iteration += 1;
    }

    index_to_utxo_list(iteration, best_selection, w_utxos)
}

fn index_to_utxo_list<Utxo: WeightedUtxo>(
    iterations: u32,
    index_list: Vec<usize>,
    wu: Vec<(Amount, SignedAmount, &Utxo)>,
) -> Option<(u32, Vec<&Utxo>)> {
    let mut result: Vec<_> = Vec::new();
    let list = index_list;

    for i in list {
        let wu = wu[i].2;
        result.push(wu);
    }

    if result.is_empty() {
        None
    } else {
        Some((iterations, result))
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;
    use std::iter::{once, zip};

    use arbitrary::{Arbitrary, Unstructured};
    use arbtest::arbtest;
    use bitcoin::transaction::effective_value;
    use bitcoin::{Amount, Weight};

    use super::*;
    use crate::tests::{assert_ref_eq, Utxo, UtxoPool};
    use crate::WeightedUtxo;

    const TX_IN_BASE_WEIGHT: u64 = 160;

    #[derive(Debug)]
    pub struct ParamsStr<'a> {
        target: &'a str,
        cost_of_change: &'a str,
        fee_rate: &'a str,
        lt_fee_rate: &'a str,
        effective_values: Vec<&'a str>,
        weights: Vec<&'a str>
    }

    //#[track_caller]
    //fn assert_coin_select(
        //target_str: &str,
        //expected_iterations: u32,
        //expected_inputs_str: &[&str],
    //) {
        //let p = ParamsStr {
            //target: target_str,
            //cost_of_change: "0",
            //fee_rate: "0",
            //lt_fee_rate: "0",
            //weighted_utxos: vec!["1 cBTC", "2 cBTC", "3 cBTC", "4 cBTC"],
        //};
        //assert_coin_select_params(&p, expected_iterations, Some(expected_inputs_str));
    //}

    // TODO check about adding this to rust-bitcoins from_str for FeeRate
    fn parse_fee_rate(f: &str) -> FeeRate {
        let rate_parts: Vec<_> = f.split(" ").collect();
        let rate = rate_parts[0].parse::<u64>().unwrap();

        match rate_parts.len() {
            1 => {
                assert!(rate == 0);
                FeeRate::ZERO
            }

            2 => match rate_parts[1] {
                "sat/kwu" => FeeRate::from_sat_per_kwu(rate),
                "sat/vb" => FeeRate::from_sat_per_vb(rate).unwrap(),
                "0" => FeeRate::ZERO,
                _ => panic!("only support sat/kwu or sat/vb rates"),
            },

            _ => panic!("number, space then rate not parsed.  example: 10 sat/kwu"),
        }
    }

    #[track_caller]
    fn assert_effective_values_selected(
        p: &ParamsStr,
        expected_iterations: u32,
        expected_inputs_str: Option<&[&str]>,
    ) {
        // Remove this check once iteration count is returned by error
        if expected_inputs_str.is_none() {
            assert_eq!(0, expected_iterations);
        }

        let target = Amount::from_str(p.target).unwrap();
        let cost_of_change = Amount::from_str(p.cost_of_change).unwrap();

        let fee_rate = parse_fee_rate(p.fee_rate);
        let lt_fee_rate = parse_fee_rate(p.lt_fee_rate);

        let pool: UtxoPool = UtxoPool::from_effective_values_with_weights(&p.effective_values, &p.weights, p.fee_rate);
        //let result = select_coins_bnb(target, cost_of_change, fee_rate, lt_fee_rate, &pool.utxos);

        //if let Some((iterations, inputs)) = result {
            //assert_eq!(iterations, expected_iterations);

            //let expected: UtxoPool = UtxoPool::from_str_list(expected_inputs_str.unwrap());
            //assert_ref_eq(inputs, expected.utxos);
        //} else {
            //assert!(expected_inputs_str.is_none());
        //}
    }

    // Use in place of arbitrary_in_range()
    // see: https://github.com/rust-fuzz/arbitrary/pull/192
    fn arb_amount_in_range(u: &mut Unstructured, r: std::ops::RangeInclusive<u64>) -> Amount {
        let u = u.int_in_range::<u64>(r).unwrap();
        Amount::from_sat(u)
    }

    // Use in place of arbitrary_in_range()
    // see: https://github.com/rust-fuzz/arbitrary/pull/192
    fn arb_fee_rate_in_range(u: &mut Unstructured, r: std::ops::RangeInclusive<u64>) -> FeeRate {
        let u = u.int_in_range::<u64>(r).unwrap();
        FeeRate::from_sat_per_kwu(u)
    }

    fn calculate_max_fee_rate(amount: Amount, weight: Weight) -> Option<FeeRate> {
        let weight = weight + Weight::from_wu(TX_IN_BASE_WEIGHT);

        let mut result = None;
        if let Some(fee_rate) = amount.checked_div_by_weight_floor(weight) {
            if fee_rate > FeeRate::ZERO {
                result = Some(fee_rate)
            }
        };

        result
    }

    #[test]
    fn select_coins_bnb_skip_effective_negative_effective_value() {
        let params = ParamsStr {
            target: "1 cBTC",
            cost_of_change: "1 cBTC",
            fee_rate: "10 sat/kwu",
            lt_fee_rate: "10 sat/kwu",
            effective_values: vec!["1.5 cBTC", "-1 sat"],
            weights: vec!["68 vb", "68 vb"]
        };

        assert_effective_values_selected(&params, 2, Some(&["1.5 cBTC"]));
    }
}
