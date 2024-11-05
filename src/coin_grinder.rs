// SPDX-License-Identifier: CC0-1.0
//
//! Coin Grinder.
//!
//! This module introduces the Coin Grinder selection Algorithm
//!
/// Coin Grinder is a DFS-based selection Algorithm which optimises for transaction weight.
///
/// # Parameters
///
/// * target: Target spend `Amount`
/// * change_target: A bound on the `Amount` to increase target by with which to create a change
/// output.
/// * max_selection_weight: Maximum allowable selection weight. 
/// * weighted_utxos: The candidate Weighted UTXOs from which to choose a selection from

use crate::WeightedUtxo;
use bitcoin::Amount;
use bitcoin::FeeRate;
use bitcoin::SignedAmount;

pub fn coin_grinder<Utxo: WeightedUtxo>(
    target: Amount,
    cost_of_change: Amount,
    fee_rate: FeeRate,
    long_term_fee_rate: FeeRate,
    weighted_utxos: &[Utxo],
) -> Option<std::vec::IntoIter<&Utxo>> {

    // Creates a tuple of (effective_value, weighted_utxo)
    let mut w_utxos: Vec<(Amount, &Utxo)> = weighted_utxos
        .iter()
        // calculate effective_value and waste for each w_utxo.
        .map(|wu| (wu.effective_value(fee_rate), wu))
        // remove utxos that either had an error in the effective_value or waste calculation.
        .filter(|(eff_val, _)| eff_val.is_some())
        // unwrap the option type since we know they are not None (see previous step).
        .map(|(eff_val, wu)| (eff_val.unwrap(), wu))
        // filter out all effective_values that are negative.
        .filter(|(eff_val, _)| eff_val.is_positive())
        // all utxo effective_values are now positive (see previous step) - cast to unsigned.
        .map(|(eff_val, wu)| (eff_val.to_unsigned().unwrap(), wu))
        .collect();

    // decending sort by effective_value using weight as tie breaker.
    w_utxos.sort_by(|a, b| { 
        b.0.cmp(&a.0)
            .then(b.1.satisfaction_weight().cmp(&a.1.satisfaction_weight()))
    });

    None
}
