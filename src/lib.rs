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
use rand::thread_rng;

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

// Serialized length of a u32.
const SEQUENCE_SIZE: u64 = 4;
// The serialized lengths of txid and vout.
const OUTPOINT_SIZE: u64 = 32 + 4;
const TX_IN_BASE_WEIGHT: Weight =
        Weight::from_vb_unwrap(OUTPOINT_SIZE + SEQUENCE_SIZE);

// Predict the fee Amount to spend a UTXO.
//
// To predict the fee, the predicted weight is:
// weight = satisfaction_weight + TX_IN base weight.
// 
// The fee is then calculated as:
// fee = weight * fee_rate
fn calculate_fee_prediction(satisfaction_weight: Weight, fee_rate: FeeRate) -> Option<Amount> {
    let weight = satisfaction_weight.checked_add(TX_IN_BASE_WEIGHT)?;
    fee_rate.checked_mul_by_weight(weight)
}

impl WeightedUtxo {
    fn waste(&self, fee_rate: FeeRate, long_term_fee_rate: FeeRate) -> Option<SignedAmount> {
        let fee: SignedAmount = calculate_fee_prediction(self.satisfaction_weight, fee_rate)?.to_signed().ok()?;
        let lt_fee: SignedAmount = calculate_fee_prediction(self.satisfaction_weight, long_term_fee_rate)?.to_signed().ok()?;
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
pub fn select_coins(
    target: Amount,
    cost_of_change: Amount,
    fee_rate: FeeRate,
    long_term_fee_rate: FeeRate,
    weighted_utxos: &[WeightedUtxo],
) -> Option<std::vec::IntoIter<&WeightedUtxo>> {
    {
        let bnb =
            select_coins_bnb(target, cost_of_change, fee_rate, long_term_fee_rate, weighted_utxos);

        if bnb.is_some() {
            bnb
        } else {
            select_coins_srd(target, fee_rate, weighted_utxos, &mut thread_rng())
        }
    }
}
