import { afterEach, describe, expect, it, vi } from 'vitest';
import { simulateContract, validateContract } from './client';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('api client', () => {
  it('posts validation payload', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ valid: true, diagnostics: [] })
    });

    vi.stubGlobal('fetch', fetchMock);

    const result = await validateContract('contract-code');

    expect(result.valid).toBe(true);
    expect(fetchMock).toHaveBeenCalledWith('/api/validate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ code: 'contract-code' })
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
});
