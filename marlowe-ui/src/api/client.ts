export type ValidationDiagnostic = {
  message: string;
  path?: string;
  line?: number;
  column?: number;
  endLine?: number;
  endColumn?: number;
  code?: string;
  subcode?: string;
  details?: Record<string, unknown>;
};

export type ValidationResponse = {
  valid: boolean;
  diagnostics: ValidationDiagnostic[];
  readyToRun?: boolean;
  summary?: ValidationSummary;
  blocking?: ValidationExplainItem[];
  warnings?: ValidationExplainItem[];
};

export type ValidationSummary = {
  blockingCount: number;
  warningCount: number;
  errorCount: number;
  holeCount: number;
  paramCount: number;
};

export type ValidationExplainItem = {
  code: string;
  path: string;
  message: string;
  hint: string;
  details?: Record<string, unknown>;
};

export type ChoiceBound = {
  from: string;
  to: string;
};

export type SimulationContext = {
  contractYaml: string;
  state: Record<string, unknown> | null;
  minTime: string;
};

type SimulationChoiceInput = {
  kind: 'choice';
  id: Record<string, unknown>;
  name: string;
  bounds: ChoiceBound[];
};

type SimulationDepositInput = {
  kind: 'deposit';
  by: Record<string, unknown>;
  into: Record<string, unknown>;
  token: Record<string, unknown>;
  amount: string;
};

type SimulationNotifyInput = {
  kind: 'notify';
};

export type SimulationInput =
  | SimulationChoiceInput
  | SimulationDepositInput
  | SimulationNotifyInput;

export type SimulationResponse = {
  summary: string;
  warnings: SimulationWarning[];
  inputs: SimulationInput[];
  context: SimulationContext;
};

export type SimulationStepResponse = {
  summary: string;
  warnings: SimulationWarning[];
  context: SimulationContext;
  traceEvents: SimulationTraceEvent[];
};

export type SimulationWarning = {
  code: string;
  message: string;
  fields: Array<{ name: string; value: string }>;
};

export type SimulationTraceEvent = {
  eventId: string;
  code: string;
  contractPath: string;
  label: string;
  warningCode?: string;
};

type ApiDiagnostic = {
  code?: string;
  subcode?: string;
  path?: string;
  message: string;
  line?: number;
  column?: number;
  end_line?: number;
  end_column?: number;
  details?: Record<string, unknown>;
};

type ApiSimulateState = {
  min_time?: string;
} & Record<string, unknown>;

type ApiPreviewInput =
  | {
      choice: {
        id: Record<string, unknown>;
        bounds: ChoiceBound[];
      };
    }
  | {
      deposit: {
        by: Record<string, unknown>;
        into: Record<string, unknown>;
        token: Record<string, unknown>;
        amount: string;
      };
    }
  | {
      notify: {
        can_notify: boolean;
      };
    };

type ApiSimulatePreviewResponse = {
  result: string;
  error?: {
    subcode: string;
    message: string;
    diagnostics?: ApiDiagnostic[] | null;
  } | null;
  success?: {
    contract_yaml: string;
    state: ApiSimulateState;
    inputs?: ApiPreviewInput[];
    warnings?: ApiWarning[];
  } | null;
};

type ApiSimulateStepResponse = {
  result: string;
  error?: {
    message: string;
  } | null;
  success?: {
    contract_yaml: string;
    state: ApiSimulateState;
    warnings: ApiWarning[];
    trace?: ApiTraceEvent[] | null;
  } | null;
};

type ApiWarning = {
  code?: string;
} & Record<string, unknown>;

type ApiTraceInput =
  | {
      kind: 'choice';
      id: Record<string, unknown>;
      value: string;
    }
  | {
      kind: 'deposit';
      by: Record<string, unknown>;
      into: Record<string, unknown>;
      token: Record<string, unknown>;
      amount: string;
    }
  | {
      kind: 'notify';
    };

type ApiTraceEventReduced = {
  event_id: string;
  code: 'Reduced';
  contract_path: string;
  rule: string;
  warning?: { code?: string } | null;
};

type ApiTraceEventInputApplied = {
  event_id: string;
  code: 'InputApplied';
  contract_path: string;
  input_index: number;
  input: ApiTraceInput;
  warning?: { code?: string } | null;
};

type ApiTraceEvent = ApiTraceEventReduced | ApiTraceEventInputApplied;

type ApiTypecheckExplainItem = {
  code: string;
  path: string;
  message: string;
  hint: string;
  details?: Record<string, unknown>;
};

type ApiTypecheckExplainSummary = {
  blocking_count: number;
  warning_count: number;
  error_count: number;
  hole_count: number;
  param_count: number;
};

type ApiTypecheckExplainSuccess = {
  ready_to_run: boolean;
  summary: ApiTypecheckExplainSummary;
  blocking: ApiTypecheckExplainItem[];
  warnings: ApiTypecheckExplainItem[];
};

type ApiTypecheckExplainResponse = {
  result: string;
  error?: {
    code?: string;
    subcode?: string;
    message: string;
  } | null;
  success?: ApiTypecheckExplainSuccess | null;
};

