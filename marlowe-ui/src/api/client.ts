export type ValidationResponse = {
  valid: boolean;
  diagnostics: string[];
};

export type SimulationResponse = {
  summary: string;
  warnings: string[];
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
  return postJson<ValidationResponse>('/api/validate', { code });
}

export async function simulateContract(code: string): Promise<SimulationResponse> {
  return postJson<SimulationResponse>('/api/simulate', { code });
}
