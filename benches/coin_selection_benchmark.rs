use criterion::{criterion_group, criterion_main, Criterion};
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

    let utxo_range = ONE_BTC..ONE_BTC + 100_000;

    let mut utxo_pool:Vec<_> = utxo_range.map(|v| MinimalUtxo { value: v }).collect();

    const COST_OF_CHANGE: u64 = 50_000_000;

    c.bench_function("find_solution_with_large_utxo_pool", |b| {
        b.iter(|| select_coins_bnb(ONE_BTC + 1, COST_OF_CHANGE, &mut utxo_pool))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