const JSON_HEADERS = {
  'Content-Type': 'application/json'
};

type JsonResponse<T> = {
  ok: boolean;
  status: number;
  data: T | null;
};

async function requestJson<T>(
  url: string,
  payload: Record<string, unknown>
): Promise<JsonResponse<T>> {
  const response = await fetch(url, {
    method: 'POST',
    headers: JSON_HEADERS,
    body: JSON.stringify(payload)
  });

  let data: T | null = null;
  try {
    data = (await response.json()) as T;
  } catch {
    data = null;
  }

  return {
    ok: response.ok,
    status: response.status,
    data
  };
}

function statusError(status: number): Error {
  return new Error(`Request failed (${status})`);
}

function getChoiceName(id: Record<string, unknown>): string {
  const choiceId = (id.ChoiceId ?? null) as { name?: unknown } | null;
  if (choiceId && typeof choiceId.name === 'string') {
    return choiceId.name;
  }

  const hole = id.Hole;
  if (typeof hole === 'string') {
    return hole;
  }

  return 'Unnamed choice';
}

function traceInputLabel(input: ApiTraceInput): string {
  if (input.kind === 'choice') {
    const choiceName = getChoiceName(input.id);
    return `choice ${choiceName}=${input.value}`;
  }

  if (input.kind === 'deposit') {
    return `deposit amount=${input.amount}`;
  }

  return 'notify';
}

function mapTraceEvents(trace: ApiTraceEvent[] | null | undefined): SimulationTraceEvent[] {
  if (!trace || trace.length === 0) {
    return [];
  }

  return trace.map((event) => {
    if (event.code === 'InputApplied') {
      return {
        eventId: event.event_id,
        code: event.code,
        contractPath: event.contract_path,
        label: `Input applied: ${traceInputLabel(event.input)}`,
        warningCode: event.warning?.code
      };
    }

    return {
      eventId: event.event_id,
      code: event.code,
      contractPath: event.contract_path,
      label: `Reduced: ${event.rule}`,
      warningCode: event.warning?.code
    };
  });
}

function mapPreviewErrorToMessages(error: ApiSimulatePreviewResponse['error']): string[] {
  if (!error) {
    return [];
  }
  return error.diagnostics?.map((item) => item.message) ?? [error.message];
}

function formatWarningCode(code: string): string {
  return code.replace(/([a-z0-9])([A-Z])/g, '$1 $2');
}

