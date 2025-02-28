//! Rust Bitcoin coin selection library.
//!
//! This library provides efficient algorithms to compose a set of unspent transaction outputs
//! (UTXOs).

// Coding conventions.
#![deny(non_upper_case_globals)]
#![deny(non_camel_case_types)]
#![deny(non_snake_case)]
#![deny(unused_mut)]
#![deny(missing_docs)]
// Experimental features we need.
#![cfg_attr(docsrs, feature(doc_cfg))]

mod branch_and_bound;
mod single_random_draw;

use bitcoin::{Amount, FeeRate, SignedAmount, Weight};
use rand::thread_rng;

pub use crate::branch_and_bound::select_coins_bnb;
pub use crate::single_random_draw::select_coins_srd;

// https://github.com/bitcoin/bitcoin/blob/f722a9bd132222d9d5cd503b5af25c905b205cdb/src/wallet/coinselection.h#L20
const CHANGE_LOWER: Amount = Amount::from_sat(50_000);

/// Computes the value of an output accounting for the cost to spend it.
///
/// The effective_value can be calculated as: value - (fee_rate * weight).
///
/// Note: the effective value of a `Transaction` may increase less than the effective value of
/// a `TxOut` when adding another `TxOut` to the transaction. This happens when the new
/// `TxOut` added causes the output length `VarInt` to increase its encoding length.
///
/// # Parameters
///
/// * `fee_rate` - the fee rate of the transaction being created.
/// * `weight` - utxo spending conditions weight.
pub(crate) fn effective_value(
    fee_rate: FeeRate,
    weight: Weight,
    value: Amount,
) -> Option<SignedAmount> {
    let signed_input_fee: SignedAmount = fee_rate.fee_wu(weight)?.to_signed().ok()?;
    value.to_signed().ok()?.checked_sub(signed_input_fee)
}

/// Behavior needed for coin-selection.
pub trait WeightedUtxo {
    /// Total UTXO weight.
    fn weight(&self) -> Weight;

    /// The UTXO value.
    fn value(&self) -> Amount;

    /// Computes the effective_value.
    ///
    /// The effective value is calculated as: fee rate * (satisfaction_weight + the base weight).
    fn effective_value(&self, fee_rate: FeeRate) -> Option<SignedAmount> {
        effective_value(fee_rate, self.weight(), self.value())
    }

    /// Computes how wastefull it is to spend this `Utxo`
    ///
    /// The waste is the difference of the fee to spend this `Utxo` now compared with the expected
    /// fee to spend in the future (long_term_fee_rate).
    fn waste(&self, fee_rate: FeeRate, long_term_fee_rate: FeeRate) -> Option<SignedAmount> {
        let fee: SignedAmount = fee_rate.fee_wu(self.weight())?.to_signed().ok()?;
        let lt_fee: SignedAmount = long_term_fee_rate.fee_wu(self.weight())?.to_signed().ok()?;
        fee.checked_sub(lt_fee)
    }
}

/// Select coins first using BnB algorithm similar to what is done in bitcoin
/// core see: <https://github.com/bitcoin/bitcoin/blob/f3bc1a72825fe2b51f4bc20e004cef464f05b965/src/wallet/coinselection.cpp>,
/// and falls back on a random UTXO selection. Returns none if the target cannot
/// be reached with the given utxo pool.
/// Requires compilation with the "rand" feature.
#[cfg(feature = "rand")]
#[cfg_attr(docsrs, doc(cfg(feature = "rand")))]
pub fn select_coins<Utxo: WeightedUtxo>(
    target: Amount,
    cost_of_change: Amount,
    fee_rate: FeeRate,
    long_term_fee_rate: FeeRate,
    weighted_utxos: &[Utxo],
) -> Option<impl Iterator<Item = &Utxo>> {
    let bnb =
        select_coins_bnb(target, cost_of_change, fee_rate, long_term_fee_rate, weighted_utxos);

    if bnb.is_some() {
        bnb
    } else {
        select_coins_srd(target, fee_rate, weighted_utxos, &mut thread_rng())
    }
}

#[cfg(test)]
mod tests {
    use arbitrary::{Arbitrary, Result, Unstructured};
    use arbtest::arbtest;
    use bitcoin::amount::CheckedSum;
    use bitcoin::{Amount, ScriptBuf, TxOut, Weight};

    use super::*;
    use crate::effective_value;

