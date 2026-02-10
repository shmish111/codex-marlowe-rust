use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use marlowe_api::analyze::{
    analyze_authorization_safety, analyze_deadline_safety, apply_auto_repair_patch,
    AuthorizationAction, AuthorizationRule, Counterexample, CounterexampleRequest,
    CounterexampleResult,
};
use marlowe_api::ast::{ChoiceId, Party, PayeeTarget, Token};
use marlowe_api::{
    contract_to_yaml_string, parse_contract_yaml, preview_inputs, simulate_transaction_with_trace,
    type_check, ChoiceRef, PartyRef, SimError, SimInput, SimState, SimTransaction,
    SimTransactionResult, TokenRef, TraceReduceRule, TraceStep, TransactionWarning,
    TypeCheckContext,
};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "marlowe-cli", version, about = "Embedded Marlowe contract CLI")]
struct Cli {
    #[arg(long, value_enum, default_value_t = OutputFormat::Json, global = true)]
    format: OutputFormat,
    #[arg(long, default_value = "1.0.0", global = true)]
    schema_version: String,
    #[arg(long, global = true)]
    quiet: bool,
    #[arg(long, global = true)]
    verbose: bool,
    #[arg(long, global = true)]
    strict: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Validate(ValidateCmd),
    Preview(PreviewCmd),
    Step(StepCmd),
    Analyze(AnalyzeCmd),
    Repair(RepairCmd),
    RunPlan(RunPlanCmd),
    Version,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Property {
    DeadlineSafety,
    AuthorizationSafety,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AuthAction {
    Deposit,
    Choice,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("contract_input")
        .args(["in_file", "stdin"])
        .required(true)
))]
struct ContractInput {
    #[arg(long = "in")]
    in_file: Option<PathBuf>,
    #[arg(long)]
    stdin: bool,
}

#[derive(Args, Debug)]
struct ContextFlags {
    #[arg(long)]
    require_known_definitions: bool,
    #[arg(long)]
    known_accounts: Option<String>,
    #[arg(long)]
    known_parties: Option<String>,
    #[arg(long)]
    known_tokens: Option<String>,
    #[arg(long)]
    known_choices: Option<String>,
}

#[derive(Args, Debug)]
struct ValidateCmd {
    #[command(flatten)]
    input: ContractInput,
    #[command(flatten)]
    context: ContextFlags,
}

