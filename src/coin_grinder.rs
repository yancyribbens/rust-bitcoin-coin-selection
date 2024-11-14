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
/// * fee_rate: Needed to calculate the effective_value.
/// * weighted_utxos: The candidate Weighted UTXOs from which to choose a selection from

use crate::WeightedUtxo;
use bitcoin::Amount;
use bitcoin::FeeRate;
use bitcoin::Weight;
use bitcoin::SignedAmount;
use bitcoin::amount::CheckedSum;

//6.4.3 Highest Priority
//Priority based selection has only one redeeming quality, its optimization of short
//term costs. However, in light of the fees being likely to rise in the longterm, putting
//oﬀ spending of UTXOs actively harms the user interests. Besides this, it causes
//an enormous UTXO pool in all examined scenarios, has the largest in transit ratio
//and only a fraction of the lead in short-term costs carries over to the total cost.
//Highest Priority has by far the largest outlier input sets which likely causes users
//to scratch their head at times even at much fewer transactions than performed in
//these scenarios.


//Please refer to the topic on Delving Bitcoin discussing Gutter Guard/Coingrinder simulation results.

//Adds a coin selection algorithm that minimizes the weight of the input set while creating change.
//Motivations

    //At high feerates, using unnecessary inputs can significantly increase the fees
    //Users are upset when fees are relatively large compared to the amount sent
    //Some users struggle to maintain a sufficient count of UTXOs in their wallet

//Approach

//So far, Bitcoin Core has used a balanced approach to coin selection, where it will generate multiple input set candidates using various coin selection algorithms and pick the least wasteful among their results, but not explicitly minimize the input set weight. Under some circumstances, we do want to minimize the weight of the input set. Sometimes changeless solutions require many or heavy inputs, and there is not always a changeless solution for Branch and Bound to find in the first place. This can cause expensive transactions unnecessarily. Given a wallet with sufficient funds, CoinGrinder will pick the minimal-waste input set for a transaction with a change output. The current implementation only runs CoinGrinder at feerates over 3×long-term-feerate-estimate (by default 30 ṩ/vB), which may be a decent compromise between our goal to reduce costs for the users, but still permit transactions at lower feerates to naturally reduce the wallet’s UTXO pool to curb bloat.
//Trade-offs

//Simulations for my thesis on coin selection (see Section 6.3.2.1 [PDF]) suggest that minimizing the input set for all transactions tends to grind a wallet’s UTXO pool to dust (pun intended): an approach selecting inputs per coin-age-priority (in effect similar to “largest first selection”) on average produced a UTXO pool with 15× the UTXO count as Bitcoin Core’s Knapsack-based Coin Selection then (in 2016). Therefore, I do not recommend running CoinGrinder under all circumstances, but only at extreme feerates or when we have another good reason to minimize the input set for other reasons. In the long-term, we should introduce additional metrics to score different input set candidates, e.g. on basis of their privacy and wallet health impact, to pick from all our coin selection results, but until then, we may want to limit use of CoinGrinder in other ways.

