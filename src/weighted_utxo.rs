use bitcoin_units::{Amount, FeeRate, SignedAmount, Weight};

use crate::effective_value;

// 32 byte txid, 4 byte output index, 1 byte scriptSig, and 4 byte sequence
const BASE_WEIGHT: Weight = Weight::from_vb_unwrap(32 + 4 + 1 + 4);

/// Behavior needed for coin-selection.
pub trait WeightedUtxo {
    /// weight
    fn weight(&self) -> Weight;

    /// value.
    fn value(&self) -> Amount;    

    /// feerate.
    fn feerate(&self) -> FeeRate;

    /// eff_value
    fn effective_value(&self) -> Amount {
        let fee = self.feerate().to_fee(self.weight());
        let eff_value = (self.value() - fee).unwrap_or(Amount::ZERO);
        eff_value
    }
}

use std::cmp::Ordering;

impl Eq for dyn 'static + WeightedUtxo {} 

impl PartialEq for dyn 'static + WeightedUtxo {
    fn eq(&self, other: &Self) -> bool { true }
} 

impl Ord for dyn 'static + WeightedUtxo {
    fn cmp(&self, other: &Self) -> Ordering {
        other.effective_value().cmp(&self.effective_value()).then(self.weight().cmp(&other.weight()))
    }
}

impl PartialOrd for dyn 'static + WeightedUtxo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_utxo_constructor_overflow() {
        let value = Amount::from_sat_u32(100);
        let weight = Weight::MAX;
        let fee_rate = FeeRate::MAX;
        let long_term_fee_rate = FeeRate::MAX;

        let utxo = WeightedUtxo::new(value, weight, fee_rate, long_term_fee_rate);
        assert!(utxo.is_none());
    }

    #[test]
    fn weighted_utxo_constructor_negative_eff_value() {
        let value = Amount::from_sat_u32(1);
        let weight = Weight::from_vb(68).unwrap();
        let fee_rate = FeeRate::from_sat_per_kwu(20);
        let long_term_fee_rate = FeeRate::from_sat_per_kwu(20);

        let utxo = WeightedUtxo::new(value, weight, fee_rate, long_term_fee_rate);
        assert!(utxo.is_none());
    }
}
