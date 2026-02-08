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

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('App', () => {
  it('loads examples and opens the modal', async () => {
    const fetchMock = vi.fn();
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => examples
    });

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
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => examples
    });
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ openapi: '3.1.0' })
    });

    vi.stubGlobal('fetch', fetchMock);

    render(<App />);
    expect(await screen.findByText('Simple Pay')).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /connect api/i }));

    expect(await screen.findByText('Connected (3.1.0)')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenNthCalledWith(2, 'http://127.0.0.1:3000/openapi.json');
  });

  it('shows an error when OpenAPI fetch fails', async () => {
    const fetchMock = vi.fn();
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => examples
    });
    fetchMock.mockResolvedValueOnce({
      ok: false,
      status: 500,
      json: async () => ({})
    });

    vi.stubGlobal('fetch', fetchMock);

    render(<App />);
    expect(await screen.findByText('Simple Pay')).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /connect api/i }));

    expect(await screen.findByText('Connection failed: HTTP 500')).toBeInTheDocument();
  });
});
