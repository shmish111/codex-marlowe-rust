# Marlowe Studio

This repository includes both a human-oriented web UI and an embedded CLI for AI-agent workflows.

## What is included

- `marlowe-ui`: frontend app (React, TypeScript, Vite, Monaco)
- `marlowe-api`: Rust library + HTTP server for parse/typecheck/simulate/analyze
- `marlowe-cli`: embedded CLI that uses `marlowe-api` directly (no HTTP required)
- `examples`: sample contracts

## CLI quick start (embedded mode)

From `marlowe-cli`:

```bash
cargo run -- validate --in ../examples/simple-pay.yaml
cargo run -- preview --in ../examples/simple-pay.yaml --interval-start 0 --interval-end 0
cargo run -- analyze --in ../examples/simple-pay.yaml --property deadline-safety
```

Useful commands:

- `validate`
- `preview`
- `step`
- `analyze`
- `repair`
- `version`

## Web UI quick start

The UI expects the API server at `http://127.0.0.1:3000`.

Start API:

```bash
cd marlowe-api
cargo run
```

Start UI:

```bash
cd marlowe-ui
npm install
npm run dev
```

Then open the Vite URL (usually `http://127.0.0.1:5173`).

## Notes

- The analyzer currently requires `z3` in `PATH`.
- UI simulation is enabled only when validation is fully successful.
