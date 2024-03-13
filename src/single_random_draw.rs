//! This library provides efficient algorithms to compose a set of unspent transaction outputs
//! (UTXOs).

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
    eff_values: &'a mut [Amount],
    rng: &mut R,
) -> Option<Vec<usize>> {
    eff_values.shuffle(rng);

    let threshold = target + CHANGE_LOWER;

    let mut sum = Amount::ZERO;
    let mut index_list = vec![];

    for (i, e) in eff_values.iter().enumerate() {
        sum += *e;
        index_list.push(i);

        if sum >= threshold {
            return Some(index_list);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::single_random_draw::select_coins_srd;
    use crate::WeightedUtxo;
    use crate::CHANGE_LOWER;
    use bitcoin::Amount;
    use bitcoin::ScriptBuf;
    use bitcoin::TxOut;
    use bitcoin::Weight;
    use core::str::FromStr;
    use rand::rngs::mock::StepRng;

    const FEE_RATE: FeeRate = FeeRate::from_sat_per_kwu(10);
    const SATISFACTION_SIZE: Weight = Weight::from_wu(204);

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
        let mut eff_values =
            vec![Amount::from_str("1 cBTC").unwrap(), Amount::from_str("2 cBTC").unwrap()];

        let result = select_coins_srd(target, &mut eff_values, &mut get_rng()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], 0);
    }

    #[test]
    fn select_coins_srd_no_solution() {
        let target: Amount = Amount::from_str("4 cBTC").unwrap();
        let mut eff_values =
            vec![Amount::from_str("1 cBTC").unwrap(), Amount::from_str("2 cBTC").unwrap()];

        let result = select_coins_srd(target, &mut eff_values, &mut get_rng());
        assert!(result.is_none())
    }

    #[test]
    fn select_coins_srd_all_solution() {
        let target: Amount = Amount::from_str("2.5 cBTC").unwrap();
        let mut eff_values =
            vec![Amount::from_str("1 cBTC").unwrap(), Amount::from_str("2 cBTC").unwrap()];

        let result = select_coins_srd(target, &mut eff_values, &mut get_rng()).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], 0);
        assert_eq!(result[1], 1);
    }

    #[test]
    fn select_coins_skip_negative_effective_value() {
        //let target: Amount = Amount::from_str("2 cBTC").unwrap() - CHANGE_LOWER;

        //let mut eff_values = vec![
            //Amount::from_str("1 cBTC").unwrap(),
            //Amount::from_str("2 cBTC").unwrap(),
            //Amount::from_str("1 sat").unwrap(),
        //];

        //let mut rng = get_rng();
        //let result = select_coins_srd(target, &mut eff_values, &mut rng).unwrap();

        //assert_eq!(result.len(), 2);
        //assert_eq!(result[0], 0);
        //assert_eq!(result[1], 1);
    }

    #[test]
    fn select_coins_srd_fee_rate_error() {
        //let target: Amount = Amount::from_str("2 cBTC").unwrap();
        //let mut eff_values =
            //vec![Amount::from_str("1 cBTC").unwrap(), Amount::from_str("2 cBTC").unwrap()];
        //let result = select_coins_srd(target, &mut eff_values, &mut get_rng());
        //assert!(result.is_none());
    }

    #[test]
    fn select_coins_srd_change_output_too_small() {
        let target: Amount = Amount::from_str("3 cBTC").unwrap();
        let mut eff_values =
            vec![Amount::from_str("1 cBTC").unwrap(), Amount::from_str("2 cBTC").unwrap()];
        let result = select_coins_srd(target, &mut eff_values, &mut get_rng());
        assert!(result.is_none());
    }

    #[test]
    fn select_coins_srd_with_high_fee() {
        // the first UTXO is 2 cBTC.  If the fee is greater than 10 sats,
        // then more than the single 2 cBTC output will need to be selected
        // if the target is 1.99999 cBTC.  That is, 2 cBTC - 1.9999 cBTC = 10 sats.
        let target: Amount = Amount::from_str("1.99999 cBTC").unwrap();

        // fee = 15 sats, since
        // 40 sat/kwu * (204 + BASE_WEIGHT) = 15 sats
        let fee_rate: FeeRate = FeeRate::from_sat_per_kwu(40);

        let mut eff_values =
            vec![Amount::from_str("1 cBTC").unwrap(), Amount::from_str("2 cBTC").unwrap()];

        let result = select_coins_srd(target, &mut eff_values, &mut get_rng()).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], 0);
        assert_eq!(result[1], 1);
    }

    #[test]
    // TODO fix me.
    fn select_coins_srd_addition_overflow() {
        let target: Amount = Amount::from_str("2 cBTC").unwrap();
        let mut eff_value = vec![Amount::from_str("1 cBTC").unwrap()];
        let result = select_coins_srd(target, &mut eff_value, &mut get_rng());
        assert!(result.is_none());
    }
}
