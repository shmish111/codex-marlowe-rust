# Marlowe Studio (React + Monaco)

This repository contains a Marlowe contract editing and simulation UI built with React and Monaco Editor.

## What is included

- `marlowe-ui`: Frontend app (React, TypeScript, Vite, Monaco)
- `examples`: Example contracts used by the UI

## Frontend features

- Monaco editor for YAML contracts
- Example loader modal
- Auto-validation with debounce
- Validation diagnostics with inline editor markers
- Incomplete contract detection (unresolved holes/parameters)
- Simulation panel gated by validation status
- Step-by-step simulation input application

## Prerequisites

- Node.js 18+
- npm
- Backend API available (default OpenAPI URL in UI: `http://127.0.0.1:3000/openapi.json`)

## Run the app

From the UI directory:

```bash
cd marlowe-ui
npm install
npm run dev
```

Then open the URL shown by Vite (usually `http://127.0.0.1:5173`).

## Quality checks

From `marlowe-ui`:

```bash
npm run format
npm run lint
npm run test
```

## API endpoints used by the frontend

- `POST /simulate/preview`
- `POST /simulate/step`
- `POST /typecheck/explain` (available for richer validation UX)
- `GET /health`
- `GET /openapi.json`

## Notes

- Simulation is enabled only when contract validation is fully successful.
- If API connectivity fails, a warning is shown in the header.
