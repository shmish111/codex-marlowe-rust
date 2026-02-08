# marlowe-api

Rust API for Extended Marlowe contracts:

- parse and serialize Extended Marlowe YAML
- type-check with hole/parameter inference
- simulate one transaction step or preview available inputs
- expose OpenAPI from Rust source definitions

## DSL support

The library understands the schema defined in `/Users/davidsmith/code/codex-marlowe-rust/extended-marlowe.yaml`.

Key syntax rules:

- Constructors are single-key mappings, for example `{ Close: {} }` or `Pay: { ... }`.
- Holes are universal and use `?name` in any typed position.
- Parameters are strict and use `$name` only in `Value` and `Timeout` positions.
- Escaping uses double prefixes: `??x` parses as literal `?x`, `$$x` parses as literal `$x`.

Important detail:

- A string field like `ChoiceId.name` is still a string field, so `name: ?x` there is treated as a literal string unless the whole typed node is a hole.

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

## HTTP API

Exposed routes:

- `GET /health`
- `GET /openapi.json`
- `POST /simulate/step`
- `POST /simulate/preview`
- `POST /typecheck/explain`

The server also includes permissive CORS middleware intended for local UI dev-server usage.

## Simulator

`simulate_transaction` implements one-transaction Core Marlowe execution semantics:

- applies `fixInterval` rules
- rejects invalid intervals (`start > end`)
- rejects intervals fully in the past (`end < min_time`)
- clamps interval start to `max(start, min_time)`
- reduces contract to quiescence, applies one transaction input list, then reduces again
- reports Marlowe-style warnings (`NonPositivePay`, `PartialPay`, `AssertionFailed`, etc.)

Request payload fields:

- `contract_yaml`: full contract YAML
- `state`: optional state (`min_time`, `accounts`, `choices`, `bound_values`)
- `transaction`: `interval_start`, `interval_end`, and `inputs`

The endpoint returns either:

- `result = "success"` with updated state, payments, warnings, and next `contract_yaml`
- `result = "error"` with a structured simulator error code/message

Error payloads include frontend-locatable fields:

- `error.path` for the primary failing location
- `error.diagnostics[]` for detailed type/hole/param issues, each with its own `path`

`/simulate/preview` returns:

- reduced contract/state for the given interval
- top-level semantic warnings from reduction-to-quiescence
- currently-available `deposit` / `choice` / `notify` inputs at quiescence
- per-input potential warnings (for example, non-positive deposit)

When preview rejects for validation, each diagnostic now includes source span fields for editor markers when available:

- `line`, `column`
- optional `end_line`, `end_column`

`/simulate/step` supports optional trace mode by passing `trace: true`:

- returns `success.trace[]` semantic events with stable `event_id`
- includes `contract_path`, `state_paths`, optional `warning`, optional `payment`
- includes per-event state `delta` (accounts/choices/bound values/min_time changes)

## Explain endpoint

`POST /typecheck/explain` returns grouped diagnostics with remediation hints:

- `summary` counts (`blocking`, `warnings`, `errors`, `holes`, `params`)
- `blocking[]` for `TypeError`, `HoleUnresolved`, `ParamUnresolved`
- `warnings[]` for type-checker warnings

Request supports optional type-check context:

- `require_known_definitions`
- `known_accounts`, `known_parties`, `known_tokens`, `known_choices`

Context entries must be concrete (no holes), otherwise request is rejected with `RequestError/ContextError`.

Malformed JSON requests across major POST endpoints are normalized to structured `RequestError/InvalidJson` responses (instead of framework-default 422 payloads).

## Testing

Run all checks:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Generate OpenAPI from Rust source:

```bash
cargo run --bin generate_openapi
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
