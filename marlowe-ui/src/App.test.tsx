import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App';

vi.mock('@monaco-editor/react', () => ({
  default: ({ value, onChange }: { value: string; onChange?: (value: string) => void }) => (
    <textarea
      data-testid="editor-input"
      value={value}
      onChange={(event) => onChange?.(event.target.value)}
    />
  )
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

function mockPreviewOk(fetchMock: ReturnType<typeof vi.fn>, contractYaml = 'test') {
  fetchMock.mockResolvedValueOnce({
    ok: true,
    json: async () => ({
      result: 'ok',
      success: {
        contract_yaml: contractYaml,
        state: { min_time: '0' },
        inputs: []
      },
      error: null
    })
  });
}

function mockTypecheckReady(fetchMock: ReturnType<typeof vi.fn>) {
  fetchMock.mockResolvedValueOnce({
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
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('App', () => {
  it('loads examples and opens the modal', async () => {
    const fetchMock = vi.fn();
    mockExamplesFetch(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ openapi: '3.1.0' })
    });
    mockPreviewOk(fetchMock);
    mockTypecheckReady(fetchMock);
    vi.stubGlobal('fetch', fetchMock);

    render(<App />);

    expect(await screen.findByTestId('editor-input')).toBeInTheDocument();
    expect(await screen.findByText('Simple Pay')).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /load example/i }));

    expect(screen.getByText('Choose a starting point')).toBeInTheDocument();
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
    expect(screen.queryByRole('button', { name: /run simulation/i })).not.toBeInTheDocument();
    expect(screen.getByText('Fix validation issues to enable simulation.')).toBeInTheDocument();
  });

  it('auto-validates on editor changes and shows diagnostics', async () => {
    const fetchMock = vi.fn();
    mockExamplesFetch(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ openapi: '3.1.0' })
    });
    mockPreviewOk(fetchMock);
    mockTypecheckReady(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: false,
      status: 400,
      json: async () => ({
        result: 'error',
        error: {
          subcode: 'INVALID_CONTRACT',
          message: 'Invalid contract',
          diagnostics: [{ message: 'Expected Close but found malformed when' }]
        }
      })
    });
    fetchMock.mockResolvedValueOnce({
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
              message: 'Expected Close but found malformed when',
              hint: 'Fix malformed node.'
            }
          ],
          warnings: []
        },
        error: null
      })
    });
    vi.stubGlobal('fetch', fetchMock);

    render(<App />);
    const input = await screen.findByTestId('editor-input');

    fireEvent.change(input, { target: { value: 'edited-contract' } });
    await new Promise((resolve) => setTimeout(resolve, 700));

    expect(await screen.findByText('Invalid contract')).toBeInTheDocument();
    expect((await screen.findAllByText('Expected Close but found malformed when')).length).toBe(2);
  });

  it('progresses to the next choice after applying the first input', async () => {
    const fetchMock = vi.fn();
    mockExamplesFetch(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ openapi: '3.1.0' })
    });
    mockPreviewOk(fetchMock);
    mockTypecheckReady(fetchMock);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'v1',
          state: { min_time: '10' },
          inputs: [
            {
              choice: {
                id: { ChoiceId: { name: 'FirstChoice', party: { Role: 'Alice' } } },
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
        success: {
          contract_yaml: 'v2',
          state: { min_time: '11' },
          warnings: []
        },
        error: null
      })
    });
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({
        result: 'ok',
        success: {
          contract_yaml: 'v2',
          state: { min_time: '11' },
          inputs: [
            {
              choice: {
                id: { ChoiceId: { name: 'SecondChoice', party: { Role: 'Bob' } } },
                bounds: [{ from: '10', to: '20' }]
              }
            }
          ]
        },
        error: null
      })
    });
    vi.stubGlobal('fetch', fetchMock);

    render(<App />);
    expect(await screen.findByText('Simple Pay')).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /run simulation/i }));

    const select = await screen.findByLabelText('Available input');
    expect(select).toHaveTextContent('FirstChoice');

    await user.clear(screen.getByLabelText('Choice value'));
    await user.type(screen.getByLabelText('Choice value'), '3');
    await user.click(screen.getByRole('button', { name: /apply input/i }));

    expect(
      await screen.findByText('Preview succeeded with 1 available input(s)')
    ).toBeInTheDocument();
    expect(await screen.findByLabelText('Available input')).toHaveTextContent('SecondChoice');
  });
});
