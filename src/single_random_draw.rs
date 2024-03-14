//! This library provides efficient algorithms to compose a set of unspent transaction outputs
//! (UTXOs).

use crate::CoinSelect;
use crate::CHANGE_LOWER;
use bitcoin::Amount;
use rand::seq::SliceRandom;

/// Randomly select coins for the given target by shuffling the UTXO pool and
/// taking UTXOs until the given target is reached.
///
/// The fee_rate can have an impact on the selection process since the fee
/// must be paid for in addition to the target.  However, the total fee
/// is dependant on the number of UTXOs consumed and the new inputs created.
/// The selection strategy therefore calculates the fees of what is known
/// ahead of time (the number of UTXOs create and the transaction header),
/// and then then for each new input, the effective_value is tracked which
/// deducts the fee for each individual input at selection time.  For more
/// discussion see the following:
///
/// https://bitcoin.stackexchange.com/questions/103654/calculating-fee-based-on-fee-rate-for-bitcoin-transaction/114847#114847
///
/// ## Parameters
/// ///
/// /// * `target` - target value to send to recipient.  Include the fee to pay for the known parts of the transaction excluding the fee for the inputs.
/// /// * `fee_rate` - ratio of transaction amount per size.
/// /// * `weighted_utxos` - Weighted UTXOs from which to sum the target amount.
/// /// * `rng` - used primarily by tests to make the selection deterministic.
pub fn select_coins_srd<'a, R: rand::Rng + ?Sized>(
    target: Amount,
    coin_select: &'a [CoinSelect],
    rng: &mut R,
) -> Option<std::vec::IntoIter<&'a CoinSelect>> {
    let mut result: Vec<_> = coin_select.iter().collect();
    let mut origin = result.to_owned();
    origin.shuffle(rng);

    result.clear();

    // Avoid making needlessly small change amounts.
    // The amount should be larger than the target by a reasonable amount.
    // That way, dust amounts are avoided.
    let threshold = target + CHANGE_LOWER;
    let mut value = Amount::ZERO;

    for coin in origin {
        let effective_value = coin.effective_value;
        value += effective_value;

        result.push(coin);

        if value >= threshold {
            return Some(result.into_iter());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::single_random_draw::select_coins_srd;
    use crate::CoinSelect;
    use bitcoin::Amount;
    use bitcoin::SignedAmount;
    use bitcoin::ScriptBuf;
    use bitcoin::TxOut;
    use core::str::FromStr;
    use rand::rngs::mock::StepRng;

    fn create_coin() -> Vec<CoinSelect> {
        let coin_one = CoinSelect {
            effective_value: Amount::from_str("1 cBTC").unwrap(),
            utxo: TxOut {
                value: Amount::from_str("1 cBTC").unwrap(), // TODO calculate eff_value
                script_pubkey: ScriptBuf::new(),
            },
            waste: SignedAmount::ZERO 
        };

        let coin_two = CoinSelect {
            effective_value: Amount::from_str("2 cBTC").unwrap(),
            utxo: TxOut {
                value: Amount::from_str("2 cBTC").unwrap(), // TODO calculate eff_value
                script_pubkey: ScriptBuf::new(),
            },
            waste: SignedAmount::ZERO 
        };

        vec![coin_one, coin_two]
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

    #[test]
    fn select_coins_srd_with_solution() {
        let target: Amount = Amount::from_str("1.5 cBTC").unwrap();
        let coin: Vec<CoinSelect> = create_coin();

        let result: Vec<&CoinSelect> =
            select_coins_srd(target, &coin, &mut get_rng())
                .expect("unexpected error")
                .collect();

        let expected_result = Amount::from_str("2 cBTC").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(expected_result, result[0].utxo.value);
    }

    #[test]
    fn select_coins_srd_no_solution() {
        let target: Amount = Amount::from_str("4 cBTC").unwrap();
        let coin: Vec<CoinSelect> = create_coin();

        let result = select_coins_srd(target, &coin, &mut get_rng());
        assert!(result.is_none())
    }

    #[test]
    fn select_coins_srd_all_solution() {
        let target: Amount = Amount::from_str("2.5 cBTC").unwrap();
        let coin: Vec<CoinSelect> = create_coin();

        let result: Vec<&CoinSelect> =
            select_coins_srd(target, &coin, &mut get_rng())
                .expect("unexpected error")
                .collect();

        let expected_second_element = Amount::from_str("1 cBTC").unwrap();
        let expected_first_element = Amount::from_str("2 cBTC").unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].utxo.value, expected_first_element);
        assert_eq!(result[1].utxo.value, expected_second_element);
    }

    //#[test]
    //fn select_coins_srd_fee_rate_error() {
        //let target: Amount = Amount::from_str("2 cBTC").unwrap();
        //let weighted_utxos: Vec<WeightedUtxo> = create_weighted_utxos();

        //let result = select_coins_srd(target, FeeRate::MAX, &weighted_utxos, &mut get_rng());
        //assert!(result.is_none());
    //}

    #[test]
    fn select_coins_srd_change_output_too_small() {
        // Test that we don't make needlessly small change amount.
        // The result must be larget than the target by CHANGE_LOWER.
        let target: Amount = Amount::from_str("3 cBTC").unwrap();
        let coin: Vec<CoinSelect> = create_coin();

        let result = select_coins_srd(target, &coin, &mut get_rng());

        assert!(result.is_none());
    }

    //#[test]
    //fn select_coins_srd_with_high_fee() {
        // the first UTXO is 2 cBTC.  If the fee is greater than 10 sats,
        // then more than the single 2 cBTC output will need to be selected
        // if the target is 1.99999 cBTC.  That is, 2 cBTC - 1.9999 cBTC = 10 sats.
        //let target: Amount = Amount::from_str("1.99999 cBTC").unwrap();

        // fee = 15 sats, since
        // 40 sat/kwu * (204 + BASE_WEIGHT) = 15 sats
        //let fee_rate: FeeRate = FeeRate::from_sat_per_kwu(40);
        //let weighted_utxos: Vec<WeightedUtxo> = create_weighted_utxos();

        //let result: Vec<_> = select_coins_srd(target, fee_rate, &weighted_utxos, &mut get_rng())
            //.expect("unexpected error")
            //.collect();
        //let expected_second_element = Amount::from_str("1 cBTC").unwrap();
        //let expected_first_element = Amount::from_str("2 cBTC").unwrap();

        //assert_eq!(result.len(), 2);
        //assert_eq!(result[0].utxo.value, expected_first_element);
        //assert_eq!(result[1].utxo.value, expected_second_element);
    //}

    //#[test]
    //fn select_coins_srd_addition_overflow() {
        //let target: Amount = Amount::from_str("2 cBTC").unwrap();

        //let weighted_utxos: Vec<WeightedUtxo> = vec![WeightedUtxo {
            //satisfaction_weight: Weight::MAX,
            //utxo: TxOut {
                //value: Amount::from_str("1 cBTC").unwrap(),
                //script_pubkey: ScriptBuf::new(),
            //},
        //}];

        //let result = select_coins_srd(target, FEE_RATE, &weighted_utxos, &mut get_rng());
        //assert!(result.is_none());
    //}
}