use bitcoin::transaction::effective_value;
pub fn coin_grinder<Utxo: WeightedUtxo>(
    target: Amount,
    change_target: Amount,
    max_selection_weight: Weight,
    fee_rate: FeeRate,
    weighted_utxos: &[Utxo],
) -> Option<std::vec::IntoIter<&Utxo>> {

    // Creates a tuple of (effective_value, weighted_utxo)
    let mut w_utxos: Vec<(Amount, &Utxo)> = weighted_utxos
        .iter()
        // calculate effective_value and waste for each w_utxo.
        .map(|wu| (wu.effective_value(fee_rate), wu))
        // remove utxos that either had an error in the effective_value calculation.
        .filter(|(eff_val, _)| eff_val.is_some())
        // unwrap the option type since we know they are not None (see previous step).
        .map(|(eff_val, wu)| (eff_val.unwrap(), wu))
        // filter out all effective_values that are negative.
        .filter(|(eff_val, _)| eff_val.is_positive())
        // all utxo effective_values are now positive (see previous step) - cast to unsigned.
        .map(|(eff_val, wu)| (eff_val.to_unsigned().unwrap(), wu))
        .collect();

    let available_value = w_utxos.clone().into_iter().map(|(ev, _)| ev).checked_sum()?;

    // decending sort by effective_value using weight as tie breaker.
    w_utxos.sort_by(|a, b| { 
        b.0.cmp(&a.0)
            .then(b.1.satisfaction_weight().cmp(&a.1.satisfaction_weight()))
    });

    let lookahead = w_utxos.clone();
    let lookahead: Vec<Amount> = lookahead.iter().map(|(e, w)| e).scan(available_value, |state, &u| {
        *state = *state - u;
        Some(*state)
    }).collect();

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use crate::tests::{build_utxo, Utxo};
    use crate::coin_grinder::coin_grinder;

    #[test]
    fn coin_grinder_insufficient_funds() {
        // Insufficient funds, select all provided coins and fail
        let target = Amount::from_str("1 cBTC").unwrap();
        let max_weight = Weight::from_wu(10_000);
        let change_target = Amount::from_str("100 uBTC").unwrap(); //10k sats
        println!("chage_target {:?}", change_target.to_sat());
        let fee_rate = FeeRate::ZERO;

        let mut pool = Vec::new();
        for i in 0..10 {
            let one_cbtc = build_utxo(Amount::from_str("1 cBTC").unwrap(), Weight::ZERO);
            let two_cbtc = build_utxo(Amount::from_str("2 cBTC").unwrap(), Weight::ZERO);
            pool.push(one_cbtc);
            pool.push(two_cbtc);
        }

        let c = coin_grinder(target, change_target, max_weight, fee_rate, &pool);
    }

    #[test]
    fn coin_grinder_solution_with_mixed_weights() {
        // This test case mirrors that of Bitcoin Cores:
        // https://github.com/bitcoin/bitcoin/blob/8d340be92470f3fd37f2ef4e709d1040bb2a84cf/src/wallet/test/coinselector_tests.cpp#L1213
        //
        // A note on converstion.  In Bitcoin core, the fee_rate is 5,000k while in rust-bitcoin,
        // the equivalent is FeeRate::from_sat_per_vb(5).unwrap() because bitcoin-core uses sat/vB
        // whilerust-bitcoin FeeRate module defaults to sat/kwu
        //
        // Also, in the core tests, a weight of 150 is equal to Weight::from_vb_unwrap(110).  The
        // math is:
        // 110 * segwit multiplyer + input_base_weight = 
        // 110 * 4 + 160 =
        // 150
        let target = Amount::from_str("30 BTC").unwrap();
        let max_weight = Weight::from_wu(400_000);
        let change_target = Amount::from_str("1 BTC").unwrap();

        let fee_rate = FeeRate::from_sat_per_vb(5).unwrap();

        let heavy_coins = vec![3, 6, 9, 12, 15];
        let medium_coins = vec![2, 5, 8, 11, 14];
        let light_coins = vec![1, 4, 7, 10, 13];

        let init = build_utxo(Amount::from_str("3 BTC").unwrap(), Weight::from_vb_unwrap(110));

        let mut pool = vec![init];
        let mut heavy_utxos: Vec<Utxo> = heavy_coins
            .iter()
            .map(|a| {
                let amt_str = format!("{} BTC", a);
                let amt = Amount::from_str(&amt_str).unwrap();
                let weight = Weight::from_vb_unwrap(310);
                build_utxo(amt, weight)
            })
            .collect();

        let mut medium_utxos: Vec<Utxo> = medium_coins
            .iter()
            .map(|a| {
                let amt_str = format!("{} BTC", a);
                let amt = Amount::from_str(&amt_str).unwrap();
                let weight = Weight::from_vb_unwrap(210);
                build_utxo(amt, weight)
            })
            .collect();

        let mut light_utxos: Vec<Utxo> = light_coins
            .iter()
            .map(|a| {
                let amt_str = format!("{} BTC", a);
                let amt = Amount::from_str(&amt_str).unwrap();
                let weight = Weight::from_vb_unwrap(110);
                build_utxo(amt, weight)
            })
            .collect();

        pool.append(&mut heavy_utxos);
        pool.append(&mut medium_utxos);
        pool.append(&mut light_utxos);
        println!("pool len {}", pool.len());

        let c = coin_grinder(target, change_target, max_weight, fee_rate, &pool);
    }

    //fn select_coins_bnb_one() {
        //assert_coin_select("1 cBTC", &["1 cBTC"]); }
        //let result = coin_grinder( );
    //}
}
