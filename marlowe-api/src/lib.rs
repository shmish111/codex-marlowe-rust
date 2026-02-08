//! Extended Marlowe DSL support for parsing and type-checking.
//!
//! This crate provides:
//! - A strongly typed AST for the YAML schema in `../extended-marlowe.yaml`.
//! - A YAML parser with support for universal holes (`?name`) and strict params (`$name`).
//! - A type checker with inference for holes/params and context-based definition checks.

pub mod ast;
pub mod http;
pub mod parser;
pub mod serializer;
pub mod sim;
pub mod typecheck;

pub use parser::{parse_contract_yaml, ParseError};
pub use serializer::{contract_to_yaml, contract_to_yaml_string};
pub use sim::{
    preview_inputs, simulate_transaction, simulate_transaction_with_trace, AccountDelta, AccountId,
    BoundValueDelta, ChoiceDelta, MinTimeDelta, Payment, PreviewInput, PreviewResult, SimError,
    SimInput, SimState, SimTransaction, SimTransactionResult, SimTransactionSuccess, StateDelta,
    TraceReduceRule, TraceStep, TransactionWarning,
};
pub use typecheck::{
    type_check, ChoiceRef, Diagnostic, PartyRef, TokenRef, TypeCheckContext, TypeCheckResult,
};
