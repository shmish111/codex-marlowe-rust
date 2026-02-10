import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  analyzeContract,
  applyDeterministicRepair,
  simulateContract,
  simulateStep,
  simulateTimeoutStep,
  validateContract
} from './client';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('api client', () => {
  it('posts validation payload', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          result: 'ok',
          success: { contract_yaml: 'x', state: { min_time: '0' }, inputs: [] },
          error: null
        })
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          result: 'ok',
          success: {
            ready_to_run: true,
            summary: {
              blocking_count: 0,
              warning_count: 0,
              error_count: 0,
              hole_count: 0,
              param_count: 0
            },
            blocking: [],
            warnings: []
          },
          error: null
        })
      });

    vi.stubGlobal('fetch', fetchMock);

    const result = await validateContract('contract-code');

    expect(result.valid).toBe(true);
    expect(result.readyToRun).toBe(true);
    expect(fetchMock).toHaveBeenNthCalledWith(1, '/api/simulate/preview', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        interval_start: '0',
        interval_end: '0'
      })
    });
    expect(fetchMock).toHaveBeenNthCalledWith(2, '/api/typecheck/explain', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code'
      })
    });
  });

  it('returns detailed diagnostics for 400 validation responses', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({
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
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          result: 'ok',
          success: {
            ready_to_run: false,
            summary: {
              blocking_count: 1,
              warning_count: 0,
              error_count: 1,
              hole_count: 0,
              param_count: 0
            },
            blocking: [
              {
                code: 'TypeError',
                path: '$.When',
                message: 'Bad token at line 4',
                hint: 'Check this node.'
              }
            ],
            warnings: []
          },
          error: null
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
    expect(result.summary?.errorCount).toBe(1);
    expect(result.blocking?.[0]?.hint).toBe('Check this node.');
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

  it('maps structured warnings from preview responses', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'next-contract',
          state: { min_time: '10' },
          inputs: [],
          warnings: [
            {
              code: 'PartialPay',
              expected: '100',
              paid: '80'
            }
          ]
        }
      })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await simulateContract('contract-code');
    expect(result.warnings).toEqual([
      {
        code: 'PartialPay',
        message: 'Partial Pay',
        fields: [
          { name: 'expected', value: '100' },
          { name: 'paid', value: '80' }
        ]
      }
    ]);
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
    expect(result.traceEvents).toEqual([]);
    expect(fetchMock).toHaveBeenCalledWith('/api/simulate/step', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        state: { min_time: '10' },
        trace: true,
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

  it('maps counterexample analysis response', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'success',
        success: {
          property: 'deadline_safety',
          status: 'counterexample_found',
          checked_nodes: 4,
          counterexample: {
            violating_path: '$.When',
            explanation: 'Timeout continuation is not Close.',
            steps: [
              {
                id: 'deadline_safety.step.0',
                index: 0,
                kind: 'timeout_reached',
                severity: 'info',
                path: '$.transaction.interval_end',
                detail: 'Reached timeout',
                suggested_fix: 'Review timeout branch'
              }
            ],
            auto_repair_patch: {
              kind: 'set_contract',
              path: '$.When.timeout_continuation',
              value: '{ Close: {} }',
              rationale: 'Close on timeout'
            }
          }
        }
      })
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await analyzeContract('contract-code');
    expect(result.status).toBe('counterexample_found');
    expect(result.counterexample?.steps[0]?.id).toBe('deadline_safety.step.0');
    expect(result.counterexample?.autoRepairPatch?.kind).toBe('set_contract');
    expect(fetchMock).toHaveBeenCalledWith('/api/analyze/counterexample', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        property: 'deadline_safety'
      })
    });
  });

  it('maps apply repair response with patched yaml', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'success',
        success: {
          property: 'deadline_safety',
          repaired: true,
          before: {
            property: 'deadline_safety',
            status: 'counterexample_found',
            checked_nodes: 4
          },
          after: {
            property: 'deadline_safety',
            status: 'pass_bounded',
            checked_nodes: 4
          },
          patched_contract_yaml: 'When:\\n  timeout_continuation: { Close: {} }'
        }
      })
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await applyDeterministicRepair('contract-code');
    expect(result.repaired).toBe(true);
    expect(result.after?.status).toBe('pass_bounded');
    expect(result.patchedContractYaml).toContain('Close');
    expect(fetchMock).toHaveBeenCalledWith('/api/analyze/apply-repair', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        property: 'deadline_safety'
      })
    });
  });

  it('sends authorization_safety rule in analyze payload', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'success',
        success: {
          property: 'authorization_safety',
          status: 'pass_bounded',
          checked_nodes: 10
        }
      })
    });
    vi.stubGlobal('fetch', fetchMock);

    await analyzeContract('contract-code', {
      property: 'authorization_safety',
      authorizationRule: {
        action: 'choice',
        target: 'release_funds',
        allowedParties: [{ Role: 'alice' }]
      }
    });

    expect(fetchMock).toHaveBeenCalledWith('/api/analyze/counterexample', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        property: 'authorization_safety',
        authorization_rule: {
          action: 'choice',
          target: 'release_funds',
          allowed_parties: [{ Role: 'alice' }]
        }
      })
    });
  });

  it('sends authorization_safety rule in apply repair payload', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'success',
        success: {
          property: 'authorization_safety',
          repaired: false,
          before: {
            property: 'authorization_safety',
            status: 'pass_bounded',
            checked_nodes: 10
          }
        }
      })
    });
    vi.stubGlobal('fetch', fetchMock);

    await applyDeterministicRepair('contract-code', {
      property: 'authorization_safety',
      authorizationRule: {
        action: 'deposit',
        allowedParties: [{ Role: 'alice' }, { Role: 'bob' }]
      }
    });

    expect(fetchMock).toHaveBeenCalledWith('/api/analyze/apply-repair', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        property: 'authorization_safety',
        authorization_rule: {
          action: 'deposit',
          target: undefined,
          allowed_parties: [{ Role: 'alice' }, { Role: 'bob' }]
        }
      })
    });
  });

  it('maps trace events from simulation step', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'after-step',
          state: { min_time: '11' },
          warnings: [
            {
              code: 'AssertionFailed'
            }
          ],
          trace: [
            {
              event_id: 'evt-1',
              code: 'InputApplied',
              contract_path: '$.When',
              input_index: 0,
              input: {
                kind: 'choice',
                id: { ChoiceId: { name: 'MakeChoice', party: { Role: 'Alice' } } },
                value: '3'
              }
            },
            {
              event_id: 'evt-2',
              code: 'Reduced',
              contract_path: '$.When.then',
              rule: 'ReducePay'
            }
          ]
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

    expect(result.traceEvents).toEqual([
      {
        eventId: 'evt-1',
        code: 'InputApplied',
        contractPath: '$.When',
        label: 'Input applied: choice MakeChoice=3',
        warningCode: undefined
      },
      {
        eventId: 'evt-2',
        code: 'Reduced',
        contractPath: '$.When.then',
        label: 'Reduced: ReducePay',
        warningCode: undefined
      }
    ]);
    expect(result.warnings).toEqual([
      {
        code: 'AssertionFailed',
        message: 'Assertion Failed',
        fields: []
      }
    ]);
  });

  it('posts timeout step with empty inputs', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'after-timeout',
          state: { min_time: '20' },
          warnings: []
        }
      })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await simulateTimeoutStep(
      { contractYaml: 'contract-code', state: { min_time: '10' }, minTime: '10' },
      '20'
    );

    expect(result.summary).toContain('20');
    expect(fetchMock).toHaveBeenCalledWith('/api/simulate/step', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'contract-code',
        state: { min_time: '10' },
        trace: true,
        transaction: {
          interval_start: '20',
          interval_end: '20',
          inputs: []
        }
      })
    });
  });
});