#[derive(Args, Debug)]
struct PreviewCmd {
    #[command(flatten)]
    input: ContractInput,
    #[arg(long)]
    state: Option<PathBuf>,
    #[arg(long, default_value = "0")]
    interval_start: String,
    #[arg(long, default_value = "0")]
    interval_end: String,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("step_mode")
        .args(["input_json", "input_file", "timeout"])
        .required(true)
))]
struct StepCmd {
    #[command(flatten)]
    input: ContractInput,
    #[arg(long)]
    state: Option<PathBuf>,
    #[arg(long = "input")]
    input_json: Option<String>,
    #[arg(long)]
    input_file: Option<PathBuf>,
    #[arg(long)]
    timeout: Option<String>,
    #[arg(long)]
    trace: bool,
    #[arg(long)]
    out_contract: Option<PathBuf>,
    #[arg(long)]
    out_state: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct AnalyzeCmd {
    #[command(flatten)]
    input: ContractInput,
    #[arg(long, value_enum)]
    property: Property,
    #[arg(long, value_enum)]
    action: Option<AuthAction>,
    #[arg(long)]
    target: Option<String>,
    #[arg(long)]
    allowed_parties: Option<String>,
    #[arg(long, default_value_t = 512)]
    max_nodes: usize,
    #[command(flatten)]
    context: ContextFlags,
}

#[derive(Args, Debug)]
struct RepairCmd {
    #[command(flatten)]
    input: ContractInput,
    #[arg(long, value_enum)]
    property: Property,
    #[arg(long, value_enum)]
    action: Option<AuthAction>,
    #[arg(long)]
    target: Option<String>,
    #[arg(long)]
    allowed_parties: Option<String>,
    #[arg(long)]
    write: Option<PathBuf>,
    #[arg(long)]
    stdout: bool,
    #[arg(long, default_value_t = 512)]
    max_nodes: usize,
}

#[derive(Args, Debug)]
struct RunPlanCmd {
    #[arg(long)]
    plan: PathBuf,
}

#[derive(Serialize)]
struct Envelope {
    schema_version: String,
    command: String,
    status: Status,
    exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<CliError>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ok,
    Error,
}

#[derive(Serialize)]
struct CliError {
    kind: ErrorKind,
    code: String,
    message: String,
    details: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ErrorKind {
    Validation,
    Simulation,
    Analysis,
    Io,
    Internal,
}

#[derive(Debug)]
#[repr(i32)]
enum ExitCodeNum {
    Ok = 0,
    Internal = 1,
    ValidationBlocking = 2,
    SimulationError = 3,
    CounterexampleFound = 4,
    NoDeterministicRepair = 5,
    StrictWarningGate = 6,
}

#[derive(Debug)]
struct Failure {
    kind: ErrorKind,
    code: String,
    message: String,
    details: BTreeMap<String, Value>,
    exit_code: ExitCodeNum,
}

#[derive(Serialize)]
struct ValidationSummary {
    blocking_count: usize,
    warning_count: usize,
    error_count: usize,
    hole_count: usize,
    param_count: usize,
}

#[derive(Serialize)]
struct ValidationExplainItem {
    code: String,
    path: String,
    message: String,
    hint: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    details: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct ValidationDiagnostic {
    message: String,
    path: String,
    code: String,
}

#[derive(Serialize)]
struct ValidateResult {
    valid: bool,
    ready_to_run: bool,
    summary: ValidationSummary,
    blocking: Vec<ValidationExplainItem>,
    warnings: Vec<ValidationExplainItem>,
    diagnostics: Vec<ValidationDiagnostic>,
}

#[derive(Serialize)]
struct SimStateOut {
    min_time: String,
    accounts: Vec<AccountBalanceOut>,
    choices: Vec<ChoiceValueOut>,
    bound_values: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct AccountBalanceOut {
    owner: Party,
    token: Token,
    amount: String,
}

#[derive(Serialize)]
struct ChoiceValueOut {
    id: ChoiceId,
    value: String,
}

#[derive(Serialize)]
struct WarningOut {
    code: String,
    message: String,
    fields: BTreeMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum PreviewInputOut {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        amount: String,
        warnings: Vec<WarningOut>,
    },
    Choice {
        id: ChoiceId,
        bounds: Vec<PreviewBoundOut>,
        warnings: Vec<WarningOut>,
    },
    Notify {
        can_notify: bool,
        warnings: Vec<WarningOut>,
    },
}

#[derive(Serialize)]
struct PreviewBoundOut {
    from: String,
    to: String,
}

#[derive(Serialize)]
struct SimulationContextOut {
    contract_yaml: String,
    state: SimStateOut,
    min_time: String,
}

#[derive(Serialize)]
struct PreviewResultOut {
    summary: String,
    warnings: Vec<WarningOut>,
    inputs: Vec<PreviewInputOut>,
    context: SimulationContextOut,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "code")]
enum TraceEventOut {
    Reduced {
        event_id: String,
        rule: String,
        contract_path: String,
        warning: Option<WarningOut>,
        payment: Option<PaymentOut>,
        delta: Option<StateDeltaOut>,
    },
    InputApplied {
        event_id: String,
        input_index: usize,
        contract_path: String,
        next_contract_path: String,
        input: TraceInputOut,
        warning: Option<WarningOut>,
        delta: Option<StateDeltaOut>,
    },
}

#[derive(Serialize)]
struct PaymentOut {
    from: Party,
    to: PayeeTarget,
    token: Token,
    amount: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum TraceInputOut {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        amount: String,
    },
    Choice {
        id: ChoiceId,
        value: String,
    },
    Notify,
}

#[derive(Serialize)]
struct StateDeltaOut {
    accounts_upserted: Vec<AccountBalanceOut>,
    accounts_removed: Vec<AccountTargetOut>,
    choices_upserted: Vec<ChoiceValueOut>,
    choices_removed: Vec<ChoiceId>,
    bound_values_upserted: BTreeMap<String, String>,
    bound_values_removed: Vec<String>,
    min_time: Option<MinTimeDeltaOut>,
}

#[derive(Serialize)]
struct AccountTargetOut {
    owner: Party,
    token: Token,
}

#[derive(Serialize)]
struct MinTimeDeltaOut {
    before: String,
    after: String,
}

#[derive(Serialize)]
struct StepResultOut {
    summary: String,
    warnings: Vec<WarningOut>,
    trace_events: Vec<TraceEventOut>,
    context: SimulationContextOut,
}

#[derive(Serialize)]
struct AnalyzeResultOut {
    property: String,
    status: String,
    checked_nodes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    counterexample: Option<CounterexampleOut>,
}

#[derive(Serialize)]
struct CounterexampleOut {
    violating_path: String,
    explanation: String,
    steps: Vec<CounterexampleStepOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_repair_patch: Option<AutoRepairPatchOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    witness_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offending_party: Option<Party>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
}

#[derive(Serialize)]
struct CounterexampleStepOut {
    id: String,
    index: usize,
    kind: String,
    severity: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<Party>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time: Option<String>,
    detail: String,
    suggested_fix: String,
}

#[derive(Serialize)]
struct AutoRepairPatchOut {
    kind: String,
    path: String,
    value: String,
    rationale: String,
}

#[derive(Serialize)]
struct RepairResultOut {
    property: String,
    repaired: bool,
    before: AnalyzeResultOut,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<AnalyzeResultOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    patched_contract_yaml: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StateFile {
    #[serde(default)]
    min_time: String,
    #[serde(default)]
    accounts: Vec<AccountBalanceFile>,
    #[serde(default)]
    choices: Vec<ChoiceValueFile>,
    #[serde(default)]
    bound_values: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct AccountBalanceFile {
    owner: Party,
    token: Token,
    amount: String,
}

#[derive(Debug, Deserialize)]
struct ChoiceValueFile {
    id: ChoiceId,
    value: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (command_name, outcome) = dispatch(&cli);

    let (status, exit_code, result, error) = match outcome {
        Ok((exit_code, result)) => (Status::Ok, exit_code as i32, Some(result), None),
        Err(failure) => (
            Status::Error,
            failure.exit_code as i32,
            None,
            Some(CliError {
                kind: failure.kind,
                code: failure.code,
                message: failure.message,
                details: failure.details,
            }),
        ),
    };

    let envelope = Envelope {
        schema_version: cli.schema_version,
        command: command_name,
        status,
        exit_code,
        result,
        error,
    };

    print_envelope(&envelope, cli.format, cli.quiet, cli.verbose);
    ExitCode::from(exit_code as u8)
}

fn dispatch(cli: &Cli) -> (String, Result<(ExitCodeNum, Value), Failure>) {
    match &cli.command {
        Command::Validate(cmd) => ("validate".to_owned(), run_validate(cmd, cli.strict)),
        Command::Preview(cmd) => ("preview".to_owned(), run_preview(cmd)),
        Command::Step(cmd) => ("step".to_owned(), run_step(cmd)),
        Command::Analyze(cmd) => ("analyze".to_owned(), run_analyze(cmd)),
        Command::Repair(cmd) => ("repair".to_owned(), run_repair(cmd)),
        Command::RunPlan(cmd) => ("run_plan".to_owned(), run_plan(cmd)),
        Command::Version => ("version".to_owned(), run_version()),
    }
}

fn run_validate(cmd: &ValidateCmd, strict: bool) -> Result<(ExitCodeNum, Value), Failure> {
    let contract_yaml = read_contract_input(&cmd.input)?;
    let contract = parse_contract_yaml(&contract_yaml).map_err(|err| {
        fail(
            ErrorKind::Validation,
            "ParseError",
            format!("{}: {}", err.path, err.message),
            ExitCodeNum::ValidationBlocking,
        )
    })?;
    let context = map_context_flags(&cmd.context)?;
    let validation = type_check(&contract, &context);

    let mut blocking = Vec::new();
    let mut diagnostics = Vec::new();

    for error in &validation.errors {
        blocking.push(ValidationExplainItem {
            code: "TypeError".to_owned(),
            path: error.path.clone(),
            message: error.message.clone(),
            hint: hint_for_type_error(&error.message),
            details: BTreeMap::new(),
        });
        diagnostics.push(ValidationDiagnostic {
            message: error.message.clone(),
            path: error.path.clone(),
            code: "TypeError".to_owned(),
        });
    }

    for hole in &validation.holes {
        let mut details = BTreeMap::new();
        details.insert("name".to_owned(), hole.name.clone());
        details.insert("type".to_owned(), hole.ty.as_str().to_owned());
        let message = format!("hole '{}' has inferred type {}", hole.name, hole.ty.as_str());
        blocking.push(ValidationExplainItem {
            code: "HoleUnresolved".to_owned(),
            path: hole.path.clone(),
            message: message.clone(),
            hint: format!(
                "Provide a concrete {} value for '?{}' to fully instantiate the contract.",
                hole.ty.as_str(),
                hole.name
            ),
            details,
        });
        diagnostics.push(ValidationDiagnostic {
            message,
            path: hole.path.clone(),
            code: "HoleUnresolved".to_owned(),
        });
    }

    for param in &validation.params {
        let mut details = BTreeMap::new();
        details.insert("name".to_owned(), param.name.clone());
        details.insert("type".to_owned(), param.ty.as_str().to_owned());
        let message = format!(
            "parameter '{}' has inferred type {} and must be instantiated",
            param.name,
            param.ty.as_str()
        );
        blocking.push(ValidationExplainItem {
            code: "ParamUnresolved".to_owned(),
            path: param.path.clone(),
            message: message.clone(),
            hint: format!(
                "Substitute '${}' with a concrete {} value before simulation.",
                param.name,
                param.ty.as_str()
            ),
            details,
        });
        diagnostics.push(ValidationDiagnostic {
            message,
            path: param.path.clone(),
            code: "ParamUnresolved".to_owned(),
        });
    }

    let warnings = validation
        .warnings
        .iter()
        .map(|warning| ValidationExplainItem {
            code: "Warning".to_owned(),
            path: warning.path.clone(),
            message: warning.message.clone(),
            hint: hint_for_warning(&warning.message),
            details: BTreeMap::new(),
        })
        .collect::<Vec<_>>();

    let result = ValidateResult {
        valid: blocking.is_empty(),
        ready_to_run: validation.ready_to_run,
        summary: ValidationSummary {
            blocking_count: blocking.len(),
            warning_count: warnings.len(),
            error_count: validation.errors.len(),
            hole_count: validation.holes.len(),
            param_count: validation.params.len(),
        },
        blocking,
        warnings,
        diagnostics,
    };

    let exit_code = if !result.valid {
        ExitCodeNum::ValidationBlocking
    } else if strict && result.summary.warning_count > 0 {
        ExitCodeNum::StrictWarningGate
    } else {
        ExitCodeNum::Ok
    };

    to_value_result(exit_code, &result)
}

fn run_preview(cmd: &PreviewCmd) -> Result<(ExitCodeNum, Value), Failure> {
    let contract_yaml = read_contract_input(&cmd.input)?;
    let contract = parse_contract_yaml(&contract_yaml).map_err(|err| {
        fail(
            ErrorKind::Validation,
            "ParseError",
            format!("{}: {}", err.path, err.message),
            ExitCodeNum::ValidationBlocking,
        )
    })?;

    let state = read_state_file(cmd.state.as_ref())?;
    let interval_start = parse_bigint(&cmd.interval_start, "interval_start")?;
    let interval_end = parse_bigint(&cmd.interval_end, "interval_end")?;

    let preview = preview_inputs(&contract, &state, &interval_start, &interval_end).map_err(sim_failure)?;
    let contract_yaml = contract_to_yaml_string(&preview.contract)
        .map_err(|err| fail(ErrorKind::Internal, "SerializeError", err.to_string(), ExitCodeNum::Internal))?;

    let result = PreviewResultOut {
        summary: format!("Preview succeeded with {} available input(s)", preview.inputs.len()),
        warnings: preview
            .warnings
            .iter()
            .map(warning_to_output)
            .collect::<Vec<_>>(),
        inputs: preview
            .inputs
            .iter()
            .map(preview_input_to_output)
            .collect::<Vec<_>>(),
        context: SimulationContextOut {
            min_time: preview.state.min_time.to_string(),
            contract_yaml,
            state: state_to_output(&preview.state),
        },
    };

    to_value_result(ExitCodeNum::Ok, &result)
}

fn run_step(cmd: &StepCmd) -> Result<(ExitCodeNum, Value), Failure> {
    let contract_yaml = read_contract_input(&cmd.input)?;
    let contract = parse_contract_yaml(&contract_yaml).map_err(|err| {
        fail(
            ErrorKind::Validation,
            "ParseError",
            format!("{}: {}", err.path, err.message),
            ExitCodeNum::ValidationBlocking,
        )
    })?;

    let state = read_state_file(cmd.state.as_ref())?;

    let transaction = if let Some(timeout) = &cmd.timeout {
        let t = parse_bigint(timeout, "timeout")?;
        SimTransaction {
            interval_start: t.clone(),
            interval_end: t,
            inputs: Vec::new(),
        }
    } else {
        let input = parse_step_input(cmd)?;
        let min_time = state.min_time.clone();
        SimTransaction {
            interval_start: min_time.clone(),
            interval_end: min_time,
            inputs: vec![input],
        }
    };

    let result = simulate_transaction_with_trace(&contract, &state, &transaction, cmd.trace);

    let SimTransactionResult::Success(success) = result else {
        let SimTransactionResult::Error(err) = result else {
            unreachable!();
        };
        return Err(sim_failure(err));
    };

    let next_contract_yaml = contract_to_yaml_string(&success.contract)
        .map_err(|err| fail(ErrorKind::Internal, "SerializeError", err.to_string(), ExitCodeNum::Internal))?;
    let next_state = success.state.clone();

    if let Some(path) = &cmd.out_contract {
        atomic_write(path, next_contract_yaml.as_bytes())?;
    }
    if let Some(path) = &cmd.out_state {
        let json = serde_json::to_vec_pretty(&state_to_output(&next_state)).map_err(|err| {
            fail(
                ErrorKind::Internal,
                "SerializeError",
                err.to_string(),
                ExitCodeNum::Internal,
            )
        })?;
        atomic_write(path, &json)?;
    }

    let step_result = StepResultOut {
        summary: "Simulation step applied".to_owned(),
        warnings: success
            .warnings
            .iter()
            .map(warning_to_output)
            .collect::<Vec<_>>(),
        trace_events: success
            .trace
            .iter()
            .enumerate()
            .map(|(idx, event)| trace_to_output(idx, event))
            .collect::<Vec<_>>(),
        context: SimulationContextOut {
            min_time: next_state.min_time.to_string(),
            contract_yaml: next_contract_yaml,
            state: state_to_output(&next_state),
        },
    };

    to_value_result(ExitCodeNum::Ok, &step_result)
}

fn run_analyze(cmd: &AnalyzeCmd) -> Result<(ExitCodeNum, Value), Failure> {
    let contract_yaml = read_contract_input(&cmd.input)?;
    let contract = parse_contract_yaml(&contract_yaml).map_err(|err| {
        fail(
            ErrorKind::Validation,
            "ParseError",
            format!("{}: {}", err.path, err.message),
            ExitCodeNum::ValidationBlocking,
        )
    })?;

    let validation = type_check(&contract, &TypeCheckContext::default());
    if !validation.errors.is_empty() || !validation.holes.is_empty() || !validation.params.is_empty() {
        return Err(fail(
            ErrorKind::Validation,
            "NotReadyToRun",
            "contract must be fully instantiated and type-safe before analysis".to_owned(),
            ExitCodeNum::ValidationBlocking,
        ));
    }

    let (property_name, result) = run_counterexample_analysis(
        &contract,
        cmd.property,
        cmd.action,
        cmd.target.clone(),
        cmd.allowed_parties.as_ref(),
        cmd.max_nodes,
    )?;

    let out = analyze_result_to_output(property_name, result);
    let exit = if out.status == "counterexample_found" {
        ExitCodeNum::CounterexampleFound
    } else {
        ExitCodeNum::Ok
    };
    to_value_result(exit, &out)
}

fn run_repair(cmd: &RepairCmd) -> Result<(ExitCodeNum, Value), Failure> {
    let contract_yaml = read_contract_input(&cmd.input)?;
    let contract = parse_contract_yaml(&contract_yaml).map_err(|err| {
        fail(
            ErrorKind::Validation,
            "ParseError",
            format!("{}: {}", err.path, err.message),
            ExitCodeNum::ValidationBlocking,
        )
    })?;

    let validation = type_check(&contract, &TypeCheckContext::default());
    if !validation.errors.is_empty() || !validation.holes.is_empty() || !validation.params.is_empty() {
        return Err(fail(
            ErrorKind::Validation,
            "NotReadyToRun",
            "contract must be fully instantiated and type-safe before analysis".to_owned(),
            ExitCodeNum::ValidationBlocking,
        ));
    }

    let (property_name, before_result) = run_counterexample_analysis(
        &contract,
        cmd.property,
        cmd.action,
        cmd.target.clone(),
        cmd.allowed_parties.as_ref(),
        cmd.max_nodes,
    )?;

    let before_out = analyze_result_to_output(property_name, before_result.clone());

    let (repaired, after, patched_contract_yaml, patched_contract) = match before_result {
        CounterexampleResult::PassBounded { .. } => (false, None, None, None),
        CounterexampleResult::Unsupported { path, reason, .. } => {
            return Err(fail_with_detail(
                ErrorKind::Analysis,
                "UnsupportedContractForProperty",
                reason,
                "path",
                Value::String(path),
                ExitCodeNum::Internal,
            ))
        }
        CounterexampleResult::CounterexampleFound { counterexample, .. } => {
            let Some(patch) = counterexample.auto_repair_patch else {
                return Err(fail(
                    ErrorKind::Analysis,
                    "NoDeterministicRepair",
                    "counterexample found but no deterministic safe patch is available".to_owned(),
                    ExitCodeNum::NoDeterministicRepair,
                ));
            };

            let repaired_contract = apply_auto_repair_patch(&contract, &patch).map_err(|err| {
                fail(
                    ErrorKind::Internal,
                    "PatchApplyError",
                    err,
                    ExitCodeNum::Internal,
                )
            })?;
            let yaml = contract_to_yaml_string(&repaired_contract).map_err(|err| {
                fail(
                    ErrorKind::Internal,
                    "SerializeError",
                    err.to_string(),
                    ExitCodeNum::Internal,
                )
            })?;

            let (_, after_result) = run_counterexample_analysis(
                &repaired_contract,
                cmd.property,
                cmd.action,
                cmd.target.clone(),
                cmd.allowed_parties.as_ref(),
                cmd.max_nodes,
            )?;
            (true, Some(analyze_result_to_output(property_name, after_result)), Some(yaml), Some(repaired_contract))
        }
    };

    if let Some(path) = &cmd.write {
        if let Some(yaml) = &patched_contract_yaml {
            atomic_write(path, yaml.as_bytes())?;
        }
    }

    if cmd.stdout {
        if let Some(yaml) = &patched_contract_yaml {
            let mut stdout = io::stdout();
            stdout
                .write_all(yaml.as_bytes())
                .map_err(|err| fail(ErrorKind::Io, "WriteError", err.to_string(), ExitCodeNum::Internal))?;
            stdout
                .write_all(b"\n")
                .map_err(|err| fail(ErrorKind::Io, "WriteError", err.to_string(), ExitCodeNum::Internal))?;
        }
    }

    let result = RepairResultOut {
        property: property_name.to_owned(),
        repaired,
        before: before_out,
        after,
        patched_contract_yaml,
    };

    let exit_code = if repaired {
        ExitCodeNum::Ok
    } else if matches!(patched_contract, None) {
        ExitCodeNum::Ok
    } else {
        ExitCodeNum::NoDeterministicRepair
    };

    to_value_result(exit_code, &result)
}

fn run_plan(cmd: &RunPlanCmd) -> Result<(ExitCodeNum, Value), Failure> {
    let _ = fs::read_to_string(&cmd.plan).map_err(|err| {
        fail_with_detail(
            ErrorKind::Io,
            "ReadError",
            err.to_string(),
            "path",
            Value::String(cmd.plan.display().to_string()),
            ExitCodeNum::Internal,
        )
    })?;

    Err(fail(
        ErrorKind::Internal,
        "NotImplemented",
        "run-plan is not implemented yet".to_owned(),
        ExitCodeNum::Internal,
    ))
}

fn run_version() -> Result<(ExitCodeNum, Value), Failure> {
    let value = serde_json::json!({
        "cli_version": env!("CARGO_PKG_VERSION"),
        "embedded": true,
    });
    Ok((ExitCodeNum::Ok, value))
}

fn to_value_result<T: Serialize>(exit_code: ExitCodeNum, result: &T) -> Result<(ExitCodeNum, Value), Failure> {
    let value = serde_json::to_value(result).map_err(|err| {
        fail(
            ErrorKind::Internal,
            "SerializeError",
            err.to_string(),
            ExitCodeNum::Internal,
        )
    })?;
    Ok((exit_code, value))
}

fn read_contract_input(input: &ContractInput) -> Result<String, Failure> {
    if input.stdin {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|err| fail(ErrorKind::Io, "ReadError", err.to_string(), ExitCodeNum::Internal))?;
        return Ok(buf);
    }

    let path = input
        .in_file
        .as_ref()
        .ok_or_else(|| fail(ErrorKind::Io, "MissingInput", "no contract input provided".to_owned(), ExitCodeNum::Internal))?;
    fs::read_to_string(path).map_err(|err| {
        fail_with_detail(
            ErrorKind::Io,
            "ReadError",
            err.to_string(),
            "path",
            Value::String(path.display().to_string()),
            ExitCodeNum::Internal,
        )
    })
}

fn read_state_file(path: Option<&PathBuf>) -> Result<SimState, Failure> {
    let Some(path) = path else {
        return Ok(SimState::default());
    };

    let content = fs::read_to_string(path).map_err(|err| {
        fail_with_detail(
            ErrorKind::Io,
            "ReadError",
            err.to_string(),
            "path",
            Value::String(path.display().to_string()),
            ExitCodeNum::Internal,
        )
    })?;

    let parsed: StateFile = serde_json::from_str(&content).map_err(|err| {
        fail_with_detail(
            ErrorKind::Io,
            "InvalidStateJson",
            err.to_string(),
            "path",
            Value::String(path.display().to_string()),
            ExitCodeNum::Internal,
        )
    })?;

    map_state_file(parsed)
}

fn map_state_file(file: StateFile) -> Result<SimState, Failure> {
    let min_time = if file.min_time.is_empty() {
        BigInt::from(0)
    } else {
        parse_bigint(&file.min_time, "state.min_time")?
    };

    let mut accounts = BTreeMap::new();
    for account in file.accounts {
        accounts.insert(
            marlowe_api::AccountId {
                owner: account.owner,
                token: account.token,
            },
            parse_bigint(&account.amount, "state.accounts[].amount")?,
        );
    }

    let mut choices = BTreeMap::new();
    for choice in file.choices {
        choices.insert(choice.id, parse_bigint(&choice.value, "state.choices[].value")?);
    }

    let mut bound_values = BTreeMap::new();
    for (name, value) in file.bound_values {
        bound_values.insert(name, parse_bigint(&value, "state.bound_values")?);
    }

    Ok(SimState {
        accounts,
        choices,
        bound_values,
        min_time,
    })
}

fn parse_bigint(value: &str, field_name: &str) -> Result<BigInt, Failure> {
    value.parse::<BigInt>().map_err(|_| {
        fail_with_detail(
            ErrorKind::Validation,
            "InvalidInteger",
            format!("invalid integer '{value}'"),
            "field",
            Value::String(field_name.to_owned()),
            ExitCodeNum::ValidationBlocking,
        )
    })
}

fn resolve_json_arg(arg: &str) -> Result<String, Failure> {
    if let Some(path) = arg.strip_prefix('@') {
        return fs::read_to_string(path).map_err(|err| {
            fail_with_detail(
                ErrorKind::Io,
                "ReadError",
                err.to_string(),
                "path",
                Value::String(path.to_owned()),
                ExitCodeNum::Internal,
            )
        });
    }
    Ok(arg.to_owned())
}

fn map_context_flags(flags: &ContextFlags) -> Result<TypeCheckContext, Failure> {
    let known_accounts: Vec<Party> = parse_vec_flag(flags.known_accounts.as_ref())?;
    let known_parties: Vec<Party> = parse_vec_flag(flags.known_parties.as_ref())?;
    let known_tokens: Vec<Token> = parse_vec_flag(flags.known_tokens.as_ref())?;
    let known_choices: Vec<ChoiceId> = parse_vec_flag(flags.known_choices.as_ref())?;

    let mut mapped_accounts = HashSet::new();
    for (idx, party) in known_accounts.iter().enumerate() {
        mapped_accounts.insert(party_ref_from_party(
            party,
            &format!("known_accounts[{idx}]"),
        )?);
    }

    let mut mapped_parties = HashSet::new();
    for (idx, party) in known_parties.iter().enumerate() {
        mapped_parties.insert(party_ref_from_party(
            party,
            &format!("known_parties[{idx}]"),
        )?);
    }

    let mut mapped_tokens = HashSet::new();
    for (idx, token) in known_tokens.iter().enumerate() {
        mapped_tokens.insert(token_ref_from_token(token, &format!("known_tokens[{idx}]"))?);
    }

    let mut mapped_choices = HashSet::new();
    for (idx, choice) in known_choices.iter().enumerate() {
        mapped_choices.insert(choice_ref_from_choice(choice, &format!("known_choices[{idx}]"))?);
    }

    Ok(TypeCheckContext {
        known_accounts: mapped_accounts,
        known_parties: mapped_parties,
        known_tokens: mapped_tokens,
        known_choices: mapped_choices,
        require_known_definitions: flags.require_known_definitions,
    })
}

fn parse_vec_flag<T: for<'de> Deserialize<'de>>(flag: Option<&String>) -> Result<Vec<T>, Failure> {
    let Some(raw) = flag else {
        return Ok(Vec::new());
    };
    let json = resolve_json_arg(raw)?;
    serde_json::from_str(&json).map_err(|err| {
        fail(
            ErrorKind::Validation,
            "InvalidContextJson",
            err.to_string(),
            ExitCodeNum::ValidationBlocking,
        )
    })
}

fn party_ref_from_party(party: &Party, path: &str) -> Result<PartyRef, Failure> {
    match party {
        Party::Role(name) => Ok(PartyRef::Role(name.clone())),
        Party::Address(name) => Ok(PartyRef::Address(name.clone())),
        Party::Hole(name) => Err(fail_with_detail(
            ErrorKind::Validation,
            "ContextError",
            format!("context definitions must be concrete; found hole '?{name}'"),
            "path",
            Value::String(path.to_owned()),
            ExitCodeNum::ValidationBlocking,
        )),
    }
}

fn token_ref_from_token(token: &Token, path: &str) -> Result<TokenRef, Failure> {
    match token {
        Token::Token {
            currency_symbol,
            token_name,
        } => Ok(TokenRef {
            currency_symbol: currency_symbol.clone(),
            token_name: token_name.clone(),
        }),
        Token::Hole(name) => Err(fail_with_detail(
            ErrorKind::Validation,
            "ContextError",
            format!("context definitions must be concrete; found token hole '?{name}'"),
            "path",
            Value::String(path.to_owned()),
            ExitCodeNum::ValidationBlocking,
        )),
    }
}

fn choice_ref_from_choice(choice: &ChoiceId, path: &str) -> Result<ChoiceRef, Failure> {
    match choice {
        ChoiceId::ChoiceId { name, party } => Ok(ChoiceRef {
            name: name.clone(),
            party: party_ref_from_party(party, &format!("{path}.party"))?,
        }),
        ChoiceId::Hole(name) => Err(fail_with_detail(
            ErrorKind::Validation,
            "ContextError",
            format!("context definitions must be concrete; found choice hole '?{name}'"),
            "path",
            Value::String(path.to_owned()),
            ExitCodeNum::ValidationBlocking,
        )),
    }
}

fn parse_step_input(cmd: &StepCmd) -> Result<SimInput, Failure> {
    let raw = if let Some(json) = &cmd.input_json {
        json.clone()
    } else if let Some(path) = &cmd.input_file {
        fs::read_to_string(path).map_err(|err| {
            fail_with_detail(
                ErrorKind::Io,
                "ReadError",
                err.to_string(),
                "path",
                Value::String(path.display().to_string()),
                ExitCodeNum::Internal,
            )
        })?
    } else {
        return Err(fail(
            ErrorKind::Validation,
            "MissingStepInput",
            "provide --input or --input-file or --timeout".to_owned(),
            ExitCodeNum::ValidationBlocking,
        ));
    };

    let value: Value = serde_json::from_str(&raw).map_err(|err| {
        fail(
            ErrorKind::Validation,
            "InvalidInputJson",
            err.to_string(),
            ExitCodeNum::ValidationBlocking,
        )
    })?;

    map_step_input_value(value)
}

fn map_step_input_value(value: Value) -> Result<SimInput, Failure> {
    if let Value::String(tag) = &value {
        if tag == "notify" {
            return Ok(SimInput::Notify);
        }
    }

    if let Some(obj) = value.as_object() {
        if let Some(inner) = obj.get("deposit") {
            #[derive(Deserialize)]
            struct DepositIn {
                into: Party,
                by: Party,
                token: Token,
                amount: String,
            }
            let dep: DepositIn = serde_json::from_value(inner.clone()).map_err(|err| {
                fail(
                    ErrorKind::Validation,
                    "InvalidInputJson",
                    err.to_string(),
                    ExitCodeNum::ValidationBlocking,
                )
            })?;
            return Ok(SimInput::Deposit {
                into: dep.into,
                by: dep.by,
                token: dep.token,
                amount: parse_bigint(&dep.amount, "input.deposit.amount")?,
            });
        }

        if let Some(inner) = obj.get("choice") {
            #[derive(Deserialize)]
            struct ChoiceIn {
                id: ChoiceId,
                value: String,
            }
            let choice: ChoiceIn = serde_json::from_value(inner.clone()).map_err(|err| {
                fail(
                    ErrorKind::Validation,
                    "InvalidInputJson",
                    err.to_string(),
                    ExitCodeNum::ValidationBlocking,
                )
            })?;
            return Ok(SimInput::Choice {
                id: choice.id,
                value: parse_bigint(&choice.value, "input.choice.value")?,
            });
        }

        if obj.contains_key("notify") {
            return Ok(SimInput::Notify);
        }

        if let Some(kind) = obj.get("kind").and_then(Value::as_str) {
            match kind {
                "notify" => return Ok(SimInput::Notify),
                "deposit" => {
                    let into: Party = serde_json::from_value(
                        obj.get("into").cloned().ok_or_else(|| {
                            fail(
                                ErrorKind::Validation,
                                "InvalidInputJson",
                                "missing field 'into' for kind=deposit".to_owned(),
                                ExitCodeNum::ValidationBlocking,
                            )
                        })?,
                    )
                    .map_err(|err| {
                        fail(
                            ErrorKind::Validation,
                            "InvalidInputJson",
                            err.to_string(),
                            ExitCodeNum::ValidationBlocking,
                        )
                    })?;
                    let by: Party = serde_json::from_value(
                        obj.get("by").cloned().ok_or_else(|| {
                            fail(
                                ErrorKind::Validation,
                                "InvalidInputJson",
                                "missing field 'by' for kind=deposit".to_owned(),
                                ExitCodeNum::ValidationBlocking,
                            )
                        })?,
                    )
                    .map_err(|err| {
                        fail(
                            ErrorKind::Validation,
                            "InvalidInputJson",
                            err.to_string(),
                            ExitCodeNum::ValidationBlocking,
                        )
                    })?;
                    let token: Token = serde_json::from_value(
                        obj.get("token").cloned().ok_or_else(|| {
                            fail(
                                ErrorKind::Validation,
                                "InvalidInputJson",
                                "missing field 'token' for kind=deposit".to_owned(),
                                ExitCodeNum::ValidationBlocking,
                            )
                        })?,
                    )
                    .map_err(|err| {
                        fail(
                            ErrorKind::Validation,
                            "InvalidInputJson",
                            err.to_string(),
                            ExitCodeNum::ValidationBlocking,
                        )
                    })?;
                    let amount = obj
                        .get("amount")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            fail(
                                ErrorKind::Validation,
                                "InvalidInputJson",
                                "missing field 'amount' for kind=deposit".to_owned(),
                                ExitCodeNum::ValidationBlocking,
                            )
                        })?;
                    return Ok(SimInput::Deposit {
                        into,
                        by,
                        token,
                        amount: parse_bigint(amount, "input.amount")?,
                    });
                }
                "choice" => {
                    let id: ChoiceId = serde_json::from_value(
                        obj.get("id").cloned().ok_or_else(|| {
                            fail(
                                ErrorKind::Validation,
                                "InvalidInputJson",
                                "missing field 'id' for kind=choice".to_owned(),
                                ExitCodeNum::ValidationBlocking,
                            )
                        })?,
                    )
                    .map_err(|err| {
                        fail(
                            ErrorKind::Validation,
                            "InvalidInputJson",
                            err.to_string(),
                            ExitCodeNum::ValidationBlocking,
                        )
                    })?;
                    let value_str = obj
                        .get("value")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            fail(
                                ErrorKind::Validation,
                                "InvalidInputJson",
                                "missing field 'value' for kind=choice".to_owned(),
                                ExitCodeNum::ValidationBlocking,
                            )
                        })?;
                    return Ok(SimInput::Choice {
                        id,
                        value: parse_bigint(value_str, "input.value")?,
                    });
                }
                _ => {}
            }
        }
    }

