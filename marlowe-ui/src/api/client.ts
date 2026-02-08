export type ValidationResponse = {
  valid: boolean;
  diagnostics: string[];
};

export type SimulationResponse = {
  summary: string;
  warnings: string[];
};

type ApiDiagnostic = {
  code: string;
  subcode: string;
  path: string;
  message: string;
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
    inputs?: unknown[];
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

export async function validateContract(code: string): Promise<ValidationResponse> {
  const response = await postJson<ApiSimulatePreviewResponse>('/api/simulate/preview', {
    contract_yaml: code,
    interval_start: '0',
    interval_end: '0'
  });

  if (response.error) {
    const diagnostics = response.error.diagnostics?.map((item) => item.message) ?? [
      response.error.message
    ];
    return {
      valid: false,
      diagnostics
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
    const warnings = response.error.diagnostics?.map((item) => item.message) ?? [
      response.error.message
    ];
    return {
      summary: `Preview failed (${response.error.subcode})`,
      warnings
    };
  }

  const inputCount = response.success?.inputs?.length ?? 0;
  return {
    summary: `Preview succeeded with ${inputCount} available input(s)`,
    warnings: []
  };
}
