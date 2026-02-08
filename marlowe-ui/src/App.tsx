import { useEffect, useMemo, useState } from 'react';
import Editor from '@monaco-editor/react';
import { simulateContract, simulateStep, validateContract } from './api/client';
import type { SimulationInput, SimulationResponse } from './api/client';
import './styles.css';

const OPEN_API_URL = 'http://127.0.0.1:3000/openapi.json';

type Example = {
  id: string;
  name: string;
  description: string;
  language: string;
  code: string;
  file: string;
};

type ValidationState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'success'; valid: boolean; diagnostics: string[] }
  | { status: 'error'; message: string };

type SimulationState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'success'; result: SimulationResponse }
  | { status: 'error'; message: string };

export default function App() {
  const [isModalOpen, setModalOpen] = useState(false);
  const [selectedExample, setSelectedExample] = useState<Example | null>(null);
  const [examples, setExamples] = useState<Example[]>([]);
  const [code, setCode] = useState<string>('');
  const [isLoadingExamples, setLoadingExamples] = useState(true);
  const [examplesError, setExamplesError] = useState<string | null>(null);
  const [apiStatus, setApiStatus] = useState<'idle' | 'connecting' | 'connected' | 'error'>('idle');
  const [apiMessage, setApiMessage] = useState('API is not connected.');
  const [validationState, setValidationState] = useState<ValidationState>({ status: 'idle' });
  const [simulationState, setSimulationState] = useState<SimulationState>({ status: 'idle' });
  const [isValidationRunning, setValidationRunning] = useState(false);
  const [isSimulationRunning, setSimulationRunning] = useState(false);
  const [selectedInputKey, setSelectedInputKey] = useState<string>('');
  const [choiceValue, setChoiceValue] = useState<string>('0');

  const activeLanguage = selectedExample?.language ?? 'yaml';

  const checkApiConnection = async (): Promise<boolean> => {
    setApiStatus('connecting');
    setApiMessage('Connecting to API...');

    try {
      const response = await fetch(OPEN_API_URL);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }

      const spec = (await response.json()) as Record<string, unknown>;
      if (typeof spec.openapi !== 'string') {
        throw new Error('Missing openapi field');
      }

      setApiStatus('connected');
      setApiMessage('');
      return true;
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setApiStatus('error');
      setApiMessage(`API connection failed: ${message}`);
      return false;
    }
  };

  useEffect(() => {
    const loadExamples = async () => {
      try {
        const response = await fetch('/examples/index.json');
        if (!response.ok) {
          throw new Error(`Failed to load examples: ${response.status}`);
        }
        const data = (await response.json()) as Example[];
        setExamples(data);
        if (data[0]) {
          setSelectedExample(data[0]);
          setCode(data[0].code);
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : 'Unknown error';
        setExamplesError(message);
      } finally {
        setLoadingExamples(false);
      }
    };

    loadExamples();
    void checkApiConnection();
  }, []);

  const exampleCards = useMemo(
    () =>
      examples.map((example) => (
        <button
          key={example.id}
          className="example-card"
          type="button"
          onClick={() => {
            setSelectedExample(example);
            setCode(example.code);
            setModalOpen(false);
          }}
        >
          <div className="example-card__header">
            <span className="pill">{example.language.toUpperCase()}</span>
            <h3>{example.name}</h3>
          </div>
          <p>{example.description}</p>
        </button>
      )),
    [examples]
  );

  const runPreview = async (yaml: string, state: Record<string, unknown> | null = null) => {
    const result = await simulateContract(yaml, state);
    setSimulationState({ status: 'success', result });

    if (result.inputs[0]) {
      setSelectedInputKey(JSON.stringify(result.inputs[0]));
      if (result.inputs[0].kind === 'choice') {
        setChoiceValue(result.inputs[0].bounds[0]?.from ?? '0');
      }
    } else {
      setSelectedInputKey('');
    }
  };

  const ensureApiConnection = async (): Promise<boolean> => {
    if (apiStatus === 'connected') {
      return true;
    }

    return checkApiConnection();
  };

  const handleValidate = async () => {
    const connected = await ensureApiConnection();
    if (!connected) {
      setValidationState({ status: 'error', message: 'API not connected.' });
      return;
    }

    setValidationRunning(true);
    setValidationState({ status: 'loading' });
    try {
      const result = await validateContract(code);
      setValidationState({
        status: 'success',
        valid: result.valid,
        diagnostics: result.diagnostics
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setValidationState({ status: 'error', message });
    } finally {
      setValidationRunning(false);
    }
  };

  const handleSimulate = async () => {
    const connected = await ensureApiConnection();
    if (!connected) {
      setSimulationState({ status: 'error', message: 'API not connected.' });
      return;
    }

    setSimulationRunning(true);
    setSimulationState({ status: 'loading' });
    try {
      await runPreview(code);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setSimulationState({ status: 'error', message });
    } finally {
      setSimulationRunning(false);
    }
  };

  const selectedSimulationInput =
    simulationState.status === 'success'
      ? (simulationState.result.inputs.find(
          (input) => JSON.stringify(input) === selectedInputKey
        ) ?? null)
      : null;

  const handleApplyInput = async () => {
    if (simulationState.status !== 'success' || !selectedSimulationInput) {
      return;
    }

    setSimulationRunning(true);
    setSimulationState({ status: 'loading' });
    try {
      const stepResult = await simulateStep(
        simulationState.result.context,
        selectedSimulationInput,
        choiceValue
      );
      await runPreview(stepResult.context.contractYaml, stepResult.context.state);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setSimulationState({ status: 'error', message });
    } finally {
      setSimulationRunning(false);
    }
  };

  const renderInputLabel = (input: SimulationInput): string => {
    if (input.kind === 'choice') {
      const bounds = input.bounds.map((bound) => `${bound.from}..${bound.to}`).join(', ');
      return `Choice: ${input.name} (${bounds})`;
    }

    if (input.kind === 'deposit') {
      return `Deposit: ${input.amount}`;
    }

    return 'Notify';
  };

  return (
    <div className="app">
      <header className="app__header">
        <div>
          <p className="eyebrow">Marlowe Studio</p>
          <h1>Contract Playground</h1>
          <p className="subtitle">
            Draft and test smart contract ideas. Hooked to an API-ready workflow.
          </p>
        </div>
        <div className="header-actions">
          <button className="primary" type="button" onClick={() => setModalOpen(true)}>
            Load example
          </button>
          {apiStatus !== 'connected' ? (
            <span className={`api-warning api-warning--${apiStatus}`}>{apiMessage}</span>
          ) : null}
        </div>
      </header>

      <main className="app__content">
        <section className="editor">
          <div className="editor__toolbar">
            <div>
              <span className="label">Active example</span>
              <strong>{selectedExample?.name ?? 'Starter'}</strong>
            </div>
            <div>
              <span className="label">Language</span>
              <strong>{activeLanguage.toUpperCase()}</strong>
            </div>
          </div>
          <div className="editor__pane">
            {isLoadingExamples ? (
              <div className="editor__placeholder">Loading examples...</div>
            ) : examplesError ? (
              <div className="editor__placeholder error">
                Unable to load examples. {examplesError}
              </div>
            ) : (
              <Editor
                height="100%"
                defaultLanguage={activeLanguage}
                language={activeLanguage}
                theme="vs-dark"
                value={code}
                onChange={(value) => setCode(value ?? '')}
                options={{
                  fontSize: 14,
                  minimap: { enabled: false },
                  scrollBeyondLastLine: false,
                  smoothScrolling: true,
                  padding: { top: 16, bottom: 16 }
                }}
              />
            )}
          </div>
        </section>

        <aside className="side-panel">
          <div className="side-panel__header">
            <p className="eyebrow">Next up</p>
            <h2>Simulation Panel</h2>
            <p className="subtitle">
              This space will host inputs, validation, and simulation outputs.
            </p>
          </div>
          <div className="side-panel__body">
            <div className="panel-card">
              <h3>Inputs</h3>
              <p>Add parameter controls tied to contract fields.</p>
            </div>
            <div className="panel-card">
              <h3>Validation</h3>
              <p>Surface schema and logic checks in real-time.</p>
              <button
                className="panel-action"
                type="button"
                onClick={handleValidate}
                disabled={apiStatus !== 'connected' || isValidationRunning}
              >
                Run validation
              </button>
              {validationState.status === 'idle' ? (
                <p className="panel-result">No validation run yet.</p>
              ) : null}
              {validationState.status === 'loading' ? (
                <p className="panel-result">Validation in progress...</p>
              ) : null}
              {validationState.status === 'error' ? (
                <p className="panel-result panel-result--error">
                  Validation failed: {validationState.message}
                </p>
              ) : null}
              {validationState.status === 'success' ? (
                <div className="panel-block">
                  <div
                    className={`panel-badge ${
                      validationState.valid ? 'panel-badge--ok' : 'panel-badge--error'
                    }`}
                  >
                    {validationState.valid ? 'Valid contract' : 'Invalid contract'}
                  </div>
                  {validationState.diagnostics.length === 0 ? (
                    <p className="panel-result">No diagnostics.</p>
                  ) : (
                    <ul className="panel-list">
                      {validationState.diagnostics.map((diagnostic, index) => (
                        <li key={`${diagnostic}-${index}`}>{diagnostic}</li>
                      ))}
                    </ul>
                  )}
                </div>
              ) : null}
            </div>
            <div className="panel-card">
              <h3>Simulation</h3>
              <p>Preview outcomes and step through required inputs.</p>
              <button
                className="panel-action"
                type="button"
                onClick={handleSimulate}
                disabled={apiStatus !== 'connected' || isSimulationRunning}
              >
                Run simulation
              </button>
              {simulationState.status === 'idle' ? (
                <p className="panel-result">No simulation run yet.</p>
              ) : null}
              {simulationState.status === 'loading' ? (
                <p className="panel-result">Simulation in progress...</p>
              ) : null}
              {simulationState.status === 'error' ? (
                <p className="panel-result panel-result--error">
                  Simulation failed: {simulationState.message}
                </p>
              ) : null}
              {simulationState.status === 'success' ? (
                <div className="panel-block">
                  <p className="panel-result">{simulationState.result.summary}</p>
                  {simulationState.result.inputs.length > 0 ? (
                    <div className="choice-form">
                      <label className="choice-form__label" htmlFor="input-select">
                        Available input
                      </label>
                      <select
                        id="input-select"
                        className="choice-form__select"
                        value={selectedInputKey}
                        onChange={(event) => setSelectedInputKey(event.target.value)}
                      >
                        {simulationState.result.inputs.map((input) => {
                          const key = JSON.stringify(input);
                          return (
                            <option key={key} value={key}>
                              {renderInputLabel(input)}
                            </option>
                          );
                        })}
                      </select>
                      {selectedSimulationInput?.kind === 'choice' ? (
                        <>
                          <label className="choice-form__label" htmlFor="choice-value">
                            Choice value
                          </label>
                          <input
                            id="choice-value"
                            className="choice-form__input"
                            value={choiceValue}
                            onChange={(event) => setChoiceValue(event.target.value)}
                          />
                        </>
                      ) : null}
                      <button
                        className="panel-action"
                        type="button"
                        disabled={isSimulationRunning || !selectedSimulationInput}
                        onClick={handleApplyInput}
                      >
                        Apply input
                      </button>
                    </div>
                  ) : (
                    <div className="panel-badge panel-badge--ok">No further inputs available</div>
                  )}
                  {simulationState.result.warnings.length === 0 ? (
                    <div className="panel-badge panel-badge--ok">No warnings</div>
                  ) : (
                    <ul className="panel-list">
                      {simulationState.result.warnings.map((warning, index) => (
                        <li key={`${warning}-${index}`}>{warning}</li>
                      ))}
                    </ul>
                  )}
                </div>
              ) : null}
            </div>
          </div>
        </aside>
      </main>

      {isModalOpen ? (
        <div className="modal" role="dialog" aria-modal="true">
          <div className="modal__overlay" onClick={() => setModalOpen(false)} />
          <div className="modal__content">
            <div className="modal__header">
              <div>
                <p className="eyebrow">Examples</p>
                <h2>Choose a starting point</h2>
                <p className="subtitle">Load curated snippets to explore structure and style.</p>
              </div>
              <button className="ghost" type="button" onClick={() => setModalOpen(false)}>
                Close
              </button>
            </div>
            <div className="modal__grid">
              {examples.length === 0 ? (
                <p className="modal__empty">No examples available.</p>
              ) : (
                exampleCards
              )}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