    Err(fail(
        ErrorKind::Validation,
        "InvalidInputJson",
        "unsupported input format; expected notify/deposit/choice".to_owned(),
        ExitCodeNum::ValidationBlocking,
    ))
}

fn run_counterexample_analysis(
    contract: &marlowe_api::ast::Contract,
    property: Property,
    action: Option<AuthAction>,
    target: Option<String>,
    allowed_parties: Option<&String>,
    max_nodes: usize,
) -> Result<(&'static str, CounterexampleResult), Failure> {
    match property {
        Property::DeadlineSafety => Ok((
            "deadline_safety",
            analyze_deadline_safety(contract, &CounterexampleRequest { max_nodes }),
        )),
        Property::AuthorizationSafety => {
            let action = action.ok_or_else(|| {
                fail(
                    ErrorKind::Validation,
                    "MissingAuthorizationRule",
                    "authorization_safety requires --action and --allowed-parties".to_owned(),
                    ExitCodeNum::ValidationBlocking,
                )
            })?;

            let allowed_parties_raw = allowed_parties.ok_or_else(|| {
                fail(
                    ErrorKind::Validation,
                    "MissingAuthorizationRule",
                    "authorization_safety requires --action and --allowed-parties".to_owned(),
                    ExitCodeNum::ValidationBlocking,
                )
            })?;
            let parties_json = resolve_json_arg(allowed_parties_raw)?;
            let parties: Vec<Party> = serde_json::from_str(&parties_json).map_err(|err| {
                fail(
                    ErrorKind::Validation,
                    "InvalidAuthorizationRule",
                    err.to_string(),
                    ExitCodeNum::ValidationBlocking,
                )
            })?;
            if parties.is_empty() {
                return Err(fail(
                    ErrorKind::Validation,
                    "InvalidAuthorizationRule",
                    "authorization_rule.allowed_parties must not be empty".to_owned(),
                    ExitCodeNum::ValidationBlocking,
                ));
            }

            let mapped_action = match action {
                AuthAction::Deposit => AuthorizationAction::Deposit,
                AuthAction::Choice => AuthorizationAction::Choice,
            };
            let rule = AuthorizationRule {
                action: mapped_action,
                target,
                allowed_parties: parties,
            };
            Ok((
                "authorization_safety",
                analyze_authorization_safety(contract, &CounterexampleRequest { max_nodes }, &rule),
            ))
        }
    }
}

