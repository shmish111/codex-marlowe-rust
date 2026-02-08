# marlowe-api

Rust API scaffold plus Extended Marlowe YAML parsing and type checking.

## DSL support

The library understands the schema defined in `/Users/davidsmith/code/codex-marlowe-rust/extended-marlowe.yaml`.

Key syntax rules:

- Constructors are single-key mappings, for example `{ Close: {} }` or `Pay: { ... }`.
- Holes are universal and use `?name` in any typed position.
- Parameters are strict and use `$name` only in `Value` and `Timeout` positions.
- Escaping uses double prefixes: `??x` parses as literal `?x`, `$$x` parses as literal `$x`.

## Type checker

`type_check` performs structural and semantic validation:

- infers and reports hole/parameter types
- validates `UseValue` references against in-scope `Let` bindings
- validates known accounts/parties/tokens/choices when `require_known_definitions = true`
- checks choice bound ranges when statically known
- emits warnings for shadowed/unused `Let` bindings and non-static bounds

`ready_to_run` is `true` only when:

- there are no errors
- there are no holes
- there are no unresolved parameters

## Simulator

`simulate_transaction` implements one-transaction Core Marlowe execution semantics:

- applies `fixInterval` rules
- rejects invalid intervals (`start > end`)
- rejects intervals fully in the past (`end < min_time`)
- clamps interval start to `max(start, min_time)`
- reduces contract to quiescence, applies one transaction input list, then reduces again
- reports Marlowe-style warnings (`NonPositivePay`, `PartialPay`, `AssertionFailed`, etc.)

The HTTP server exposes this as:

- `POST /simulate/step`

Request payload fields:

- `contract_yaml`: full contract YAML
- `state`: optional state (`min_time`, `accounts`, `choices`, `bound_values`)
- `transaction`: `interval_start`, `interval_end`, and `inputs`

The endpoint returns either:

- `result = "success"` with updated state, payments, warnings, and next `contract_yaml`
- `result = "error"` with a structured simulator error code/message

## Testing

Run all checks:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Run mutation checks (optional, requires `cargo-mutants`):

```bash
./scripts/run-mutation-checks.sh
```

Run fuzzing harnesses (optional, requires `cargo-fuzz`):

```bash
./scripts/run-fuzz.sh parse_yaml
./scripts/run-fuzz.sh parse_and_typecheck
```