    const MAX_POOL_SIZE: usize = 20;

    pub fn build_pool() -> Vec<Utxo> {
        let amts = [27_336, 238, 9_225, 20_540, 35_590, 49_463, 6_331, 35_548, 50_363, 28_009];

        let utxos: Vec<_> = amts
            .iter()
            .map(|a| {
                let amt = Amount::from_sat(*a);
                let weight = Weight::ZERO;
                build_utxo(amt, weight)
            })
            .collect();

        utxos
    }

    #[derive(Debug, Clone, PartialEq, Ord, Eq, PartialOrd, Arbitrary)]
    pub struct Utxo {
        pub output: TxOut,
        pub weight: Weight,
    }

    pub fn build_utxo(amt: Amount, weight: Weight) -> Utxo {
        let output = TxOut { value: amt, script_pubkey: ScriptBuf::new() };
        Utxo { output, weight }
    }

    impl<'a> Arbitrary<'a> for UtxoPool {
        fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
            let len = u.arbitrary_len::<u64>()? % MAX_POOL_SIZE;

            let mut pool: Vec<Utxo> = Vec::with_capacity(len);
            for _ in 0..len {
                let utxo = Utxo::arbitrary(u)?;
                pool.push(utxo);
            }

            Ok(UtxoPool { utxos: pool })
        }
    }

    #[derive(Debug)]
    pub struct UtxoPool {
        pub utxos: Vec<Utxo>,
    }

    impl WeightedUtxo for Utxo {
        fn weight(&self) -> Weight { self.weight }
        fn value(&self) -> Amount { self.output.value }
    }

    #[test]
    fn select_coins_no_solution() {
        let target = Amount::from_sat(255432);
        let cost_of_change = Amount::ZERO;
        let fee_rate = FeeRate::ZERO;
        let lt_fee_rate = FeeRate::ZERO;
        let pool = build_pool(); // eff value sum 262643

        let result = select_coins(target, cost_of_change, fee_rate, lt_fee_rate, &pool);

        // This yields no solution because:
        //  * BnB fails because the sum overage is greater than cost_of_change
        //  * SRD fails because the sum is greater the utxo sum + CHANGE_LOWER
        assert!(result.is_none());
    }

    #[test]
    fn select_coins_srd_solution() {
        let target = Amount::from_sat(255432) - CHANGE_LOWER;
        let cost_of_change = Amount::ZERO;
        let fee_rate = FeeRate::ZERO;
        let lt_fee_rate = FeeRate::ZERO;
        let pool = build_pool();

        let result = select_coins(target, cost_of_change, fee_rate, lt_fee_rate, &pool);

        assert!(result.is_some());
        let result: Amount = result.unwrap().map(|u| u.value()).sum();
        assert!(result > target);
    }

    #[test]
    fn select_coins_bnb_solution() {
        let target = Amount::from_sat(255432);
        let fee_rate = FeeRate::ZERO;
        let lt_fee_rate = FeeRate::ZERO;
        let pool = build_pool();

        // set cost_of_change to be the differene
        // between the total pool sum and the target amount
        // plus 1.  This creates an upper bound that the sum
        // of all utxos will fall bellow resulting in a BnB match.
        let cost_of_change = Amount::from_sat(7211);

        let result = select_coins(target, cost_of_change, fee_rate, lt_fee_rate, &pool);

        assert!(result.is_some());
        let result: Amount = result.unwrap().map(|u| u.value()).sum();
        assert!(result > target);
        assert!(result <= target + cost_of_change);
    }

    pub fn build_possible_solutions_srd<'a>(
        pool: &'a UtxoPool,
        fee_rate: FeeRate,
        target: Amount,
        solutions: &mut Vec<Vec<&'a Utxo>>,
    ) {
        let mut gen = exhaustigen::Gen::new();
        while !gen.done() {
            let subset = gen.gen_subset(&pool.utxos).collect::<Vec<_>>();
            let effective_values_sum = subset
                .iter()
                .filter_map(|u| effective_value(fee_rate, u.weight(), u.value()))
                .checked_sum();

            if let Some(s) = effective_values_sum {
                if let Ok(p) = s.to_unsigned() {
                    if p >= target {
                        solutions.push(subset)
                    }
                }
            }
        }
    }

    pub fn build_possible_solutions_bnb<'a>(
        pool: &'a UtxoPool,
        fee_rate: FeeRate,
        target: Amount,
        cost_of_change: Amount,
        solutions: &mut Vec<Vec<&'a Utxo>>,
    ) {
        let mut gen = exhaustigen::Gen::new();
        while !gen.done() {
            let subset = gen.gen_subset(&pool.utxos).collect::<Vec<_>>();
            let effective_values_sum = subset
                .iter()
                .filter_map(|u| effective_value(fee_rate, u.weight(), u.value()))
                .checked_sum();

            if let Some(eff_sum) = effective_values_sum {
                if eff_sum <= SignedAmount::MAX_MONEY {
                    if let Ok(unsigned_sum) = eff_sum.to_unsigned() {
                        if unsigned_sum >= target {
                            if let Some(upper_bound) = target.checked_add(cost_of_change) {
                                if unsigned_sum <= upper_bound {
                                    solutions.push(subset)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn assert_proptest_bnb<'a, T: Iterator<Item = &'a Utxo>>(
        target: Amount,
        cost_of_change: Amount,
        fee_rate: FeeRate,
        pool: UtxoPool,
        result: Option<T>,
    ) {
        let mut bnb_solutions: Vec<Vec<&Utxo>> = Vec::new();
        build_possible_solutions_bnb(&pool, fee_rate, target, cost_of_change, &mut bnb_solutions);

        if let Some(r) = result {
            let utxo_sum: Amount = r
                .map(|u| {
                    effective_value(fee_rate, u.weight(), u.value()).unwrap().to_unsigned().unwrap()
                })
                .sum();

            assert!(utxo_sum >= target);
            assert!(utxo_sum <= target + cost_of_change);
        } else {
            assert!(
                target > Amount::MAX_MONEY || target == Amount::ZERO || bnb_solutions.is_empty()
            );
        }
    }

    pub fn assert_proptest_srd<'a, T: Iterator<Item = &'a Utxo>>(
        target: Amount,
        fee_rate: FeeRate,
        pool: UtxoPool,
        result: Option<T>,
    ) {
        let mut srd_solutions: Vec<Vec<&Utxo>> = Vec::new();
        build_possible_solutions_srd(&pool, fee_rate, target, &mut srd_solutions);

        if let Some(r) = result {
            let utxo_sum: Amount = r
                .map(|u| {
                    effective_value(fee_rate, u.weight(), u.value()).unwrap().to_unsigned().unwrap()
                })
                .sum();

            assert!(utxo_sum >= target);
        } else {
            assert!(
                target > Amount::MAX_MONEY || target == Amount::ZERO || srd_solutions.is_empty()
            );
        }
    }

    pub fn assert_proptest<'a, T: Iterator<Item = &'a Utxo>>(
        target: Amount,
        cost_of_change: Amount,
        fee_rate: FeeRate,
        pool: UtxoPool,
        result: Option<T>,
    ) {
        let mut bnb_solutions: Vec<Vec<&Utxo>> = Vec::new();
        build_possible_solutions_bnb(&pool, fee_rate, target, cost_of_change, &mut bnb_solutions);

        let mut srd_solutions: Vec<Vec<&Utxo>> = Vec::new();
        build_possible_solutions_srd(&pool, fee_rate, target, &mut srd_solutions);

        if let Some(r) = result {
            let utxo_sum: Amount = r
                .map(|u| {
                    effective_value(fee_rate, u.weight(), u.value()).unwrap().to_unsigned().unwrap()
                })
                .sum();

            assert!(utxo_sum >= target);
        } else {
            assert!(
                target > Amount::MAX_MONEY
                    || target == Amount::ZERO
                    || bnb_solutions.is_empty() && srd_solutions.is_empty()
            );
        }
    }

    #[test]
    fn select_coins_proptest() {
        arbtest(|u| {
            let pool = UtxoPool::arbitrary(u)?;
            let target = Amount::arbitrary(u)?;
            let cost_of_change = Amount::arbitrary(u)?;
            let fee_rate = FeeRate::arbitrary(u)?;
            let lt_fee_rate = FeeRate::arbitrary(u)?;

            let utxos = pool.utxos.clone();
            let result = select_coins(target, cost_of_change, fee_rate, lt_fee_rate, &utxos);

            assert_proptest(target, cost_of_change, fee_rate, pool, result);

            Ok(())
        });
    }
}