fn analyze_result_to_output(property_name: &str, result: CounterexampleResult) -> AnalyzeResultOut {
    match result {
        CounterexampleResult::PassBounded { checked_nodes } => AnalyzeResultOut {
            property: property_name.to_owned(),
            status: "pass_bounded".to_owned(),
            checked_nodes,
            counterexample: None,
        },
        CounterexampleResult::CounterexampleFound {
            checked_nodes,
            counterexample,
        } => AnalyzeResultOut {
            property: property_name.to_owned(),
            status: "counterexample_found".to_owned(),
            checked_nodes,
            counterexample: Some(counterexample_to_output(counterexample)),
        },
        CounterexampleResult::Unsupported {
            checked_nodes,
            path,
            reason,
        } => AnalyzeResultOut {
            property: property_name.to_owned(),
            status: "unsupported".to_owned(),
            checked_nodes,
            counterexample: Some(CounterexampleOut {
                violating_path: path,
                explanation: reason,
                steps: Vec::new(),
                auto_repair_patch: None,
                timeout: None,
                witness_time: None,
                offending_party: None,
                action: None,
                target: None,
            }),
        },
    }
}

fn counterexample_to_output(counterexample: Counterexample) -> CounterexampleOut {
    CounterexampleOut {
        violating_path: counterexample.violating_path,
        explanation: counterexample.explanation,
        steps: counterexample
            .steps
            .into_iter()
            .map(|step| CounterexampleStepOut {
                id: step.id,
                index: step.index,
                kind: step.kind.to_owned(),
                severity: step.severity.to_owned(),
                path: step.path,
                actor: step.actor,
                time: step.time.map(|time| time.to_string()),
                detail: step.detail,
                suggested_fix: step.suggested_fix,
            })
            .collect(),
        auto_repair_patch: counterexample.auto_repair_patch.map(|patch| AutoRepairPatchOut {
            kind: patch.kind.to_owned(),
            path: patch.path,
            value: patch.value,
            rationale: patch.rationale,
        }),
        timeout: counterexample.timeout.map(|value| value.to_string()),
        witness_time: counterexample.witness_time.map(|value| value.to_string()),
        offending_party: counterexample.offending_party,
        action: counterexample.action.map(str::to_owned),
        target: counterexample.target,
    }
}

