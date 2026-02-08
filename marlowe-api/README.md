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
