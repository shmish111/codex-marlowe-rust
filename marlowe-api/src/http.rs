use std::collections::{BTreeMap, BTreeSet};

use axum::{
    extract::rejection::JsonRejection,
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use utoipa::{OpenApi, ToSchema};

use crate::{
    analyze::{
        analyze_authorization_safety, analyze_deadline_safety, apply_auto_repair_patch,
        AuthorizationAction, AuthorizationRule, Counterexample, CounterexampleRequest,
        CounterexampleResult,
    },
    ast::{ChoiceId, Party, PayeeTarget, Token},
    contract_to_yaml_string, parse_contract_yaml,
    sim::{
        preview_inputs, simulate_transaction_with_trace, AccountId, Payment, PreviewInput,
        SimError, SimInput, SimState, SimTransaction, SimTransactionResult, StateDelta,
        TraceReduceRule, TraceStep, TransactionWarning,
    },
    type_check, ChoiceRef, PartyRef, TokenRef, TypeCheckContext,
};

#[derive(Clone, Default)]
pub struct AppState;

pub fn build_router() -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/openapi.json", get(openapi_handler))
        .route("/analyze/counterexample", post(analyze_counterexample_handler))
        .route("/analyze/apply-repair", post(analyze_apply_repair_handler))
        .route("/simulate/step", post(simulate_step_handler))
        .route("/simulate/preview", post(simulate_preview_handler))
        .route("/typecheck/explain", post(typecheck_explain_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(AppState)
}

#[derive(Serialize, ToSchema)]
struct HealthResponse {
    status: &'static str,
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses((status = 200, description = "Service health", body = HealthResponse))
)]
async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    tag = "health",
    responses((status = 200, description = "OpenAPI document", body = serde_json::Value))
)]
async fn openapi_handler() -> Json<serde_json::Value> {
    let doc = ApiDoc::openapi();
    let value = serde_json::to_value(&doc).unwrap_or_else(|_| serde_json::json!({}));
    Json(value)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SimulateStepRequest {
    pub contract_yaml: String,
    #[serde(default)]
    pub state: SimulateStateRequest,
    pub transaction: SimulateTxRequest,
    #[serde(default)]
    pub trace: bool,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SimulateStateRequest {
    #[serde(default = "zero")]
    #[schema(value_type = String, example = "0")]
    pub min_time: BigIntValue,
    #[serde(default)]
    pub accounts: Vec<AccountBalanceRequest>,
    #[serde(default)]
    pub choices: Vec<ChoiceValueRequest>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub bound_values: BTreeMap<String, BigIntValue>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountBalanceRequest {
    pub owner: Party,
    pub token: Token,
    #[schema(value_type = String, example = "100")]
    pub amount: BigIntValue,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChoiceValueRequest {
    pub id: ChoiceId,
    #[schema(value_type = String, example = "1")]
    pub value: BigIntValue,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SimulateTxRequest {
    #[schema(value_type = String, example = "0")]
    pub interval_start: BigIntValue,
    #[schema(value_type = String, example = "100")]
    pub interval_end: BigIntValue,
    #[serde(default)]
    pub inputs: Vec<SimInputRequest>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SimulatePreviewRequest {
    pub contract_yaml: String,
    #[serde(default)]
    pub state: SimulateStateRequest,
    #[schema(value_type = String, example = "0")]
    pub interval_start: BigIntValue,
    #[schema(value_type = String, example = "100")]
    pub interval_end: BigIntValue,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SimInputRequest {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        #[schema(value_type = String, example = "5")]
        amount: BigIntValue,
    },
    Choice {
        id: ChoiceId,
        #[schema(value_type = String, example = "1")]
        value: BigIntValue,
    },
    Notify,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum BigIntValue {
    Integer(i64),
    String(String),
    #[default]
    Missing,
}

impl BigIntValue {
    fn into_bigint(self) -> Result<BigInt, String> {
        match self {
            BigIntValue::Integer(value) => Ok(BigInt::from(value)),
            BigIntValue::String(value) => value
                .parse::<BigInt>()
                .map_err(|_| format!("invalid integer '{value}'")),
            BigIntValue::Missing => Ok(BigInt::from(0)),
        }
    }
}

fn zero() -> BigIntValue {
    BigIntValue::Integer(0)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SimulateStepResponse {
    pub result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<SimulateSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulateErrorResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SimulateSuccessResponse {
    pub warnings: Vec<WarningResponse>,
    pub payments: Vec<PaymentResponse>,
    pub state: StateResponse,
    pub contract_yaml: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<Vec<TraceEventResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_position: Option<SourceLocationResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SimulateErrorResponse {
    pub code: String,
    pub subcode: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<ErrorDiagnosticResponse>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDiagnosticResponse {
    pub code: String,
    pub subcode: String,
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schema(value_type = Object)]
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "code")]
pub enum WarningResponse {
    NonPositiveDeposit {
        into: Party,
        by: Party,
        token: Token,
        amount: String,
    },
    NonPositivePay {
        from: Party,
        to: PayeeTarget,
        token: Token,
        amount: String,
    },
    PartialPay {
        from: Party,
        to: PayeeTarget,
        token: Token,
        expected: String,
        paid: String,
    },
    Shadowing {
        name: String,
        old: String,
        new: String,
    },
    AssertionFailed {},
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "code")]
pub enum TraceEventResponse {
    Reduced {
        event_id: String,
        rule: String,
        contract_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        column: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_line: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_column: Option<usize>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        state_paths: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        warning: Option<WarningResponse>,
        #[serde(skip_serializing_if = "Option::is_none")]
        payment: Option<PaymentResponse>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delta: Option<TraceStateDeltaResponse>,
    },
    InputApplied {
        event_id: String,
        input_index: usize,
        contract_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        column: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_line: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_column: Option<usize>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        state_paths: Vec<String>,
        input: TraceInputResponse,
        #[serde(skip_serializing_if = "Option::is_none")]
        warning: Option<WarningResponse>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delta: Option<TraceStateDeltaResponse>,
    },
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SourceLocationResponse {
    pub line: usize,
    pub column: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TraceInputResponse {
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

#[derive(Debug, Serialize, ToSchema)]
pub struct TraceStateDeltaResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts_upserted: Vec<AccountBalanceResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts_removed: Vec<AccountTargetResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices_upserted: Vec<ChoiceValueResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices_removed: Vec<ChoiceId>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bound_values_upserted: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bound_values_removed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_time: Option<TraceMinTimeDeltaResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountTargetResponse {
    pub owner: Party,
    pub token: Token,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TraceMinTimeDeltaResponse {
    pub before: String,
    pub after: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PaymentResponse {
    pub from: Party,
    pub to: PayeeTarget,
    pub token: Token,
    pub amount: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StateResponse {
    pub min_time: String,
    pub accounts: Vec<AccountBalanceResponse>,
    pub choices: Vec<ChoiceValueResponse>,
    pub bound_values: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountBalanceResponse {
    pub owner: Party,
    pub token: Token,
    pub amount: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChoiceValueResponse {
    pub id: ChoiceId,
    pub value: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SimulatePreviewResponse {
    pub result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<PreviewSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulateErrorResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PreviewSuccessResponse {
    pub state: StateResponse,
    pub contract_yaml: String,
    pub warnings: Vec<WarningResponse>,
    pub inputs: Vec<PreviewInputResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreviewInputResponse {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        amount: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningResponse>,
    },
    Choice {
        id: ChoiceId,
        bounds: Vec<PreviewBoundResponse>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningResponse>,
    },
    Notify {
        can_notify: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningResponse>,
    },
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PreviewBoundResponse {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TypecheckExplainRequest {
    pub contract_yaml: String,
    #[serde(default)]
    pub context: TypecheckExplainContextRequest,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct TypecheckExplainContextRequest {
    #[serde(default)]
    pub require_known_definitions: bool,
    #[serde(default)]
    pub known_accounts: Vec<Party>,
    #[serde(default)]
    pub known_parties: Vec<Party>,
    #[serde(default)]
    pub known_tokens: Vec<Token>,
    #[serde(default)]
    pub known_choices: Vec<ChoiceId>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TypecheckExplainResponse {
    pub result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<TypecheckExplainSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulateErrorResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TypecheckExplainSuccessResponse {
    pub ready_to_run: bool,
    pub summary: TypecheckExplainSummaryResponse,
    pub blocking: Vec<TypecheckExplainItemResponse>,
    pub warnings: Vec<TypecheckExplainItemResponse>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AnalyzeCounterexampleRequest {
    pub contract_yaml: String,
    #[serde(default)]
    pub property: AnalyzePropertyRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_rule: Option<AuthorizationRuleRequest>,
    #[serde(default = "default_counterexample_bound")]
    pub max_nodes: usize,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzePropertyRequest {
    #[default]
    DeadlineSafety,
    AuthorizationSafety,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct AuthorizationRuleRequest {
    pub action: AuthorizationActionRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub allowed_parties: Vec<Party>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationActionRequest {
    Deposit,
    Choice,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnalyzeCounterexampleResponse {
    pub result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<AnalyzeCounterexampleSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulateErrorResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnalyzeCounterexampleSuccessResponse {
    pub property: &'static str,
    pub status: &'static str,
    pub checked_nodes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterexample: Option<AnalyzeCounterexampleWitnessResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnalyzeCounterexampleWitnessResponse {
    pub violating_path: String,
    pub explanation: String,
    pub steps: Vec<AnalyzeCounterexampleStepResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_repair_patch: Option<AnalyzeAutoRepairPatchResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub witness_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offending_party: Option<Party>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnalyzeCounterexampleStepResponse {
    pub id: String,
    pub index: usize,
    pub kind: String,
    pub severity: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<Party>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    pub detail: String,
    pub suggested_fix: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnalyzeAutoRepairPatchResponse {
    pub kind: String,
    pub path: String,
    pub value: String,
    pub rationale: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AnalyzeApplyRepairRequest {
    pub contract_yaml: String,
    #[serde(default)]
    pub property: AnalyzePropertyRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_rule: Option<AuthorizationRuleRequest>,
    #[serde(default = "default_counterexample_bound")]
    pub max_nodes: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnalyzeApplyRepairResponse {
    pub result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<AnalyzeApplyRepairSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulateErrorResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnalyzeApplyRepairSuccessResponse {
    pub property: &'static str,
    pub repaired: bool,
    pub before: AnalyzeCounterexampleSuccessResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<AnalyzeCounterexampleSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patched_contract_yaml: Option<String>,
}

fn default_counterexample_bound() -> usize {
    512
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TypecheckExplainSummaryResponse {
    pub blocking_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
    pub hole_count: usize,
    pub param_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TypecheckExplainItemResponse {
    pub code: String,
    pub path: String,
    pub message: String,
    pub hint: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schema(value_type = Object)]
    pub details: BTreeMap<String, String>,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health_handler,
        openapi_handler,
        analyze_counterexample_handler,
        analyze_apply_repair_handler,
        simulate_step_handler,
        simulate_preview_handler,
        typecheck_explain_handler
    ),
    components(
        schemas(
            HealthResponse,
            SimulateStepRequest,
            SimulateStateRequest,
            AccountBalanceRequest,
            ChoiceValueRequest,
            SimulateTxRequest,
            SimInputRequest,
            SimulatePreviewRequest,
            BigIntValue,
            SimulateStepResponse,
            SimulateSuccessResponse,
            SimulateErrorResponse,
            ErrorDiagnosticResponse,
            WarningResponse,
            PaymentResponse,
            StateResponse,
            AccountBalanceResponse,
            ChoiceValueResponse,
            SimulatePreviewResponse,
            PreviewSuccessResponse,
            PreviewInputResponse,
            PreviewBoundResponse,
            TypecheckExplainRequest,
            TypecheckExplainContextRequest,
            TypecheckExplainResponse,
            TypecheckExplainSuccessResponse,
            TypecheckExplainSummaryResponse,
            TypecheckExplainItemResponse,
            AnalyzeCounterexampleRequest,
            AnalyzePropertyRequest,
            AuthorizationRuleRequest,
            AuthorizationActionRequest,
            AnalyzeCounterexampleResponse,
            AnalyzeCounterexampleSuccessResponse,
            AnalyzeCounterexampleWitnessResponse,
            AnalyzeCounterexampleStepResponse,
            AnalyzeAutoRepairPatchResponse,
            AnalyzeApplyRepairRequest,
            AnalyzeApplyRepairResponse,
            AnalyzeApplyRepairSuccessResponse,
            Party,
            Token,
            ChoiceId,
            PayeeTarget
        )
    ),
    tags(
        (name = "health", description = "Service health"),
        (name = "simulation", description = "Marlowe simulation APIs")
    )
)]
pub struct ApiDoc;

pub fn openapi_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&ApiDoc::openapi())
}

#[utoipa::path(
    post,
    path = "/analyze/counterexample",
    tag = "simulation",
    request_body = AnalyzeCounterexampleRequest,
    responses(
        (status = 200, description = "Analysis completed", body = AnalyzeCounterexampleResponse),
        (status = 400, description = "Analysis request rejected", body = AnalyzeCounterexampleResponse)
    )
)]
pub async fn analyze_counterexample_handler(
    _state: State<AppState>,
    request: Result<Json<AnalyzeCounterexampleRequest>, JsonRejection>,
) -> (StatusCode, Json<AnalyzeCounterexampleResponse>) {
    let request = match request {
        Ok(Json(request)) => request,
        Err(err) => {
            return analyze_bad_request(
                "RequestError",
                "InvalidJson",
                err.body_text(),
                Some("$.request".to_owned()),
            )
        }
    };

    let contract = match parse_contract_yaml(&request.contract_yaml) {
        Ok(contract) => contract,
        Err(err) => {
            return analyze_bad_request(
                "RequestError",
                "ParseError",
                format!("{}: {}", err.path, err.message),
                Some(err.path),
            )
        }
    };

    let validation = type_check(&contract, &TypeCheckContext::default());
    if !validation.errors.is_empty()
        || !validation.holes.is_empty()
        || !validation.params.is_empty()
    {
        return analyze_bad_request(
            "ValidationError",
            "NotReadyToRun",
            "contract must be fully instantiated and type-safe before analysis".to_owned(),
            Some("$.contract_yaml".to_owned()),
        );
    }

    let (property_name, result) = match run_counterexample_analysis(
        &contract,
        &request.property,
        request.authorization_rule.as_ref(),
        request.max_nodes,
    ) {
        Ok(value) => value,
        Err((code, subcode, message, path)) => {
            return analyze_bad_request(code, subcode, message, path)
        }
    };

    match result {
        CounterexampleResult::PassBounded { checked_nodes } => (
            StatusCode::OK,
            Json(AnalyzeCounterexampleResponse {
                result: "success",
                success: Some(analysis_success_response(property_name, checked_nodes, None)),
                error: None,
            }),
        ),
        CounterexampleResult::CounterexampleFound {
            checked_nodes,
            counterexample,
        } => (
            StatusCode::OK,
            Json(AnalyzeCounterexampleResponse {
                result: "success",
                success: Some(analysis_success_response(
                    counterexample.property,
                    checked_nodes,
                    Some(counterexample),
                )),
                error: None,
            }),
        ),
        CounterexampleResult::Unsupported {
            checked_nodes,
            path,
            reason,
        } => analyze_bad_request(
            "AnalysisError",
            "UnsupportedContractForProperty",
            format!("{reason} (checked_nodes={checked_nodes})"),
            Some(path),
        ),
    }
}

#[utoipa::path(
    post,
    path = "/analyze/apply-repair",
    tag = "simulation",
    request_body = AnalyzeApplyRepairRequest,
    responses(
        (status = 200, description = "Repair workflow completed", body = AnalyzeApplyRepairResponse),
        (status = 400, description = "Repair workflow rejected", body = AnalyzeApplyRepairResponse),
        (status = 500, description = "Internal server error", body = AnalyzeApplyRepairResponse)
    )
)]
pub async fn analyze_apply_repair_handler(
    _state: State<AppState>,
    request: Result<Json<AnalyzeApplyRepairRequest>, JsonRejection>,
) -> (StatusCode, Json<AnalyzeApplyRepairResponse>) {
    let request = match request {
        Ok(Json(request)) => request,
        Err(err) => {
            return analyze_apply_repair_bad_request(
                "RequestError",
                "InvalidJson",
                err.body_text(),
                Some("$.request".to_owned()),
            )
        }
    };

    let contract = match parse_contract_yaml(&request.contract_yaml) {
        Ok(contract) => contract,
        Err(err) => {
            return analyze_apply_repair_bad_request(
                "RequestError",
                "ParseError",
                format!("{}: {}", err.path, err.message),
                Some(err.path),
            )
        }
    };
    let validation = type_check(&contract, &TypeCheckContext::default());
    if !validation.errors.is_empty()
        || !validation.holes.is_empty()
        || !validation.params.is_empty()
    {
        return analyze_apply_repair_bad_request(
            "ValidationError",
            "NotReadyToRun",
            "contract must be fully instantiated and type-safe before analysis".to_owned(),
            Some("$.contract_yaml".to_owned()),
        );
    }

    let (property_name, before_result) = match run_counterexample_analysis(
        &contract,
        &request.property,
        request.authorization_rule.as_ref(),
        request.max_nodes,
    ) {
        Ok(value) => value,
        Err((code, subcode, message, path)) => {
            return analyze_apply_repair_bad_request(code, subcode, message, path)
        }
    };

    let (before_success, maybe_patch) = match before_result {
        CounterexampleResult::PassBounded { checked_nodes } => (
            analysis_success_response(property_name, checked_nodes, None),
            None,
        ),
        CounterexampleResult::CounterexampleFound {
            checked_nodes,
            counterexample,
        } => {
            let patch = counterexample.auto_repair_patch.clone();
            (
                analysis_success_response(counterexample.property, checked_nodes, Some(counterexample)),
                patch,
            )
        }
        CounterexampleResult::Unsupported {
            checked_nodes,
            path,
            reason,
        } => {
            return analyze_apply_repair_bad_request(
                "AnalysisError",
                "UnsupportedContractForProperty",
                format!("{reason} (checked_nodes={checked_nodes})"),
                Some(path),
            )
        }
    };

    let Some(patch) = maybe_patch else {
        return (
            StatusCode::OK,
            Json(AnalyzeApplyRepairResponse {
                result: "success",
                success: Some(AnalyzeApplyRepairSuccessResponse {
                    property: property_name,
                    repaired: false,
                    before: before_success,
                    after: None,
                    patched_contract_yaml: None,
                }),
                error: None,
            }),
        );
    };

    let patched_contract = match apply_auto_repair_patch(&contract, &patch) {
        Ok(contract) => contract,
        Err(err) => {
            return analyze_apply_repair_bad_request(
                "RepairError",
                "PatchApplyFailed",
                err,
                Some("$.counterexample.auto_repair_patch.path".to_owned()),
            )
        }
    };
    let patched_contract_yaml = match contract_to_yaml_string(&patched_contract) {
        Ok(yaml) => yaml,
        Err(err) => {
            return analyze_apply_repair_internal_error(format!(
                "failed to serialize patched contract: {err}"
            ))
        }
    };

    let (_, after_result) = match run_counterexample_analysis(
        &patched_contract,
        &request.property,
        request.authorization_rule.as_ref(),
        request.max_nodes,
    ) {
        Ok(value) => value,
        Err((code, subcode, message, path)) => {
            return analyze_apply_repair_bad_request(code, subcode, message, path)
        }
    };
    let after_success = match after_result {
        CounterexampleResult::PassBounded { checked_nodes } => {
            analysis_success_response(property_name, checked_nodes, None)
        }
        CounterexampleResult::CounterexampleFound {
            checked_nodes,
            counterexample,
        } => analysis_success_response(counterexample.property, checked_nodes, Some(counterexample)),
        CounterexampleResult::Unsupported {
            checked_nodes,
            path,
            reason,
        } => {
            return analyze_apply_repair_bad_request(
                "AnalysisError",
                "UnsupportedContractForProperty",
                format!("{reason} (checked_nodes={checked_nodes})"),
                Some(path),
            )
        }
    };

    (
        StatusCode::OK,
        Json(AnalyzeApplyRepairResponse {
            result: "success",
            success: Some(AnalyzeApplyRepairSuccessResponse {
                property: property_name,
                repaired: true,
                before: before_success,
                after: Some(after_success),
                patched_contract_yaml: Some(patched_contract_yaml),
            }),
            error: None,
        }),
    )
}

#[utoipa::path(
    post,
    path = "/simulate/step",
    tag = "simulation",
    request_body = SimulateStepRequest,
    responses(
        (status = 200, description = "Simulation step succeeded", body = SimulateStepResponse),
        (status = 400, description = "Simulation step rejected", body = SimulateStepResponse),
        (status = 500, description = "Internal server error", body = SimulateStepResponse)
    )
)]
pub async fn simulate_step_handler(
    _state: State<AppState>,
    request: Result<Json<SimulateStepRequest>, JsonRejection>,
) -> (StatusCode, Json<SimulateStepResponse>) {
    let request = match request {
        Ok(Json(request)) => request,
        Err(err) => {
            return bad_request(
                "RequestError",
                "InvalidJson",
                err.body_text(),
                Some("$.request".to_owned()),
                None,
            )
        }
    };
    let contract = match parse_contract_yaml(&request.contract_yaml) {
        Ok(contract) => contract,
        Err(err) => {
            return bad_request(
                "RequestError",
                "ParseError",
                format!("{}: {}", err.path, err.message),
                Some(err.path),
                None,
            )
        }
    };

    let validation = type_check(&contract, &TypeCheckContext::default());
    if !validation.errors.is_empty()
        || !validation.holes.is_empty()
        || !validation.params.is_empty()
    {
        let mut diagnostics = Vec::new();
        for error in &validation.errors {
            diagnostics.push(ErrorDiagnosticResponse {
                code: "Validation".to_owned(),
                subcode: "TypeError".to_owned(),
                path: error.path.clone(),
                message: error.message.clone(),
                line: None,
                column: None,
                end_line: None,
                end_column: None,
                details: BTreeMap::new(),
            });
        }
        for hole in &validation.holes {
            let mut details = BTreeMap::new();
            details.insert("name".to_owned(), hole.name.clone());
            details.insert("type".to_owned(), hole.ty.as_str().to_owned());
            diagnostics.push(ErrorDiagnosticResponse {
                code: "Validation".to_owned(),
                subcode: "HoleUnresolved".to_owned(),
                path: hole.path.clone(),
                message: format!(
                    "hole '{}' has inferred type {}",
                    hole.name,
                    hole.ty.as_str()
                ),
                line: None,
                column: None,
                end_line: None,
                end_column: None,
                details,
            });
        }
        for param in &validation.params {
            let mut details = BTreeMap::new();
            details.insert("name".to_owned(), param.name.clone());
            details.insert("type".to_owned(), param.ty.as_str().to_owned());
            diagnostics.push(ErrorDiagnosticResponse {
                code: "Validation".to_owned(),
                subcode: "ParamUnresolved".to_owned(),
                path: param.path.clone(),
                message: format!(
                    "parameter '{}' has inferred type {} and must be instantiated",
                    param.name,
                    param.ty.as_str()
                ),
                line: None,
                column: None,
                end_line: None,
                end_column: None,
                details,
            });
        }
        return bad_request(
            "ValidationError",
            "NotReadyToRun",
            "contract must be fully instantiated and type-safe before simulation".to_owned(),
            Some("$.contract_yaml".to_owned()),
            Some(diagnostics),
        );
    };

    let state = match map_state_request(request.state) {
        Ok(state) => state,
        Err(message) => {
            return bad_request(
                "RequestError",
                "StateError",
                message,
                Some("$.state".to_owned()),
                None,
            )
        }
    };

    let transaction = match map_tx_request(request.transaction) {
        Ok(transaction) => transaction,
        Err(message) => {
            return bad_request(
                "RequestError",
                "TransactionError",
                message,
                Some("$.transaction".to_owned()),
                None,
            )
        }
    };

    match simulate_transaction_with_trace(&contract, &state, &transaction, request.trace) {
        SimTransactionResult::Success(success) => {
            let contract_yaml = match contract_to_yaml_string(&success.contract) {
                Ok(yaml) => yaml,
                Err(err) => return internal_error(format!("failed to serialize contract: {err}")),
            };

            let trace = if request.trace {
                Some(
                    success
                        .trace
                        .iter()
                        .enumerate()
                        .map(|(idx, step)| {
                            trace_step_to_response(idx, step, &request.contract_yaml)
                        })
                        .collect(),
                )
            } else {
                None
            };

            let initial_position = if request.trace {
                locate_root_contract_span(&request.contract_yaml).map(|span| {
                    SourceLocationResponse {
                        line: span.line,
                        column: span.column,
                        end_line: Some(span.end_line),
                        end_column: Some(span.end_column),
                    }
                })
            } else {
                None
            };

            let response = SimulateStepResponse {
                result: "success",
                success: Some(SimulateSuccessResponse {
                    warnings: success.warnings.iter().map(warning_to_response).collect(),
                    payments: success.payments.iter().map(payment_to_response).collect(),
                    state: state_to_response(&success.state),
                    contract_yaml,
                    trace,
                    initial_position,
                }),
                error: None,
            };
            (StatusCode::OK, Json(response))
        }
        SimTransactionResult::Error(err) => bad_request(
            "SimulationError",
            &sim_error_code(&err),
            sim_error_message(&err),
            sim_error_path(&err),
            None,
        ),
    }
}

#[utoipa::path(
    post,
    path = "/simulate/preview",
    tag = "simulation",
    request_body = SimulatePreviewRequest,
    responses(
        (status = 200, description = "Preview generated", body = SimulatePreviewResponse),
        (status = 400, description = "Preview rejected", body = SimulatePreviewResponse),
        (status = 500, description = "Internal server error", body = SimulatePreviewResponse)
    )
)]
pub async fn simulate_preview_handler(
    _state: State<AppState>,
    request: Result<Json<SimulatePreviewRequest>, JsonRejection>,
) -> (StatusCode, Json<SimulatePreviewResponse>) {
    let request = match request {
        Ok(Json(request)) => request,
        Err(err) => {
            return preview_bad_request(
                "RequestError",
                "InvalidJson",
                err.body_text(),
                Some("$.request".to_owned()),
                None,
            )
        }
    };
    let contract = match parse_contract_yaml(&request.contract_yaml) {
        Ok(contract) => contract,
        Err(err) => {
            return preview_bad_request(
                "RequestError",
                "ParseError",
                format!("{}: {}", err.path, err.message),
                Some(err.path),
                None,
            )
        }
    };

    let validation = type_check(&contract, &TypeCheckContext::default());
    if !validation.errors.is_empty()
        || !validation.holes.is_empty()
        || !validation.params.is_empty()
    {
        let mut diagnostics = Vec::new();
        for error in &validation.errors {
            diagnostics.push(build_preview_diagnostic(
                &request.contract_yaml,
                "Validation",
                "TypeError",
                error.path.clone(),
                error.message.clone(),
                BTreeMap::new(),
            ));
        }
        for hole in &validation.holes {
            let mut details = BTreeMap::new();
            details.insert("name".to_owned(), hole.name.clone());
            details.insert("type".to_owned(), hole.ty.as_str().to_owned());
            diagnostics.push(build_preview_diagnostic(
                &request.contract_yaml,
                "Validation",
                "HoleUnresolved",
                hole.path.clone(),
                format!(
                    "hole '{}' has inferred type {}",
                    hole.name,
                    hole.ty.as_str()
                ),
                details,
            ));
        }
        for param in &validation.params {
            let mut details = BTreeMap::new();
            details.insert("name".to_owned(), param.name.clone());
            details.insert("type".to_owned(), param.ty.as_str().to_owned());
            diagnostics.push(build_preview_diagnostic(
                &request.contract_yaml,
                "Validation",
                "ParamUnresolved",
                param.path.clone(),
                format!(
                    "parameter '{}' has inferred type {} and must be instantiated",
                    param.name,
                    param.ty.as_str()
                ),
                details,
            ));
        }
        return preview_bad_request(
            "ValidationError",
            "NotReadyToRun",
            "contract must be fully instantiated and type-safe before simulation".to_owned(),
            Some("$.contract_yaml".to_owned()),
            Some(diagnostics),
        );
    };

    let state = match map_state_request(request.state) {
        Ok(state) => state,
        Err(message) => {
            return preview_bad_request(
                "RequestError",
                "StateError",
                message,
                Some("$.state".to_owned()),
                None,
            )
        }
    };
    let interval_start = match request.interval_start.into_bigint() {
        Ok(value) => value,
        Err(message) => {
            return preview_bad_request(
                "RequestError",
                "TransactionError",
                message,
                Some("$.interval_start".to_owned()),
                None,
            )
        }
    };
    let interval_end = match request.interval_end.into_bigint() {
        Ok(value) => value,
        Err(message) => {
            return preview_bad_request(
                "RequestError",
                "TransactionError",
                message,
                Some("$.interval_end".to_owned()),
                None,
            )
        }
    };

    match preview_inputs(&contract, &state, &interval_start, &interval_end) {
        Ok(preview) => {
            let contract_yaml = match contract_to_yaml_string(&preview.contract) {
                Ok(yaml) => yaml,
                Err(err) => {
                    return preview_internal_error(format!("failed to serialize contract: {err}"))
                }
            };

            (
                StatusCode::OK,
                Json(SimulatePreviewResponse {
                    result: "success",
                    success: Some(PreviewSuccessResponse {
                        state: state_to_response(&preview.state),
                        contract_yaml,
                        warnings: preview.warnings.iter().map(warning_to_response).collect(),
                        inputs: preview
                            .inputs
                            .iter()
                            .map(preview_input_to_response)
                            .collect(),
                    }),
                    error: None,
                }),
            )
        }
        Err(err) => preview_bad_request(
            "SimulationError",
            &sim_error_code(&err),
            sim_error_message(&err),
            sim_error_path(&err),
            None,
        ),
    }
}

#[utoipa::path(
    post,
    path = "/typecheck/explain",
    tag = "simulation",
    request_body = TypecheckExplainRequest,
    responses(
        (status = 200, description = "Typecheck explanation generated", body = TypecheckExplainResponse),
        (status = 400, description = "Explain request rejected", body = TypecheckExplainResponse)
    )
)]
pub async fn typecheck_explain_handler(
    _state: State<AppState>,
    request: Result<Json<TypecheckExplainRequest>, JsonRejection>,
) -> (StatusCode, Json<TypecheckExplainResponse>) {
    let request = match request {
        Ok(Json(request)) => request,
        Err(err) => {
            return typecheck_explain_bad_request(
                "RequestError",
                "InvalidJson",
                err.body_text(),
                Some("$.request".to_owned()),
                None,
            )
        }
    };
    let context = match map_typecheck_context_request(&request.context) {
        Ok(context) => context,
        Err(err) => {
            return typecheck_explain_bad_request(
                "RequestError",
                "ContextError",
                err.message,
                Some(err.path),
                None,
            )
        }
    };

    let contract = match parse_contract_yaml(&request.contract_yaml) {
        Ok(contract) => contract,
        Err(err) => {
            return typecheck_explain_bad_request(
                "RequestError",
                "ParseError",
                format!("{}: {}", err.path, err.message),
                Some(err.path),
                None,
            )
        }
    };

    let validation = type_check(&contract, &context);
    let error_count = validation.errors.len();
    let hole_count = validation.holes.len();
    let param_count = validation.params.len();
    let warning_count = validation.warnings.len();

    let mut blocking = Vec::new();
    for error in &validation.errors {
        blocking.push(TypecheckExplainItemResponse {
            code: "TypeError".to_owned(),
            path: error.path.clone(),
            message: error.message.clone(),
            hint: hint_for_type_error(&error.message),
            details: BTreeMap::new(),
        });
    }
    for hole in &validation.holes {
        let mut details = BTreeMap::new();
        details.insert("name".to_owned(), hole.name.clone());
        details.insert("type".to_owned(), hole.ty.as_str().to_owned());
        blocking.push(TypecheckExplainItemResponse {
            code: "HoleUnresolved".to_owned(),
            path: hole.path.clone(),
            message: format!(
                "hole '{}' has inferred type {}",
                hole.name,
                hole.ty.as_str()
            ),
            hint: format!(
                "Provide a concrete {} value for '?{}' to fully instantiate the contract.",
                hole.ty.as_str(),
                hole.name
            ),
            details,
        });
    }
    for param in &validation.params {
        let mut details = BTreeMap::new();
        details.insert("name".to_owned(), param.name.clone());
        details.insert("type".to_owned(), param.ty.as_str().to_owned());
        blocking.push(TypecheckExplainItemResponse {
            code: "ParamUnresolved".to_owned(),
            path: param.path.clone(),
            message: format!(
                "parameter '{}' has inferred type {} and must be instantiated",
                param.name,
                param.ty.as_str()
            ),
            hint: format!(
                "Substitute '${}' with a concrete {} value before simulation.",
                param.name,
                param.ty.as_str()
            ),
            details,
        });
    }

    let mut warnings = Vec::new();
    for warning in &validation.warnings {
        warnings.push(TypecheckExplainItemResponse {
            code: "Warning".to_owned(),
            path: warning.path.clone(),
            message: warning.message.clone(),
            hint: hint_for_warning(&warning.message),
            details: BTreeMap::new(),
        });
    }

    (
        StatusCode::OK,
        Json(TypecheckExplainResponse {
            result: "success",
            success: Some(TypecheckExplainSuccessResponse {
                ready_to_run: validation.ready_to_run,
                summary: TypecheckExplainSummaryResponse {
                    blocking_count: blocking.len(),
                    warning_count,
                    error_count,
                    hole_count,
                    param_count,
                },
                blocking,
                warnings,
            }),
            error: None,
        }),
    )
}

fn map_state_request(request: SimulateStateRequest) -> Result<SimState, String> {
    let mut accounts = BTreeMap::new();
    for account in request.accounts {
        accounts.insert(
            AccountId {
                owner: account.owner,
                token: account.token,
            },
            account.amount.into_bigint()?,
        );
    }

    let mut choices = BTreeMap::new();
    for choice in request.choices {
        choices.insert(choice.id, choice.value.into_bigint()?);
    }

    let mut bound_values = BTreeMap::new();
    for (name, value) in request.bound_values {
        bound_values.insert(name, value.into_bigint()?);
    }

    Ok(SimState {
        accounts,
        choices,
        bound_values,
        min_time: request.min_time.into_bigint()?,
    })
}

fn map_tx_request(request: SimulateTxRequest) -> Result<SimTransaction, String> {
    let mut inputs = Vec::with_capacity(request.inputs.len());
    for input in request.inputs {
        let mapped = match input {
            SimInputRequest::Deposit {
                into,
                by,
                token,
                amount,
            } => SimInput::Deposit {
                into,
                by,
                token,
                amount: amount.into_bigint()?,
            },
            SimInputRequest::Choice { id, value } => SimInput::Choice {
                id,
                value: value.into_bigint()?,
            },
            SimInputRequest::Notify => SimInput::Notify,
        };
        inputs.push(mapped);
    }

    Ok(SimTransaction {
        interval_start: request.interval_start.into_bigint()?,
        interval_end: request.interval_end.into_bigint()?,
        inputs,
    })
}

fn warning_to_response(warning: &TransactionWarning) -> WarningResponse {
    match warning {
        TransactionWarning::NonPositiveDeposit {
            into,
            by,
            token,
            amount,
        } => WarningResponse::NonPositiveDeposit {
            into: into.clone(),
            by: by.clone(),
            token: token.clone(),
            amount: amount.to_string(),
        },
        TransactionWarning::NonPositivePay {
            from,
            to,
            token,
            amount,
        } => WarningResponse::NonPositivePay {
            from: from.clone(),
            to: to.clone(),
            token: token.clone(),
            amount: amount.to_string(),
        },
        TransactionWarning::PartialPay {
            from,
            to,
            token,
            expected,
            paid,
        } => WarningResponse::PartialPay {
            from: from.clone(),
            to: to.clone(),
            token: token.clone(),
            expected: expected.to_string(),
            paid: paid.to_string(),
        },
        TransactionWarning::Shadowing { name, old, new } => WarningResponse::Shadowing {
            name: name.clone(),
            old: old.to_string(),
            new: new.to_string(),
        },
        TransactionWarning::AssertionFailed => WarningResponse::AssertionFailed {},
    }
}

fn trace_step_to_response(
    index: usize,
    step: &TraceStep,
    contract_source: &str,
) -> TraceEventResponse {
    let event_id = format!("trace-{index:04}");
    match step {
        TraceStep::Reduced {
            rule,
            contract_path,
            warning,
            payment,
            delta,
        } => {
            let span = trace_step_span_for_reduced(contract_source, contract_path, rule);
            TraceEventResponse::Reduced {
                line: span.map(|s| s.line),
                column: span.map(|s| s.column),
                end_line: span.map(|s| s.end_line),
                end_column: span.map(|s| s.end_column),
                event_id,
                rule: trace_rule_name(rule).to_owned(),
                contract_path: contract_path.clone(),
                state_paths: trace_state_paths(delta.as_ref()),
                warning: warning.as_ref().map(warning_to_response),
                payment: payment.as_ref().map(payment_to_response),
                delta: delta.as_ref().map(state_delta_to_response),
            }
        }
        TraceStep::InputApplied {
            input_index,
            contract_path,
            next_contract_path,
            input,
            warning,
            delta,
        } => {
            let span = trace_step_span_for_input(contract_source, next_contract_path);
            TraceEventResponse::InputApplied {
                line: span.map(|s| s.line),
                column: span.map(|s| s.column),
                end_line: span.map(|s| s.end_line),
                end_column: span.map(|s| s.end_column),
                event_id,
                input_index: *input_index,
                contract_path: contract_path.clone(),
                state_paths: trace_state_paths(delta.as_ref()),
                input: trace_input_to_response(input),
                warning: warning.as_ref().map(warning_to_response),
                delta: delta.as_ref().map(state_delta_to_response),
            }
        }
    }
}

fn trace_step_span_for_reduced(
    source: &str,
    contract_path: &str,
    rule: &TraceReduceRule,
) -> Option<SourceSpan> {
    let constructor = match rule {
        TraceReduceRule::CloseRefund => "Close",
        TraceReduceRule::Pay => "Pay",
        TraceReduceRule::IfBranch => "If",
        TraceReduceRule::WhenTimeout => "When",
        TraceReduceRule::Let => "Let",
        TraceReduceRule::Assert => "Assert",
    };
    find_constructor_span(source, contract_path, constructor)
}

fn trace_step_span_for_input(source: &str, continuation_path: &str) -> Option<SourceSpan> {
    find_path_key_span(source, continuation_path)
}

fn trace_state_paths(delta: Option<&StateDelta>) -> Vec<String> {
    let Some(delta) = delta else {
        return Vec::new();
    };

    let mut paths = BTreeSet::new();
    if !delta.accounts_upserted.is_empty() || !delta.accounts_removed.is_empty() {
        paths.insert("$.accounts".to_owned());
    }
    if !delta.choices_upserted.is_empty() || !delta.choices_removed.is_empty() {
        paths.insert("$.choices".to_owned());
    }
    for entry in &delta.bound_values_upserted {
        paths.insert(format!("$.bound_values.{}", entry.name));
    }
    for entry in &delta.bound_values_removed {
        paths.insert(format!("$.bound_values.{entry}"));
    }
    if delta.min_time.is_some() {
        paths.insert("$.min_time".to_owned());
    }

    paths.into_iter().collect()
}

fn state_delta_to_response(delta: &StateDelta) -> TraceStateDeltaResponse {
    TraceStateDeltaResponse {
        accounts_upserted: delta
            .accounts_upserted
            .iter()
            .map(|entry| AccountBalanceResponse {
                owner: entry.account.owner.clone(),
                token: entry.account.token.clone(),
                amount: entry.amount.to_string(),
            })
            .collect(),
        accounts_removed: delta
            .accounts_removed
            .iter()
            .map(|account| AccountTargetResponse {
                owner: account.owner.clone(),
                token: account.token.clone(),
            })
            .collect(),
        choices_upserted: delta
            .choices_upserted
            .iter()
            .map(|entry| ChoiceValueResponse {
                id: entry.id.clone(),
                value: entry.value.to_string(),
            })
            .collect(),
        choices_removed: delta.choices_removed.clone(),
        bound_values_upserted: delta
            .bound_values_upserted
            .iter()
            .map(|entry| (entry.name.clone(), entry.value.to_string()))
            .collect(),
        bound_values_removed: delta.bound_values_removed.clone(),
        min_time: delta
            .min_time
            .as_ref()
            .map(|entry| TraceMinTimeDeltaResponse {
                before: entry.before.to_string(),
                after: entry.after.to_string(),
            }),
    }
}

fn trace_rule_name(rule: &TraceReduceRule) -> &'static str {
    match rule {
        TraceReduceRule::CloseRefund => "CloseRefund",
        TraceReduceRule::Pay => "Pay",
        TraceReduceRule::IfBranch => "IfBranch",
        TraceReduceRule::WhenTimeout => "WhenTimeout",
        TraceReduceRule::Let => "Let",
        TraceReduceRule::Assert => "Assert",
    }
}

fn trace_input_to_response(input: &SimInput) -> TraceInputResponse {
    match input {
        SimInput::Deposit {
            into,
            by,
            token,
            amount,
        } => TraceInputResponse::Deposit {
            into: into.clone(),
            by: by.clone(),
            token: token.clone(),
            amount: amount.to_string(),
        },
        SimInput::Choice { id, value } => TraceInputResponse::Choice {
            id: id.clone(),
            value: value.to_string(),
        },
        SimInput::Notify => TraceInputResponse::Notify,
    }
}

fn payment_to_response(payment: &Payment) -> PaymentResponse {
    PaymentResponse {
        from: payment.from.clone(),
        to: payment.to.clone(),
        token: payment.token.clone(),
        amount: payment.amount.to_string(),
    }
}

fn state_to_response(state: &SimState) -> StateResponse {
    StateResponse {
        min_time: state.min_time.to_string(),
        accounts: state
            .accounts
            .iter()
            .map(|(account, amount)| AccountBalanceResponse {
                owner: account.owner.clone(),
                token: account.token.clone(),
                amount: amount.to_string(),
            })
            .collect(),
        choices: state
            .choices
            .iter()
            .map(|(id, value)| ChoiceValueResponse {
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

fn preview_input_to_response(input: &PreviewInput) -> PreviewInputResponse {
    match input {
        PreviewInput::Deposit {
            into,
            by,
            token,
            amount,
        } => PreviewInputResponse::Deposit {
            into: into.clone(),
            by: by.clone(),
            token: token.clone(),
            amount: amount.to_string(),
            warnings: if amount <= &BigInt::from(0) {
                vec![WarningResponse::NonPositiveDeposit {
                    into: into.clone(),
                    by: by.clone(),
                    token: token.clone(),
                    amount: amount.to_string(),
                }]
            } else {
                Vec::new()
            },
        },
        PreviewInput::Choice { id, bounds } => PreviewInputResponse::Choice {
            id: id.clone(),
            bounds: bounds
                .iter()
                .map(|(from, to)| PreviewBoundResponse {
                    from: from.to_string(),
                    to: to.to_string(),
                })
                .collect(),
            warnings: Vec::new(),
        },
        PreviewInput::Notify { can_notify } => PreviewInputResponse::Notify {
            can_notify: *can_notify,
            warnings: Vec::new(),
        },
    }
}

#[derive(Debug, Clone, Copy)]
struct SourceSpan {
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
}

fn build_preview_diagnostic(
    contract_yaml: &str,
    code: &str,
    subcode: &str,
    path: String,
    message: String,
    details: BTreeMap<String, String>,
) -> ErrorDiagnosticResponse {
    let span = locate_diagnostic_span(contract_yaml, &path, subcode, &details);
    ErrorDiagnosticResponse {
        code: code.to_owned(),
        subcode: subcode.to_owned(),
        path,
        message,
        line: span.map(|s| s.line),
        column: span.map(|s| s.column),
        end_line: span.map(|s| s.end_line),
        end_column: span.map(|s| s.end_column),
        details,
    }
}

fn locate_diagnostic_span(
    source: &str,
    path: &str,
    subcode: &str,
    details: &BTreeMap<String, String>,
) -> Option<SourceSpan> {
    if subcode == "HoleUnresolved" {
        if let Some(name) = details.get("name") {
            if let Some(span) = find_token_span(source, &format!("?{name}")) {
                return Some(span);
            }
        }
    }
    if subcode == "ParamUnresolved" {
        if let Some(name) = details.get("name") {
            if let Some(span) = find_token_span(source, &format!("${name}")) {
                return Some(span);
            }
        }
    }
    find_path_key_span(source, path)
}

fn find_token_span(source: &str, token: &str) -> Option<SourceSpan> {
    for (line_idx, line) in source.lines().enumerate() {
        if let Some(col_idx) = line.find(token) {
            return Some(SourceSpan {
                line: line_idx + 1,
                column: col_idx + 1,
                end_line: line_idx + 1,
                end_column: col_idx + token.len(),
            });
        }
    }
    None
}

fn find_path_key_span(source: &str, path: &str) -> Option<SourceSpan> {
    let keys = path_keys(path);
    let key = keys.last()?;
    let token = format!("{key}:");
    let lines: Vec<&str> = source.lines().collect();
    let mut best: Option<(usize, usize)> = None;
    let anchors: Vec<&str> = keys
        .iter()
        .take(keys.len().saturating_sub(1))
        .map(String::as_str)
        .collect();

    for (line_idx, line) in lines.iter().enumerate() {
        if let Some(col_idx) = line.find(&token) {
            let window_start = line_idx.saturating_sub(40);
            let score = anchors
                .iter()
                .filter(|anchor| {
                    let anchor_token = format!("{}:", anchor);
                    lines[window_start..=line_idx]
                        .iter()
                        .any(|candidate| candidate.contains(&anchor_token))
                })
                .count();
            match best {
                None => best = Some((line_idx, col_idx)),
                Some((best_line, _)) => {
                    let best_window_start = best_line.saturating_sub(40);
                    let best_score = anchors
                        .iter()
                        .filter(|anchor| {
                            let anchor_token = format!("{}:", anchor);
                            lines[best_window_start..=best_line]
                                .iter()
                                .any(|candidate| candidate.contains(&anchor_token))
                        })
                        .count();
                    if score > best_score {
                        best = Some((line_idx, col_idx));
                    }
                }
            }
        }
    }

    best.map(|(line_idx, col_idx)| SourceSpan {
        line: line_idx + 1,
        column: col_idx + 1,
        end_line: line_idx + 1,
        end_column: col_idx + key.len(),
    })
}

fn path_keys(path: &str) -> Vec<String> {
    path.strip_prefix("$.")
        .unwrap_or(path)
        .split('.')
        .filter_map(|segment| {
            let key = segment.split('[').next().unwrap_or("");
            if key.is_empty() {
                None
            } else {
                Some(key.to_owned())
            }
        })
        .collect()
}

fn locate_root_contract_span(source: &str) -> Option<SourceSpan> {
    let constructors = ["Close", "Pay", "If", "When", "Let", "Assert"];
    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        for constructor in constructors {
            let token = format!("{constructor}:");
            if trimmed.starts_with(&token) {
                let indent = line.len() - trimmed.len();
                return Some(SourceSpan {
                    line: line_idx + 1,
                    column: indent + 1,
                    end_line: line_idx + 1,
                    end_column: indent + constructor.len(),
                });
            }
        }
    }
    None
}

fn find_constructor_span(source: &str, path: &str, constructor: &str) -> Option<SourceSpan> {
    let token = format!("{constructor}:");
    let keys = path_keys(path);
    let anchors: Vec<&str> = keys.iter().map(String::as_str).collect();
    let lines: Vec<&str> = source.lines().collect();
    let mut best: Option<(usize, usize, usize)> = None;

    for (line_idx, line) in lines.iter().enumerate() {
        if let Some(col_idx) = line.find(&token) {
            let window_start = line_idx.saturating_sub(60);
            let score = anchors
                .iter()
                .filter(|anchor| {
                    let anchor_token = format!("{}:", anchor);
                    lines[window_start..=line_idx]
                        .iter()
                        .any(|candidate| candidate.contains(&anchor_token))
                })
                .count();
            match best {
                None => best = Some((score, line_idx, col_idx)),
                Some((best_score, _, _)) if score > best_score => {
                    best = Some((score, line_idx, col_idx))
                }
                _ => {}
            }
        }
    }

    best.map(|(_, line_idx, col_idx)| SourceSpan {
        line: line_idx + 1,
        column: col_idx + 1,
        end_line: line_idx + 1,
        end_column: col_idx + constructor.len(),
    })
}

struct ContextMapError {
    path: String,
    message: String,
}

fn map_typecheck_context_request(
    request: &TypecheckExplainContextRequest,
) -> Result<TypeCheckContext, ContextMapError> {
    let mut known_accounts = std::collections::HashSet::new();
    for (idx, party) in request.known_accounts.iter().enumerate() {
        known_accounts.insert(party_ref_from_request(
            party,
            &format!("$.context.known_accounts[{idx}]"),
        )?);
    }

    let mut known_parties = std::collections::HashSet::new();
    for (idx, party) in request.known_parties.iter().enumerate() {
        known_parties.insert(party_ref_from_request(
            party,
            &format!("$.context.known_parties[{idx}]"),
        )?);
    }

    let mut known_tokens = std::collections::HashSet::new();
    for (idx, token) in request.known_tokens.iter().enumerate() {
        known_tokens.insert(token_ref_from_request(
            token,
            &format!("$.context.known_tokens[{idx}]"),
        )?);
    }

    let mut known_choices = std::collections::HashSet::new();
    for (idx, choice) in request.known_choices.iter().enumerate() {
        known_choices.insert(choice_ref_from_request(
            choice,
            &format!("$.context.known_choices[{idx}]"),
        )?);
    }

    Ok(TypeCheckContext {
        known_accounts,
        known_parties,
        known_tokens,
        known_choices,
        require_known_definitions: request.require_known_definitions,
    })
}

fn party_ref_from_request(party: &Party, path: &str) -> Result<PartyRef, ContextMapError> {
    match party {
        Party::Role(name) => Ok(PartyRef::Role(name.clone())),
        Party::Address(name) => Ok(PartyRef::Address(name.clone())),
        Party::Hole(name) => Err(ContextMapError {
            path: path.to_owned(),
            message: format!("context definitions must be concrete; found hole '?{name}'"),
        }),
    }
}

fn token_ref_from_request(token: &Token, path: &str) -> Result<TokenRef, ContextMapError> {
    match token {
        Token::Token {
            currency_symbol,
            token_name,
        } => Ok(TokenRef {
            currency_symbol: currency_symbol.clone(),
            token_name: token_name.clone(),
        }),
        Token::Hole(name) => Err(ContextMapError {
            path: path.to_owned(),
            message: format!("context definitions must be concrete; found token hole '?{name}'"),
        }),
    }
}

fn choice_ref_from_request(choice: &ChoiceId, path: &str) -> Result<ChoiceRef, ContextMapError> {
    match choice {
        ChoiceId::ChoiceId { name, party } => Ok(ChoiceRef {
            name: name.clone(),
            party: party_ref_from_request(party, &format!("{path}.party"))?,
        }),
        ChoiceId::Hole(name) => Err(ContextMapError {
            path: path.to_owned(),
            message: format!("context definitions must be concrete; found choice hole '?{name}'"),
        }),
    }
}

fn bad_request(
    code: &str,
    subcode: &str,
    message: String,
    path: Option<String>,
    diagnostics: Option<Vec<ErrorDiagnosticResponse>>,
) -> (StatusCode, Json<SimulateStepResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(SimulateStepResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: code.to_owned(),
                subcode: subcode.to_owned(),
                message,
                path,
                diagnostics,
            }),
        }),
    )
}

fn internal_error(message: String) -> (StatusCode, Json<SimulateStepResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(SimulateStepResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: "InternalError".to_owned(),
                subcode: "InternalError".to_owned(),
                message,
                path: None,
                diagnostics: None,
            }),
        }),
    )
}

fn preview_bad_request(
    code: &str,
    subcode: &str,
    message: String,
    path: Option<String>,
    diagnostics: Option<Vec<ErrorDiagnosticResponse>>,
) -> (StatusCode, Json<SimulatePreviewResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(SimulatePreviewResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: code.to_owned(),
                subcode: subcode.to_owned(),
                message,
                path,
                diagnostics,
            }),
        }),
    )
}

fn preview_internal_error(message: String) -> (StatusCode, Json<SimulatePreviewResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(SimulatePreviewResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: "InternalError".to_owned(),
                subcode: "InternalError".to_owned(),
                message,
                path: None,
                diagnostics: None,
            }),
        }),
    )
}

fn typecheck_explain_bad_request(
    code: &str,
    subcode: &str,
    message: String,
    path: Option<String>,
    diagnostics: Option<Vec<ErrorDiagnosticResponse>>,
) -> (StatusCode, Json<TypecheckExplainResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(TypecheckExplainResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: code.to_owned(),
                subcode: subcode.to_owned(),
                message,
                path,
                diagnostics,
            }),
        }),
    )
}

fn run_counterexample_analysis(
    contract: &crate::ast::Contract,
    property: &AnalyzePropertyRequest,
    authorization_rule: Option<&AuthorizationRuleRequest>,
    max_nodes: usize,
) -> Result<(&'static str, CounterexampleResult), (&'static str, &'static str, String, Option<String>)> {
    match property {
        AnalyzePropertyRequest::DeadlineSafety => Ok((
            "deadline_safety",
            analyze_deadline_safety(contract, &CounterexampleRequest { max_nodes }),
        )),
        AnalyzePropertyRequest::AuthorizationSafety => {
            let Some(rule) = authorization_rule else {
                return Err((
                    "RequestError",
                    "MissingAuthorizationRule",
                    "authorization_safety requires an authorization_rule".to_owned(),
                    Some("$.authorization_rule".to_owned()),
                ));
            };
            if rule.allowed_parties.is_empty() {
                return Err((
                    "RequestError",
                    "InvalidAuthorizationRule",
                    "authorization_rule.allowed_parties must not be empty".to_owned(),
                    Some("$.authorization_rule.allowed_parties".to_owned()),
                ));
            }
            let action = match rule.action {
                AuthorizationActionRequest::Deposit => AuthorizationAction::Deposit,
                AuthorizationActionRequest::Choice => AuthorizationAction::Choice,
            };
            let mapped_rule = AuthorizationRule {
                action,
                target: rule.target.clone(),
                allowed_parties: rule.allowed_parties.clone(),
            };
            Ok((
                "authorization_safety",
                analyze_authorization_safety(contract, &CounterexampleRequest { max_nodes }, &mapped_rule),
            ))
        }
    }
}

fn analysis_success_response(
    property: &'static str,
    checked_nodes: usize,
    counterexample: Option<Counterexample>,
) -> AnalyzeCounterexampleSuccessResponse {
    AnalyzeCounterexampleSuccessResponse {
        property,
        status: if counterexample.is_some() {
            "counterexample_found"
        } else {
            "pass_bounded"
        },
        checked_nodes,
        counterexample: counterexample.map(counterexample_to_response),
    }
}

fn counterexample_to_response(counterexample: Counterexample) -> AnalyzeCounterexampleWitnessResponse {
    AnalyzeCounterexampleWitnessResponse {
        violating_path: counterexample.violating_path,
        explanation: counterexample.explanation,
        steps: counterexample
            .steps
            .into_iter()
            .map(|step| AnalyzeCounterexampleStepResponse {
                id: step.id,
                index: step.index,
                kind: step.kind.to_owned(),
                severity: step.severity.to_owned(),
                path: step.path,
                actor: step.actor,
                time: step.time.map(|value| value.to_string()),
                detail: step.detail,
                suggested_fix: step.suggested_fix,
            })
            .collect(),
        auto_repair_patch: counterexample.auto_repair_patch.map(|patch| {
            AnalyzeAutoRepairPatchResponse {
                kind: patch.kind.to_owned(),
                path: patch.path,
                value: patch.value,
                rationale: patch.rationale,
            }
        }),
        timeout: counterexample.timeout.map(|v| v.to_string()),
        witness_time: counterexample.witness_time.map(|v| v.to_string()),
        offending_party: counterexample.offending_party,
        action: counterexample.action.map(str::to_owned),
        target: counterexample.target,
    }
}

fn analyze_bad_request(
    code: &str,
    subcode: &str,
    message: String,
    path: Option<String>,
) -> (StatusCode, Json<AnalyzeCounterexampleResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(AnalyzeCounterexampleResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: code.to_owned(),
                subcode: subcode.to_owned(),
                message,
                path,
                diagnostics: None,
            }),
        }),
    )
}