fn sim_failure(error: SimError) -> Failure {
    let code = sim_error_code(&error);
    let message = sim_error_message(&error);
    let mut details = BTreeMap::new();
    if let Some(path) = sim_error_path(&error) {
        details.insert("path".to_owned(), Value::String(path));
    }
    Failure {
        kind: ErrorKind::Simulation,
        code,
        message,
        details,
        exit_code: ExitCodeNum::SimulationError,
    }
}

fn warning_to_output(warning: &TransactionWarning) -> WarningOut {
    match warning {
        TransactionWarning::NonPositiveDeposit {
            into,
            by,
            token,
            amount,
        } => WarningOut {
            code: "NonPositiveDeposit".to_owned(),
            message: "Non-positive deposit amount".to_owned(),
            fields: btree([
                ("into", json_string(into)),
                ("by", json_string(by)),
                ("token", json_string(token)),
                ("amount", amount.to_string()),
            ]),
        },
        TransactionWarning::NonPositivePay {
            from,
            to,
            token,
            amount,
        } => WarningOut {
            code: "NonPositivePay".to_owned(),
            message: "Non-positive pay amount".to_owned(),
            fields: btree([
                ("from", json_string(from)),
                ("to", json_string(to)),
                ("token", json_string(token)),
                ("amount", amount.to_string()),
            ]),
        },
        TransactionWarning::PartialPay {
            from,
            to,
            token,
            expected,
            paid,
        } => WarningOut {
            code: "PartialPay".to_owned(),
            message: "Partial payment".to_owned(),
            fields: btree([
                ("from", json_string(from)),
                ("to", json_string(to)),
                ("token", json_string(token)),
                ("expected", expected.to_string()),
                ("paid", paid.to_string()),
            ]),
        },
        TransactionWarning::Shadowing { name, old, new } => WarningOut {
            code: "Shadowing".to_owned(),
            message: "Let binding shadowing".to_owned(),
            fields: btree([
                ("name", name.clone()),
                ("old", old.to_string()),
                ("new", new.to_string()),
            ]),
        },
        TransactionWarning::AssertionFailed => WarningOut {
            code: "AssertionFailed".to_owned(),
            message: "Assertion failed".to_owned(),
            fields: BTreeMap::new(),
        },
    }
}

