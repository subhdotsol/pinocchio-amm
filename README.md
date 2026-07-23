# Pinocchio AMM

A constant-product AMM program for Solana, written with [pinocchio](https://github.com/febo/pinocchio). Benchmarked against an equivalent Anchor implementation to measure the compute unit overhead of each framework.

## Architecture

The program implements four instructions over a standard x/y liquidity pool:

| Instruction | Description |
|-------------|-------------|
| `initialize` | Creates the pool config, LP mint, and vaults |
| `deposit` | Adds liquidity and mints LP tokens to the provider |
| `swap` | Swaps one token for the other using the constant-product curve |
| `withdraw` | Burns LP tokens and returns the underlying assets |

Pool state is stored in a `Config` account (PDA seeded by `["config", seed]`). The LP mint is a separate PDA seeded by `["lp", config]`. Vaults are ATAs owned by the config PDA.

## Benchmarks

CU measurements via [mollusk-svm](https://github.com/buffalojoec/mollusk). Results written to `target/benches/`.

### Pinocchio vs Anchor

| Instruction | Pinocchio (CU) | Anchor (CU) | Savings |
|-------------|---------------:|------------:|--------:|
| initialize  |          35341 |       61782 |   42.8% |
| deposit     |          13895 |       34441 |   59.7% |
| swap        |          19263 |       27891 |   30.9% |
| withdraw    |          14328 |       35375 |   59.5% |

## Project Layout

```
src/
  instructions/   # initialize, deposit, swap, withdraw
  state/          # Config account definition
  constants.rs    # Seeds, curve precision
  error.rs        # Program errors
benchmarks/
  initialize_ix_bench.rs   # Pinocchio instruction benches
  deposit_ix_bench.rs
  swap_ix_bench.rs
  withdraw_ix_bench.rs
  initialize_anchor_bench.rs  # Equivalent Anchor benches
  deposit_anchor_bench.rs
  swap_anchor_bench.rs
  withdraw_anchor_bench.rs
  compare_bench.rs            # Side-by-side CU comparison, writes target/benches/comparison.md
tests/
  elfs/           # Prebuilt SBF binaries (pinocchio AMM, Anchor AMM, SPL programs)
```

## Development

```sh
# Build the SBF binary
make build

# Run tests
make test

# Pinocchio instruction benchmarks
make bench

# Anchor instruction benchmarks
make bench-anchor

# Side-by-side comparison (writes target/benches/comparison.md)
make compare

# Format, lint, build, test, bench all at once
make all
```

## Dependencies

- [pinocchio](https://github.com/febo/pinocchio) — zero-dependency Solana program framework
- [pinocchio-token](https://github.com/febo/pinocchio) — token CPI helpers
- [pinocchio-associated-token-account](https://github.com/febo/pinocchio) — ATA CPI helpers
- [constant-product-curve](https://github.com/deanmlittle/constant-product-curve) — swap math
- [mollusk-svm](https://github.com/buffalojoec/mollusk) — lightweight SBF test/bench harness