function warningValueToString(value: unknown): string {
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function mapWarnings(warnings: ApiWarning[] | null | undefined): SimulationWarning[] {
  if (!warnings || warnings.length === 0) {
    return [];
  }

  return warnings.map((warning) => {
    const code = typeof warning.code === 'string' ? warning.code : 'Warning';
    const fields = Object.entries(warning)
      .filter(([key]) => key !== 'code')
      .map(([name, value]) => ({
        name,
        value: warningValueToString(value)
      }));

    return {
      code,
      message: formatWarningCode(code),
      fields
    };
  });
}

function mapPreviewErrorToDiagnostics(
  error: ApiSimulatePreviewResponse['error']
): ValidationDiagnostic[] {
  if (!error) {
    return [];
  }

  if (error.diagnostics && error.diagnostics.length > 0) {
    return error.diagnostics.map((item) => ({
      message: item.message,
      path: item.path,
      line: item.line,
      column: item.column,
      endLine: item.end_line,
      endColumn: item.end_column,
      code: item.code,
      subcode: item.subcode,
      details: item.details
    }));
  }

  return [
    {
      message: error.message,
      subcode: error.subcode
    }
  ];
}

function mapExplainItem(item: ApiTypecheckExplainItem): ValidationExplainItem {
  return {
    code: item.code,
    path: item.path,
    message: item.message,
    hint: item.hint,
    details: item.details
  };
}

function mapExplainSummary(summary: ApiTypecheckExplainSummary): ValidationSummary {
  return {
    blockingCount: summary.blocking_count,
    warningCount: summary.warning_count,
    errorCount: summary.error_count,
    holeCount: summary.hole_count,
    paramCount: summary.param_count
  };
}

function mapPreviewInputs(inputs: ApiPreviewInput[]): SimulationInput[] {
  const mapped: SimulationInput[] = [];

  for (const input of inputs) {
    if ('choice' in input) {
      mapped.push({
        kind: 'choice',
        id: input.choice.id,
        bounds: input.choice.bounds,
        name: getChoiceName(input.choice.id)
      });
      continue;
    }

    if ('deposit' in input) {
      mapped.push({
        kind: 'deposit',
        by: input.deposit.by,
        into: input.deposit.into,
        token: input.deposit.token,
        amount: input.deposit.amount
      });
      continue;
    }

    if ('notify' in input && input.notify.can_notify) {
      mapped.push({ kind: 'notify' });
    }
  }

  return mapped;
}

function getMinTime(state: Record<string, unknown> | null | undefined): string {
  if (!state) {
    return '0';
  }
  const value = state.min_time;
  if (typeof value === 'string') {
    return value;
  }
  return '0';
}

export async function validateContract(code: string): Promise<ValidationResponse> {
  const [previewResponse, explainResponse] = await Promise.all([
    requestJson<ApiSimulatePreviewResponse>('/api/simulate/preview', {
      contract_yaml: code,
      interval_start: '0',
      interval_end: '0'
    }),
    requestJson<ApiTypecheckExplainResponse>('/api/typecheck/explain', {
      contract_yaml: code
    })
  ]);

  const explainSuccess = explainResponse.data?.success ?? null;
  const explainData =
    explainSuccess === null
      ? {}
      : {
          readyToRun: explainSuccess.ready_to_run,
          summary: mapExplainSummary(explainSuccess.summary),
          blocking: explainSuccess.blocking.map(mapExplainItem),
          warnings: explainSuccess.warnings.map(mapExplainItem)
        };

  if (previewResponse.data?.error) {
    return {
      valid: false,
      diagnostics: mapPreviewErrorToDiagnostics(previewResponse.data.error),
      ...explainData
    };
  }

  if (!previewResponse.ok) {
    throw statusError(previewResponse.status);
  }

  return {
    valid: true,
    diagnostics: [],
    ...explainData
  };
}

export async function simulateContract(
  contractYaml: string,
  state: Record<string, unknown> | null = null
): Promise<SimulationResponse> {
  const minTime = getMinTime(state);
  const response = await requestJson<ApiSimulatePreviewResponse>('/api/simulate/preview', {
    contract_yaml: contractYaml,
    interval_start: minTime,
    interval_end: minTime,
    state: state ?? undefined
  });

  if (response.data?.error) {
    const warningMessages = mapPreviewErrorToMessages(response.data.error);
    const warnings = warningMessages.map((message) => ({
      code: 'PreviewError',
      message,
      fields: []
    }));
    return {
      summary: `Preview failed (${response.data.error.subcode})`,
      warnings,
      inputs: [],
      context: {
        contractYaml,
        state,
        minTime
      }
    };
  }

  if (!response.ok || !response.data) {
    throw statusError(response.status);
  }

  const nextState = response.data.success?.state ?? state;
  const nextInputs = mapPreviewInputs(response.data.success?.inputs ?? []);
  return {
    summary: `Preview succeeded with ${nextInputs.length} available input(s)`,
    warnings: mapWarnings(response.data.success?.warnings),
    inputs: nextInputs,
    context: {
      contractYaml: response.data.success?.contract_yaml ?? contractYaml,
      state: nextState,
      minTime: getMinTime(nextState)
    }
  };
}

export async function simulateStep(
  context: SimulationContext,
  input: SimulationInput,
  choiceValue = '0'
): Promise<SimulationStepResponse> {
  let inputPayload: Record<string, unknown> | string;

  if (input.kind === 'choice') {
    inputPayload = {
      choice: {
        id: input.id,
        value: choiceValue
      }
    };
  } else if (input.kind === 'deposit') {
    inputPayload = {
      deposit: {
        by: input.by,
        into: input.into,
        token: input.token,
        amount: input.amount
      }
    };
  } else {
    inputPayload = 'notify';
  }

  const response = await requestJson<ApiSimulateStepResponse>('/api/simulate/step', {
    contract_yaml: context.contractYaml,
    state: context.state ?? undefined,
    trace: true,
    transaction: {
      interval_start: context.minTime,
      interval_end: context.minTime,
      inputs: [inputPayload]
    }
  });

  if (response.data?.error) {
    throw new Error(response.data.error.message);
  }

  if (!response.ok || !response.data) {
    throw statusError(response.status);
  }

  const warnings = mapWarnings(response.data.success?.warnings);
  const nextState = response.data.success?.state ?? context.state;
  const traceEvents = mapTraceEvents(response.data.success?.trace);
  return {
    summary: 'Simulation step applied',
    warnings,
    traceEvents,
    context: {
      contractYaml: response.data.success?.contract_yaml ?? context.contractYaml,
      state: nextState,
      minTime: getMinTime(nextState)
    }
  };
}

export async function simulateTimeoutStep(
  context: SimulationContext,
  nextTimeout: string
): Promise<SimulationStepResponse> {
  const response = await requestJson<ApiSimulateStepResponse>('/api/simulate/step', {
    contract_yaml: context.contractYaml,
    state: context.state ?? undefined,
    trace: true,
    transaction: {
      interval_start: nextTimeout,
      interval_end: nextTimeout,
      inputs: []
    }
  });

  if (response.data?.error) {
    throw new Error(response.data.error.message);
  }

  if (!response.ok || !response.data) {
    throw statusError(response.status);
  }

  const warnings = mapWarnings(response.data.success?.warnings);
  const nextState = response.data.success?.state ?? context.state;
  const traceEvents = mapTraceEvents(response.data.success?.trace);
  return {
    summary: `Advanced to timeout ${nextTimeout}`,
    warnings,
    traceEvents,
    context: {
      contractYaml: response.data.success?.contract_yaml ?? context.contractYaml,
      state: nextState,
      minTime: getMinTime(nextState)
    }
  };
}
