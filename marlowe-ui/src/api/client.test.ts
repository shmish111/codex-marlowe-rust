import { afterEach, describe, expect, it, vi } from 'vitest';
import { simulateChoiceStep, simulateContract, validateContract } from './client';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('api client', () => {
  it('posts validation payload', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ result: 'ok', success: { inputs: [] }, error: null })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await validateContract('contract-code');

    expect(result.valid).toBe(true);
    expect(fetchMock).toHaveBeenCalledWith('/api/simulate/preview', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        interval_start: '0',
        interval_end: '0'
      })
    });
  });

  it('throws for non-2xx responses', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({})
    });

    vi.stubGlobal('fetch', fetchMock);

    await expect(simulateContract('bad-contract')).rejects.toThrow('Request failed (400)');
  });

  it('maps preview diagnostics as validation errors', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'error',
        error: {
          code: 'SIM',
          subcode: 'INVALID_CONTRACT',
          message: 'Invalid contract',
          diagnostics: [{ code: 'X', subcode: 'Y', path: 'root', message: 'Bad yaml' }]
        }
      })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await validateContract('bad-contract');
    expect(result.valid).toBe(false);
    expect(result.diagnostics).toEqual(['Bad yaml']);
  });

  it('extracts preview choice inputs', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'next-contract',
          inputs: [
            {
              choice: {
                id: { ChoiceId: { name: 'MakeChoice', party: { Role: 'Alice' } } },
                bounds: [{ from: '1', to: '5' }]
              }
            }
          ]
        }
      })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await simulateContract('contract-code');
    expect(result.contractYaml).toBe('next-contract');
    expect(result.choices).toHaveLength(1);
    expect(result.choices[0].name).toBe('MakeChoice');
  });

  it('posts simulate step choice transaction', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'after-step',
          warnings: []
        }
      })
    });

    vi.stubGlobal('fetch', fetchMock);

    const id = { ChoiceId: { name: 'MakeChoice', party: { Role: 'Alice' } } };
    const result = await simulateChoiceStep('contract-code', id, '3');

    expect(result.contractYaml).toBe('after-step');
    expect(fetchMock).toHaveBeenCalledWith('/api/simulate/step', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        transaction: {
          interval_start: '0',
          interval_end: '0',
          inputs: [{ choice: { id, value: '3' } }]
        }
      })
    });
  });
});
