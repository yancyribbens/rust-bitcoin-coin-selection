use bitcoin_coin_selection::select_coins_bnb;
use bitcoin_coin_selection::WeightedUtxo;
use honggfuzz::fuzz;
use arbitrary::Arbitrary;

use bitcoin::FeeRate;
use bitcoin::Amount;

#[derive(Arbitrary, Debug)]
pub struct Params {
    target: Amount,
    cost_of_change: Amount,
    fee_rate: FeeRate,
    long_term_fee_rate: FeeRate,
    weighted_utxos: Vec<WeightedUtxo>,
}

fn main() {
    loop {
        fuzz!(|params: Params| {
            let Params { target: t, cost_of_change: c, fee_rate: f, long_term_fee_rate: lt_f, weighted_utxos: wu } = params;
            select_coins_bnb(t, c, f, lt_f, &wu);
        });
    }
}
