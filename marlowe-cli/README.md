# marlowe-cli

Embedded CLI for Extended Marlowe contracts. It calls `marlowe-api` library code directly and does not require the HTTP server.

## Build

```bash
cargo build
```

## Common usage

```bash
cargo run -- validate --in ../examples/simple-pay.yaml
cargo run -- preview --in ../examples/simple-pay.yaml --interval-start 0 --interval-end 0
cargo run -- analyze --in ../examples/simple-pay.yaml --property deadline-safety
```

## Commands

- `validate`
- `preview`
- `step`
- `analyze`
- `repair`
- `run-plan` (currently not implemented)
- `version`

## Notes

- Analyzer flows require `z3` in `PATH`.
- Default output format is JSON for agent/tool consumption.
