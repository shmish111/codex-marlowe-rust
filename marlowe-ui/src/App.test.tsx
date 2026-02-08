import { render, screen } from '@testing-library/react';
import { act } from 'react';
import userEvent from '@testing-library/user-event';
import { vi } from 'vitest';
import App from './App';

vi.mock('@monaco-editor/react', () => ({
  default: () => <div data-testid="editor" />
}));

describe('App', () => {
  it('loads examples and opens the modal', async () => {
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

    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        Promise.resolve({
          ok: true,
          json: async () => examples
        })
      )
    );

    await act(async () => {
      render(<App />);
    });

    await act(async () => {
      expect(await screen.findByTestId('editor')).toBeInTheDocument();
      expect(await screen.findByText('Simple Pay')).toBeInTheDocument();
    });

    const user = userEvent.setup();
    await act(async () => {
      await user.click(screen.getByRole('button', { name: /load example/i }));
    });

    expect(screen.getByText('Choose a starting point')).toBeInTheDocument();

    vi.unstubAllGlobals();
  });
});
