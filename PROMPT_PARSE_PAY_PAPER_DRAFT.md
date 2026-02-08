# Prompt, Parse, Pay: Rebuilding Marlowe Playground with AI in a Single Session

**Authors:** David Smith and Codex (GPT-5.3-Codex)

## Abstract

This paper reports a single-session software sprint in which a developer who did not write or read code (David Smith) collaborated with an AI coding agent (Codex) to produce a working reinterpretation of Marlowe Playground in Rust and React. The session produced parser/type-checker infrastructure, simulation semantics, an HTTP API with generated OpenAPI, and a browser-based editing and simulation UI. Evidence is taken from repository history and project documentation. The implementation history shows 49 commits over 4h 11m 41s on 2026-02-08, including substantial testing-oriented work (property tests, fuzz harnesses, mutation configuration, regression fixtures) and iterative UX refinement in Monaco-based diagnostics. The results are encouraging and mildly alarming for the author's future job prospects: AI-assisted development can rapidly produce coherent systems across backend and frontend layers, while leaving the human operator with limited direct understanding of internal code quality. We present quantitative observations, process notes from three prior failed attempts, and a risk framing for teams considering similar workflows.

## 1. Introduction

Marlowe is a domain-specific language for financial contracts on Cardano, with established tooling and educational resources [1,2]. The production Marlowe ecosystem on Cardano was not built through this AI-assisted workflow and is engineered to a substantially higher robustness bar, including rigorous methods and formal treatment in its broader research and implementation context [1,2]. This project aimed to build a reinterpretation of Marlowe Playground as a local, full-stack developer environment, with emphasis on contract authoring, validation, and step-wise simulation. As a playground-oriented tool, its primary goal is interactive exploration rather than claiming production-grade formal completeness.

The notable process constraint was social rather than technical: the human collaborator (David Smith) contributed prompting strategy and manual test execution, but did not write code and did not read the code at all. The AI agent produced implementation changes throughout a single concentrated session. A second constraint is background: the human collaborator had never written more than a \"hello world\" application in Rust before this project. A third constraint is verification asymmetry: the human collaborator did not manually test backend behavior and relied on automated checks and UI-observed outcomes.

The central question is not whether the system can be built this way (it can), but whether this style of success should be considered robust engineering or fast, plausible theater with excellent demos.

## 2. Prior Attempts and Setup

Before the successful sprint reported here, three previous attempts were made with OpenCode + Kimi 2.5 targeting a Haskell backend. Those attempts were slower and repeatedly stalled on frontend integration issues. Despite being unsuccessful, they provided practical constraints that improved this run:

- Prior failures established that `monaco-yaml` integration with React + Vite was costly and brittle in this context.
- Prior failures also clarified that backend and frontend work can proceed in parallel if a stable `openapi.json` contract is defined early.
- The successful run therefore adopted a simpler Monaco integration with explicit diagnostics plumbing from API responses.

This is a useful pattern: failed AI runs can function as expensive but effective prompt engineering.

## 3. Method

### 3.1 Data sources

The analysis uses:

- Git commit history from `https://github.com/shmish111/codex-marlowe-rust`.
- Project documentation in `https://github.com/shmish111/codex-marlowe-rust/blob/main/README.md` and `https://github.com/shmish111/codex-marlowe-rust/blob/main/marlowe-api/README.md`.
- Local Codex artifacts (available but incomplete/noisy) for process context.

### 3.3 Coordination model

Execution used two concurrent Codex sessions: one focused on frontend and one focused on backend. When frontend work required backend changes, the frontend session generated explicit instructions for the backend session; after backend implementation, the backend session returned a concise change summary for the frontend session. This relay protocol acted as a lightweight interface contract between two model threads.

### 3.2 Session boundaries and quantitative summary

From repository history:

- First commit: 2026-02-08 18:35:09 +0000
- Latest commit in sprint window: 2026-02-08 22:46:50 +0000
- Duration: 4h 11m 41s
- Total commits: 49
- Mean commit rate: 11.68 commits/hour

A coarse phase breakdown from commit subjects:

1. **Language + correctness foundation** (early phase): parser/type checker, serialization, and heavy testing additions.
2. **Execution semantics + API surface** (middle phase): strict simulator core, `/simulate/step`, `/simulate/preview`, OpenAPI generation/serving, typed error payloads.
3. **UI integration + diagnostics polish** (late phase): API wiring, stateful stepping, autovalidation, source-span-driven Monaco markers, and validation gating.

## 4. System Outcome

The resulting system includes the expected Marlowe-playground-style loop: edit contract, validate, preview possible inputs, execute step, inspect resulting state.

