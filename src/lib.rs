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
#![cfg_attr(bench, feature(test))]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(bench)]
extern crate test;

mod branch_and_bound;
mod single_random_draw;

use bitcoin::Amount;
use bitcoin::FeeRate;
use bitcoin::SignedAmount;
use bitcoin::TxOut;
use bitcoin::Weight;

pub use crate::branch_and_bound::select_coins_bnb;
use crate::single_random_draw::select_coins_srd;
use bitcoin::blockdata::transaction::TxIn;
use rand::thread_rng;

/// Trait that a UTXO struct must implement to be used as part of the coin selection
/// algorithm.
pub trait Utxo: Clone {
    /// Return the value of the UTXO.
    fn get_value(&self) -> u64;
}

// https://github.com/bitcoin/bitcoin/blob/f722a9bd132222d9d5cd503b5af25c905b205cdb/src/wallet/coinselection.h#L20
const CHANGE_LOWER: Amount = Amount::from_sat(50_000);

/// This struct contains the weight of all params needed to satisfy the UTXO.
///
/// The idea of using a WeightUtxo type was inspired by the BDK implementation:
/// <https://github.com/bitcoindevkit/bdk/blob/feafaaca31a0a40afc03ce98591d151c48c74fa2/crates/bdk/src/types.rs#L181>
#[derive(Clone, Debug, PartialEq)]
// note, change this to private?  No good reason to be public.
pub struct WeightedUtxo {
    /// The satisfaction_weight is the size of the required params to satisfy the UTXO.
    pub satisfaction_weight: Weight,
    /// The corresponding UTXO.
    pub utxo: TxOut,
}

/// TODO
pub fn effective_value(fee_rate: FeeRate, satisfaction_weight: Weight, value: Amount) -> Option<SignedAmount> {
    let signed_input_fee = calculate_fee(fee_rate, satisfaction_weight)?.to_signed().ok()?;
    value.to_signed().ok()?.checked_sub(signed_input_fee)
}

/// TODO
pub fn calculate_fee(fee_rate: FeeRate, satisfaction_weight: Weight) -> Option<Amount> {
    let weight = satisfaction_weight.checked_add(TxIn::BASE_WEIGHT)?;
    fee_rate.checked_mul_by_weight(weight)
}

/// TODO
pub fn calculate_waste(fee_rate: FeeRate, long_term_fee_rate: FeeRate, satisfaction_weight: Weight) -> Option<SignedAmount> {
    let fee: SignedAmount = calculate_fee(fee_rate, satisfaction_weight)?.to_signed().ok()?;
    let lt_fee: SignedAmount = calculate_fee(long_term_fee_rate, satisfaction_weight)?.to_signed().ok()?;
    fee.checked_sub(lt_fee)
}

/// Select coins first using BnB algorithm similar to what is done in bitcoin
/// core see: <https://github.com/bitcoin/bitcoin/blob/f3bc1a72825fe2b51f4bc20e004cef464f05b965/src/wallet/coinselection.cpp>,
/// and falls back on a random UTXO selection. Returns none if the target cannot
/// be reached with the given utxo pool.
/// Requires compilation with the "rand" feature.
#[cfg(feature = "rand")]
#[cfg_attr(docsrs, doc(cfg(feature = "rand")))]
pub fn select_coins<T: Utxo>(
    target: Amount,
    cost_of_change: Amount,
    fee_rate: FeeRate,
    long_term_fee_rate: FeeRate,
    weighted_utxos: &[WeightedUtxo]
) -> Option<impl Iterator<Item = &WeightedUtxo>>
    {
        let mut eff_values: Vec<Amount> = weighted_utxos
            .iter()
            .filter_map(|w| effective_value(fee_rate, w.satisfaction_weight, w.utxo.value))
            .filter(|e| e.is_positive())
            .map(|e| e.to_unsigned().unwrap())
            .collect();

        let bnb =
            select_coins_bnb(target, cost_of_change, fee_rate, long_term_fee_rate, &mut eff_values, weighted_utxos);

        let index_list = if bnb.is_none() {
            select_coins_srd(target, &mut eff_values, &mut thread_rng()).unwrap()
        } else {
            bnb.unwrap()
        };

        for (i, u) in weighted_utxos.iter().enumerate() {
            if !index_list.contains(&i) {
                weighted_utxos.to_vec().remove(i);
            }
        }

        Some(weighted_utxos.iter())
    }
