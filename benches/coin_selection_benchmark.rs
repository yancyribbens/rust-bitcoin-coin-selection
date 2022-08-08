use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_bitcoin_coin_selection::select_coins_bnb;
use rust_bitcoin_coin_selection::Utxo;

#[derive(Clone, Debug, Eq, PartialEq)]
struct MinimalUtxo {
    value: u64,
}

impl Utxo for MinimalUtxo {
    fn get_value(&self) -> u64 {
        self.value
    }
}

pub fn criterion_benchmark(c: &mut Criterion) {
    const ONE_BTC: u64 = 100_000_000;

    let utxo_range = (100_000_001..100_001_000);
    let mut utxo_pool = Vec::new();

    for i in utxo_range{
        let u = MinimalUtxo { value: i};
        utxo_pool.push(u);
    }

    const COST_OF_CHANGE: u64 = 50_000_000;

    c.bench_function(
        "large_pool_without_solution",
        |b| b.iter(|| 
            select_coins_bnb(
                ONE_BTC, COST_OF_CHANGE, &mut utxo_pool.clone()
                )
            )
        );
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
