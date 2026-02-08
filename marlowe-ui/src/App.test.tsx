import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App';

vi.mock('@monaco-editor/react', () => ({
  default: () => <div data-testid="editor" />
}));

const examples = [
  {
    id: 'simple-pay',
    name: 'Simple Pay',
    description: 'Minimal payment contract with a single transfer.',
    language: 'yaml',
    file: '/examples/simple-pay.yaml',
    code: 'test'
  }
];

function mockExamplesFetch(fetchMock: ReturnType<typeof vi.fn>) {
  fetchMock.mockResolvedValueOnce({
    ok: true,
    json: async () => examples
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('App', () => {
  it('loads examples and opens the modal', async () => {
    const fetchMock = vi.fn();
    mockExamplesFetch(fetchMock);
    vi.stubGlobal('fetch', fetchMock);

    render(<App />);

    expect(await screen.findByTestId('editor')).toBeInTheDocument();
    expect(await screen.findByText('Simple Pay')).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /load example/i }));

    expect(screen.getByText('Choose a starting point')).toBeInTheDocument();
  });

  it('connects to the OpenAPI endpoint', async () => {
    const fetchMock = vi.fn();
    mockExamplesFetch(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ openapi: '3.1.0' })
    });
    vi.stubGlobal('fetch', fetchMock);

    render(<App />);
    expect(await screen.findByText('Simple Pay')).toBeInTheDocument();
    expect(screen.queryByText(/api connection failed/i)).not.toBeInTheDocument();
    expect(fetchMock).toHaveBeenNthCalledWith(2, 'http://127.0.0.1:3000/openapi.json');
  });

  it('shows a warning when API is not connected', async () => {
    const fetchMock = vi.fn();
    mockExamplesFetch(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: false,
      status: 503,
      json: async () => ({})
    });
    vi.stubGlobal('fetch', fetchMock);

    render(<App />);

    expect(await screen.findByText(/api connection failed: http 503/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /run validation/i })).toBeDisabled();
    expect(screen.getByRole('button', { name: /run simulation/i })).toBeDisabled();
  });

  it('runs validation stub request', async () => {
    const fetchMock = vi.fn();
    mockExamplesFetch(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ openapi: '3.1.0' })
    });
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ result: 'ok', success: { inputs: [] }, error: null })
    });
    vi.stubGlobal('fetch', fetchMock);

    render(<App />);
    expect(await screen.findByText('Simple Pay')).toBeInTheDocument();
    expect(await screen.findByRole('button', { name: /run validation/i })).toBeEnabled();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /run validation/i }));

    expect(await screen.findByText('Valid contract')).toBeInTheDocument();
    expect(await screen.findByText('No diagnostics.')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenNthCalledWith(3, '/api/simulate/preview', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'test',
        interval_start: '0',
        interval_end: '0'
      })
    });
  });

  it('runs simulation and allows applying a choice input', async () => {
    const fetchMock = vi.fn();
    mockExamplesFetch(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ openapi: '3.1.0' })
    });
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'preview-contract',
          inputs: [
            {
              choice: {
                id: { ChoiceId: { name: 'PickNumber', party: { Role: 'Alice' } } },
                bounds: [{ from: '1', to: '5' }]
              }
            }
          ]
        },
        error: null
      })
    });
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: { contract_yaml: 'after-step', warnings: [] },
        error: null
      })
    });
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: { contract_yaml: 'after-step', inputs: [] },
        error: null
      })
    });
    vi.stubGlobal('fetch', fetchMock);

    render(<App />);
    expect(await screen.findByText('Simple Pay')).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /run simulation/i }));

    expect(
      await screen.findByText('Preview succeeded with 1 available input(s)')
    ).toBeInTheDocument();
    expect(await screen.findByLabelText('Choice input')).toBeInTheDocument();
    expect(await screen.findByLabelText('Choice value')).toBeInTheDocument();

    await user.clear(screen.getByLabelText('Choice value'));
    await user.type(screen.getByLabelText('Choice value'), '3');
    await user.click(screen.getByRole('button', { name: /apply choice/i }));

    expect(await screen.findByText(/Simulation step applied/)).toBeInTheDocument();
    expect(fetchMock).toHaveBeenNthCalledWith(3, '/api/simulate/preview', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'test',
        interval_start: '0',
        interval_end: '0'
      })
    });
    expect(fetchMock).toHaveBeenNthCalledWith(4, '/api/simulate/step', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contract_yaml: 'preview-contract',
        transaction: {
          interval_start: '0',
          interval_end: '0',
          inputs: [
            {
              choice: {
                id: { ChoiceId: { name: 'PickNumber', party: { Role: 'Alice' } } },
                value: '3'
              }
            }
          ]
        }
      })
    });
  });
});
