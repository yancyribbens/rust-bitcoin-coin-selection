use bitcoin_coin_selection::select_coins_srd;
use bitcoin_coin_selection::WeightedUtxo;
use honggfuzz::fuzz;
use arbitrary::Arbitrary;

use rand::thread_rng;

use bitcoin::FeeRate;
use bitcoin::Amount;

#[derive(Arbitrary, Debug)]
pub struct Params {
    target: Amount,
    fee_rate: FeeRate,
    weighted_utxos: Vec<WeightedUtxo>,
}

fn main() {
    loop {
        fuzz!(|params: Params| {
            let Params { target: t, fee_rate: f, weighted_utxos: wu } = params;
            select_coins_srd(t, f, &wu, &mut thread_rng());
        });
    }
}