fn preview_input_to_output(input: &marlowe_api::PreviewInput) -> PreviewInputOut {
    match input {
        marlowe_api::PreviewInput::Deposit {
            into,
            by,
            token,
            amount,
        } => PreviewInputOut::Deposit {
            into: into.clone(),
            by: by.clone(),
            token: token.clone(),
            amount: amount.to_string(),
            warnings: if amount <= &BigInt::from(0) {
                vec![WarningOut {
                    code: "NonPositiveDeposit".to_owned(),
                    message: "Non-positive deposit amount".to_owned(),
                    fields: btree([
                        ("into", json_string(into)),
                        ("by", json_string(by)),
                        ("token", json_string(token)),
                        ("amount", amount.to_string()),
                    ]),
                }]
            } else {
                Vec::new()
            },
        },
        marlowe_api::PreviewInput::Choice { id, bounds } => PreviewInputOut::Choice {
            id: id.clone(),
            bounds: bounds
                .iter()
                .map(|(from, to)| PreviewBoundOut {
                    from: from.to_string(),
                    to: to.to_string(),
                })
                .collect(),
            warnings: Vec::new(),
        },
        marlowe_api::PreviewInput::Notify { can_notify } => PreviewInputOut::Notify {
            can_notify: *can_notify,
            warnings: Vec::new(),
        },
    }
}