### 4.1 Backend capabilities (Rust API)

Per project documentation, implemented backend features include:

- Extended Marlowe YAML parse/serialize and type-checking with hole/parameter inference.
- Structural and semantic diagnostics, including unresolved holes vs unresolved parameters.
- Transaction simulation with interval rules and warning reporting.
- Preview endpoint for available quiescent inputs and warnings.
- Explain endpoint for grouped diagnostics and remediation hints.
- OpenAPI generation from handlers and serving at `/openapi.json`.

### 4.2 Frontend capabilities (React + Monaco)

Implemented frontend capabilities include:

- Monaco editor for YAML contracts.
- Example contract loader.
- Debounced autovalidation.
- Inline diagnostics with source-span placement.
- Simulation panel gating based on validation readiness.
- Stateful step-by-step simulation interactions.

## 5. Why This Run Worked

Three practical factors appear to explain the improvement over prior attempts:

1. **Error-shape stability.** Structured subcodes and typed payloads reduced frontend ambiguity and made marker rendering predictable.
2. **API contract-first parallelization.** Early `openapi.json` availability allowed frontend and backend work to proceed in parallel with lower integration friction.
3. **Two-session relay coordination.** Dedicated frontend/backend Codex sessions exchanged explicit change requests and concise implementation summaries, reducing cross-layer ambiguity.
4. **Testing density as control.** The commit stream shows disproportionate investment in tests/harnesses early, which likely reduced semantic drift later.

In plain terms, the process behaved less like “generate an app” and more like “run two specialized model teams connected by a strict API and handoff notes.”

## 6. Risks, Limitations, and the “Do Not Try This at Work” Section

The most important negative result is epistemic: the human operator can ship a sophisticated system while not knowing whether it is maintainable, secure, or subtly wrong.

Primary risks:

- **Unknown correctness boundary.** Passing tests do not define full semantic coverage for financial contracts.
- **Maintainability debt.** If only the model understands architecture-level rationale, future human changes become fragile.
- **Over-trust transfer.** Rapid visible progress can be mistaken for verified robustness.
- **Operational ambiguity.** Ownership and incident response are unclear when authorship is hybrid and code comprehension is asymmetric.

Recommended mitigations for real teams:

- Require human code-reading checkpoints for each subsystem.
- Add explicit threat modeling and property coverage audits.
- Track test quality metrics, not just test count.
- Budget refactoring time after AI sprint completion.

The slogan version remains accurate: AI can do this; the safety question is whether anyone can responsibly operate what it produced.

## 7. Authorship and Accountability Note

This draft attributes authorship to **David Smith and Codex (GPT-5.3-Codex)**. Contribution split in this case:

- David Smith: goals, constraints, prompting strategy, cross-session FE/BE relay coordination, manual test execution, acceptance decisions.
- Codex: code generation, iterative refactoring, endpoint/UI integration, diagnostics plumbing.

Accountability should not be delegated to the model. In professional settings, humans still own release decisions, risk acceptance, and maintenance plans.

## 8. Conclusion

A functional reinterpretation of Marlowe Playground was produced in one session with high apparent velocity and strong feature completeness. The outcome demonstrates that AI-assisted coding can cross backend/frontend boundaries effectively under tight iteration loops. It also demonstrates a governance problem: successful output can outpace human understanding. Future work should treat this gap as a first-class engineering concern, not a footnote.

## References

[1] Cardano Developer Portal. *Marlowe* overview and learning resources. [https://docs.cardano.org/developer-resources/smart-contracts/marlowe/](https://docs.cardano.org/developer-resources/smart-contracts/marlowe/)

[2] Marlowe Team (Input Output). *marlowe-cardano* repository. [https://github.com/input-output-hk/marlowe-cardano](https://github.com/input-output-hk/marlowe-cardano)

[3] Microsoft. *Monaco Editor* documentation and API. [https://microsoft.github.io/monaco-editor/](https://microsoft.github.io/monaco-editor/)

[4] React Team. *React* documentation. [https://react.dev/](https://react.dev/)

[5] Vite Team. *Vite* documentation. [https://vite.dev/](https://vite.dev/)

## Appendix A (Optional, if included in final submission)

A cleaned reproducibility appendix can be generated from:

- Git history (authoritative for code evolution timing and milestones).
- Selected local Codex runtime artifacts (for prompt/event traces), with sensitive and irrelevant entries removed.

Given variability in local logging formats, the appendix should prioritize deterministic artifacts (commit hashes, timestamps, diff summaries, and command transcripts) over model-internal telemetry.