fn analyze_apply_repair_bad_request(
    code: &str,
    subcode: &str,
    message: String,
    path: Option<String>,
) -> (StatusCode, Json<AnalyzeApplyRepairResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(AnalyzeApplyRepairResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: code.to_owned(),
                subcode: subcode.to_owned(),
                message,
                path,
                diagnostics: None,
            }),
        }),
    )
}

fn analyze_apply_repair_internal_error(
    message: String,
) -> (StatusCode, Json<AnalyzeApplyRepairResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AnalyzeApplyRepairResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: "InternalError".to_owned(),
                subcode: "InternalError".to_owned(),
                message,
                path: None,
                diagnostics: None,
            }),
        }),
    )
}

fn hint_for_type_error(error_message: &str) -> String {
    let message = error_message.to_lowercase();
    if message.contains("undefined let binding") {
        "Declare the referenced `Let.name` in scope before `UseValue`, or rename it to an existing binding.".to_owned()
    } else if message.contains("unknown token") {
        "Use a token known to your context, or extend context definitions to include this token."
            .to_owned()
    } else if message.contains("unknown account owner") {
        "Use an account owner present in context definitions for account operations.".to_owned()
    } else if message.contains("unknown party") {
        "Use a known party role/address from context definitions.".to_owned()
    } else if message.contains("not declared in contract or context") {
        "Add a matching `Choice` action in the contract, or declare the choice in context."
            .to_owned()
    } else if message.contains("invalid bound range") {
        "Make sure each choice bound has `from <= to` once values are concrete.".to_owned()
    } else if message.contains("conflicting inferred types") {
        "Keep each repeated hole/parameter name consistent with one DSL type across the contract."
            .to_owned()
    } else {
        "Adjust the contract expression at this path so it satisfies Marlowe type and scope rules."
            .to_owned()
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
            format!(
                "invalid interval: start {} is greater than end {}",
                start, end
            )
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
            format!(
                "input at index {} did not match any contract case",
                input_index
            )
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
        SimError::NoMatchForInput { input_index } => {
            Some(format!("$.transaction.inputs[{input_index}]"))
        }
        SimError::UselessTransaction => Some("$.transaction".to_owned()),
    }
}
