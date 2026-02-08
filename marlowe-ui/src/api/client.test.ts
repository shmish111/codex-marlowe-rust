import { afterEach, describe, expect, it, vi } from 'vitest';
import { simulateContract, simulateStep, validateContract } from './client';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('api client', () => {
  it('posts validation payload', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: { contract_yaml: 'x', state: { min_time: '0' }, inputs: [] },
        error: null
      })
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

  it('returns detailed diagnostics for 400 validation responses', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({
        result: 'error',
        error: {
          subcode: 'INVALID_CONTRACT',
          message: 'Invalid contract',
          diagnostics: [
            {
              message: 'Bad token at line 4',
              line: 4,
              column: 12,
              end_line: 4,
              end_column: 20
            }
          ]
        }
      })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await validateContract('bad-contract');
    expect(result.valid).toBe(false);
    expect(result.diagnostics).toEqual([
      {
        code: undefined,
        subcode: undefined,
        path: undefined,
        line: 4,
        column: 12,
        endLine: 4,
        endColumn: 20,
        message: 'Bad token at line 4'
      }
    ]);
  });

  it('extracts choice/deposit/notify inputs from preview', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'next-contract',
          state: { min_time: '10' },
          inputs: [
            {
              choice: {
                id: { ChoiceId: { name: 'MakeChoice', party: { Role: 'Alice' } } },
                bounds: [{ from: '1', to: '5' }]
              }
            },
            {
              deposit: {
                by: { Role: 'Alice' },
                into: { Role: 'Bob' },
                token: { Token: { currency_symbol: '', token_name: '' } },
                amount: '10'
              }
            },
            {
              notify: {
                can_notify: true
              }
            }
          ]
        }
      })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await simulateContract('contract-code');
    expect(result.context.contractYaml).toBe('next-contract');
    expect(result.context.minTime).toBe('10');
    expect(result.inputs).toHaveLength(3);
    expect(result.inputs[0].kind).toBe('choice');
    expect(result.inputs[1].kind).toBe('deposit');
    expect(result.inputs[2].kind).toBe('notify');
  });

  it('posts choice step with state and interval', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'after-step',
          state: { min_time: '11' },
          warnings: []
        }
      })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await simulateStep(
      { contractYaml: 'contract-code', state: { min_time: '10' }, minTime: '10' },
      {
        kind: 'choice',
        id: { ChoiceId: { name: 'MakeChoice', party: { Role: 'Alice' } } },
        name: 'MakeChoice',
        bounds: [{ from: '1', to: '5' }]
      },
      '3'
    );

    expect(result.context.contractYaml).toBe('after-step');
    expect(fetchMock).toHaveBeenCalledWith('/api/simulate/step', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        state: { min_time: '10' },
        transaction: {
          interval_start: '10',
          interval_end: '10',
          inputs: [
            {
              choice: {
                id: { ChoiceId: { name: 'MakeChoice', party: { Role: 'Alice' } } },
                value: '3'
              }
            }
          ]
        }
      })
    });
  });
});