fn state_to_output(state: &SimState) -> SimStateOut {
    SimStateOut {
        min_time: state.min_time.to_string(),
        accounts: state
            .accounts
            .iter()
            .map(|(account, amount)| AccountBalanceOut {
                owner: account.owner.clone(),
                token: account.token.clone(),
                amount: amount.to_string(),
            })
            .collect(),
        choices: state
            .choices
            .iter()
            .map(|(id, value)| ChoiceValueOut {
                id: id.clone(),
                value: value.to_string(),
            })
            .collect(),
        bound_values: state
            .bound_values
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect(),
    }
}

fn trace_to_output(index: usize, event: &TraceStep) -> TraceEventOut {
    let event_id = format!("trace.{index}");
    match event {
        TraceStep::Reduced {
            rule,
            contract_path,
            warning,
            payment,
            delta,
        } => TraceEventOut::Reduced {
            event_id,
            rule: reduce_rule_to_string(rule.clone()),
            contract_path: contract_path.clone(),
            warning: warning.as_ref().map(warning_to_output),
            payment: payment.as_ref().map(|payment| PaymentOut {
                from: payment.from.clone(),
                to: payment.to.clone(),
                token: payment.token.clone(),
                amount: payment.amount.to_string(),
            }),
            delta: delta.as_ref().map(delta_to_output),
        },
        TraceStep::InputApplied {
            input_index,
            contract_path,
            next_contract_path,
            input,
            warning,
            delta,
        } => TraceEventOut::InputApplied {
            event_id,
            input_index: *input_index,
            contract_path: contract_path.clone(),
            next_contract_path: next_contract_path.clone(),
            input: input_to_output(input),
            warning: warning.as_ref().map(warning_to_output),
            delta: delta.as_ref().map(delta_to_output),
        },
    }
}

fn input_to_output(input: &SimInput) -> TraceInputOut {
    match input {
        SimInput::Deposit {
            into,
            by,
            token,
            amount,
        } => TraceInputOut::Deposit {
            into: into.clone(),
            by: by.clone(),
            token: token.clone(),
            amount: amount.to_string(),
        },
        SimInput::Choice { id, value } => TraceInputOut::Choice {
            id: id.clone(),
            value: value.to_string(),
        },
        SimInput::Notify => TraceInputOut::Notify,
    }
}

fn delta_to_output(delta: &marlowe_api::StateDelta) -> StateDeltaOut {
    StateDeltaOut {
        accounts_upserted: delta
            .accounts_upserted
            .iter()
            .map(|item| AccountBalanceOut {
                owner: item.account.owner.clone(),
                token: item.account.token.clone(),
                amount: item.amount.to_string(),
            })
            .collect(),
        accounts_removed: delta
            .accounts_removed
            .iter()
            .map(|item| AccountTargetOut {
                owner: item.owner.clone(),
                token: item.token.clone(),
            })
            .collect(),
        choices_upserted: delta
            .choices_upserted
            .iter()
            .map(|item| ChoiceValueOut {
                id: item.id.clone(),
                value: item.value.to_string(),
            })
            .collect(),
        choices_removed: delta.choices_removed.clone(),
        bound_values_upserted: delta
            .bound_values_upserted
            .iter()
            .map(|item| (item.name.clone(), item.value.to_string()))
            .collect(),
        bound_values_removed: delta.bound_values_removed.clone(),
        min_time: delta.min_time.as_ref().map(|m| MinTimeDeltaOut {
            before: m.before.to_string(),
            after: m.after.to_string(),
        }),
    }
}

