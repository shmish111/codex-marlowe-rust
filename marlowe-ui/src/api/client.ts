export type ValidationResponse = {
  valid: boolean;
  diagnostics: string[];
};

export type ChoiceBound = {
  from: string;
  to: string;
};

export type ChoiceInput = {
  id: Record<string, unknown>;
  name: string;
  bounds: ChoiceBound[];
};

export type SimulationResponse = {
  summary: string;
  warnings: string[];
  choices: ChoiceInput[];
  contractYaml: string;
};

export type SimulationStepResponse = {
  summary: string;
  warnings: string[];
  contractYaml: string;
};

type ApiDiagnostic = {
  code: string;
  subcode: string;
  path: string;
  message: string;
};

type ApiChoiceInput = {
  choice: {
    id: Record<string, unknown>;
    bounds: ChoiceBound[];
  };
};

type ApiSimulatePreviewResponse = {
  result: string;
  error?: {
    code: string;
    subcode: string;
    message: string;
    diagnostics?: ApiDiagnostic[] | null;
  } | null;
  success?: {
    contract_yaml: string;
    inputs?: Array<ApiChoiceInput | Record<string, unknown>>;
  } | null;
};

type ApiSimulateStepResponse = {
  result: string;
  error?: {
    code: string;
    subcode: string;
    message: string;
    diagnostics?: ApiDiagnostic[] | null;
  } | null;
  success?: {
    contract_yaml: string;
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

export async function simulateContract(code: string): Promise<SimulationResponse> {
  const response = await postJson<ApiSimulatePreviewResponse>('/api/simulate/preview', {
    contract_yaml: code,
    interval_start: '0',
    interval_end: '0'
  });

  if (response.error) {
    const warnings = mapPreviewErrorToMessages(response.error);
    return {
      summary: `Preview failed (${response.error.subcode})`,
      warnings,
      choices: [],
      contractYaml: code
    };
  }

  const inputs = response.success?.inputs ?? [];
  const choices: ChoiceInput[] = [];

  for (const input of inputs) {
    if ('choice' in input) {
      const choice = input.choice as ApiChoiceInput['choice'];
      choices.push({
        id: choice.id,
        bounds: choice.bounds,
        name: getChoiceName(choice.id)
      });
    }
  }

  const inputCount = inputs.length;
  return {
    summary: `Preview succeeded with ${inputCount} available input(s)`,
    warnings: [],
    choices,
    contractYaml: response.success?.contract_yaml ?? code
  };
}

export async function simulateChoiceStep(
  contractYaml: string,
  choiceId: Record<string, unknown>,
  value: string
): Promise<SimulationStepResponse> {
  const response = await postJson<ApiSimulateStepResponse>('/api/simulate/step', {
    contract_yaml: contractYaml,
    transaction: {
      interval_start: '0',
      interval_end: '0',
      inputs: [
        {
          choice: {
            id: choiceId,
            value
          }
        }
      ]
    }
  });

  if (response.error) {
    throw new Error(response.error.message);
  }

  const warningCodes =
    response.success?.warnings?.map((warning) => warning.code ?? 'Warning') ?? [];
  return {
    summary: 'Simulation step applied',
    warnings: warningCodes,
    contractYaml: response.success?.contract_yaml ?? contractYaml
  };
}
