export type ValidationResponse = {
  valid: boolean;
  diagnostics: string[];
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
  warnings: string[];
  inputs: SimulationInput[];
  context: SimulationContext;
};

export type SimulationStepResponse = {
  summary: string;
  warnings: string[];
  context: SimulationContext;
};

type ApiDiagnostic = {
  message: string;
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
    warnings: Array<{ code?: string }>;
  } | null;
};

const JSON_HEADERS = {
  'Content-Type': 'application/json'
};

async function postJson<T>(url: string, payload: Record<string, unknown>): Promise<T> {
  const response = await fetch(url, {
    method: 'POST',
    headers: JSON_HEADERS,
    body: JSON.stringify(payload)
  });

  if (!response.ok) {
    throw new Error(`Request failed (${response.status})`);
  }

  return (await response.json()) as T;
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

function mapPreviewErrorToMessages(error: ApiSimulatePreviewResponse['error']): string[] {
  if (!error) {
    return [];
  }
  return error.diagnostics?.map((item) => item.message) ?? [error.message];
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
  const response = await postJson<ApiSimulatePreviewResponse>('/api/simulate/preview', {
    contract_yaml: code,
    interval_start: '0',
    interval_end: '0'
  });

  if (response.error) {
    return {
      valid: false,
      diagnostics: mapPreviewErrorToMessages(response.error)
    };
  }

  return {
    valid: true,
    diagnostics: []
  };
}

export async function simulateContract(
  contractYaml: string,
  state: Record<string, unknown> | null = null
): Promise<SimulationResponse> {
  const minTime = getMinTime(state);
  const response = await postJson<ApiSimulatePreviewResponse>('/api/simulate/preview', {
    contract_yaml: contractYaml,
    interval_start: minTime,
    interval_end: minTime,
    state: state ?? undefined
  });

  if (response.error) {
    const warnings = mapPreviewErrorToMessages(response.error);
    return {
      summary: `Preview failed (${response.error.subcode})`,
      warnings,
      inputs: [],
      context: {
        contractYaml,
        state,
        minTime
      }
    };
  }

  const nextState = response.success?.state ?? state;
  const nextInputs = mapPreviewInputs(response.success?.inputs ?? []);
  return {
    summary: `Preview succeeded with ${nextInputs.length} available input(s)`,
    warnings: [],
    inputs: nextInputs,
    context: {
      contractYaml: response.success?.contract_yaml ?? contractYaml,
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

  const response = await postJson<ApiSimulateStepResponse>('/api/simulate/step', {
    contract_yaml: context.contractYaml,
    state: context.state ?? undefined,
    transaction: {
      interval_start: context.minTime,
      interval_end: context.minTime,
      inputs: [inputPayload]
    }
  });

  if (response.error) {
    throw new Error(response.error.message);
  }

  const warningCodes =
    response.success?.warnings?.map((warning) => warning.code ?? 'Warning') ?? [];
  const nextState = response.success?.state ?? context.state;
  return {
    summary: 'Simulation step applied',
    warnings: warningCodes,
    context: {
      contractYaml: response.success?.contract_yaml ?? context.contractYaml,
      state: nextState,
      minTime: getMinTime(nextState)
    }
  };
}