fn reduce_rule_to_string(rule: TraceReduceRule) -> String {
    match rule {
        TraceReduceRule::CloseRefund => "CloseRefund",
        TraceReduceRule::Pay => "Pay",
        TraceReduceRule::IfBranch => "IfBranch",
        TraceReduceRule::WhenTimeout => "WhenTimeout",
        TraceReduceRule::Let => "Let",
        TraceReduceRule::Assert => "Assert",
    }
    .to_owned()
}

fn sim_error_code(error: &SimError) -> String {
    match error {
        SimError::NotReadyToRun => "NotReadyToRun",
        SimError::InvalidInterval { .. } => "InvalidInterval",
        SimError::IntervalInPast { .. } => "IntervalInPast",
        SimError::AmbiguousTimeInterval => "AmbiguousTimeInterval",
        SimError::NoMatchForInput { .. } => "NoMatchForInput",
        SimError::UselessTransaction => "UselessTransaction",
    }
    .to_owned()
}

fn sim_error_message(error: &SimError) -> String {
    match error {
        SimError::NotReadyToRun => {
            "contract must be fully instantiated and type-safe before simulation".to_owned()
        }
        SimError::InvalidInterval { start, end } => {
            format!("invalid interval: start {} is greater than end {}", start, end)
        }
        SimError::IntervalInPast {
            min_time,
            start,
            end,
        } => format!(
            "interval [{}, {}] is before current minimum time {}",
            start, end, min_time
        ),
        SimError::AmbiguousTimeInterval => {
            "interval straddles timeout boundary and is ambiguous".to_owned()
        }
        SimError::NoMatchForInput { input_index } => {
            format!("input at index {} did not match any contract case", input_index)
        }
        SimError::UselessTransaction => "transaction made no state or contract changes".to_owned(),
    }
}

fn sim_error_path(error: &SimError) -> Option<String> {
    match error {
        SimError::NotReadyToRun => Some("$.contract_yaml".to_owned()),
        SimError::InvalidInterval { .. } => Some("$.transaction".to_owned()),
        SimError::IntervalInPast { .. } => Some("$.transaction".to_owned()),
        SimError::AmbiguousTimeInterval => Some("$.transaction".to_owned()),
        SimError::NoMatchForInput { input_index } => Some(format!("$.transaction.inputs[{input_index}]")),
        SimError::UselessTransaction => Some("$.transaction".to_owned()),
    }
}

fn hint_for_type_error(error_message: &str) -> String {
    let message = error_message.to_lowercase();
    if message.contains("undefined let binding") {
        "Declare the referenced `Let.name` in scope before `UseValue`, or rename it to an existing binding.".to_owned()
    } else if message.contains("unknown token") {
        "Use a token known to your context, or extend context definitions to include this token.".to_owned()
    } else if message.contains("unknown account owner") {
        "Use an account owner present in context definitions for account operations.".to_owned()
    } else if message.contains("unknown party") {
        "Use a known party role/address from context definitions.".to_owned()
    } else if message.contains("not declared in contract or context") {
        "Add a matching `Choice` action in the contract, or declare the choice in context.".to_owned()
    } else if message.contains("invalid bound range") {
        "Make sure each choice bound has `from <= to` once values are concrete.".to_owned()
    } else if message.contains("conflicting inferred types") {
        "Keep each repeated hole/parameter name consistent with one DSL type across the contract.".to_owned()
    } else {
        "Adjust the contract expression at this path so it satisfies Marlowe type and scope rules.".to_owned()
    }
}

fn hint_for_warning(warning_message: &str) -> String {
    let message = warning_message.to_lowercase();
    if message.contains("shadows an existing binding") {
        "Rename the inner `Let.name` if the shadowing is accidental.".to_owned()
    } else if message.contains("never used") {
        "Remove the unused let binding, or reference it with `UseValue`.".to_owned()
    } else if message.contains("cannot be statically validated") {
        "Use concrete constants in bounds if you want compile-time bound validation.".to_owned()
    } else {
        "Review this warning and confirm the behavior is intentional.".to_owned()
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|err| {
        fail_with_detail(
            ErrorKind::Io,
            "WriteError",
            err.to_string(),
            "path",
            Value::String(parent.display().to_string()),
            ExitCodeNum::Internal,
        )
    })?;

    let mut tmp_path = path.to_path_buf();
    let suffix = format!(".tmp.{}", std::process::id());
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| format!("{v}{suffix}"))
        .unwrap_or_else(|| suffix.clone());
    tmp_path.set_extension(ext);

    fs::write(&tmp_path, bytes).map_err(|err| {
        fail_with_detail(
            ErrorKind::Io,
            "WriteError",
            err.to_string(),
            "path",
            Value::String(tmp_path.display().to_string()),
            ExitCodeNum::Internal,
        )
    })?;

    fs::rename(&tmp_path, path).map_err(|err| {
        fail_with_detail(
            ErrorKind::Io,
            "WriteError",
            err.to_string(),
            "path",
            Value::String(path.display().to_string()),
            ExitCodeNum::Internal,
        )
    })
}

fn print_envelope(envelope: &Envelope, format: OutputFormat, quiet: bool, verbose: bool) {
    match format {
        OutputFormat::Json => {
            if let Ok(serialized) = serde_json::to_string_pretty(envelope) {
                println!("{serialized}");
            }
            if verbose {
                eprintln!("marlowe-cli: command={} exit_code={}", envelope.command, envelope.exit_code);
            }
        }
        OutputFormat::Text => {
            if !quiet {
                println!("command: {}", envelope.command);
                println!("status: {}", match envelope.status { Status::Ok => "ok", Status::Error => "error"});
                println!("exit_code: {}", envelope.exit_code);
            }
            if let Some(result) = &envelope.result {
                if let Ok(serialized) = serde_json::to_string_pretty(result) {
                    println!("{serialized}");
                }
            }
            if let Some(error) = &envelope.error {
                println!("{}: {}", error.code, error.message);
                if !error.details.is_empty() {
                    if let Ok(serialized) = serde_json::to_string_pretty(&error.details) {
                        println!("{serialized}");
                    }
                }
            }
        }
    }
}

fn json_string<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned())
}

fn btree<const N: usize>(pairs: [(&str, String); N]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (k, v) in pairs {
        map.insert(k.to_owned(), v);
    }
    map
}

fn fail(kind: ErrorKind, code: &str, message: String, exit_code: ExitCodeNum) -> Failure {
    Failure {
        kind,
        code: code.to_owned(),
        message,
        details: BTreeMap::new(),
        exit_code,
    }
}

fn fail_with_detail(
    kind: ErrorKind,
    code: &str,
    message: String,
    detail_key: &str,
    detail_value: Value,
    exit_code: ExitCodeNum,
) -> Failure {
    let mut details = BTreeMap::new();
    details.insert(detail_key.to_owned(), detail_value);
    Failure {
        kind,
        code: code.to_owned(),
        message,
        details,
        exit_code,
    }
}
