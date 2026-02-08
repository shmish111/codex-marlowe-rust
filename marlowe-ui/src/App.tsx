import { useEffect, useMemo, useState } from 'react';
import Editor from '@monaco-editor/react';
import { simulateContract, validateContract } from './api/client';
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

export default function App() {
  const [isModalOpen, setModalOpen] = useState(false);
  const [selectedExample, setSelectedExample] = useState<Example | null>(null);
  const [examples, setExamples] = useState<Example[]>([]);
  const [code, setCode] = useState<string>('');
  const [isLoadingExamples, setLoadingExamples] = useState(true);
  const [examplesError, setExamplesError] = useState<string | null>(null);
  const [apiStatus, setApiStatus] = useState<'idle' | 'connecting' | 'connected' | 'error'>('idle');
  const [apiMessage, setApiMessage] = useState('Not connected');
  const [validationResult, setValidationResult] = useState<string>('No validation run yet.');
  const [simulationResult, setSimulationResult] = useState<string>('No simulation run yet.');
  const [isValidationRunning, setValidationRunning] = useState(false);
  const [isSimulationRunning, setSimulationRunning] = useState(false);

  const activeLanguage = selectedExample?.language ?? 'yaml';

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

  const handleConnectApi = async () => {
    setApiStatus('connecting');
    setApiMessage('Connecting...');

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
      setApiMessage(`Connected (${spec.openapi})`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setApiStatus('error');
      setApiMessage(`Connection failed: ${message}`);
    }
  };

  const handleValidate = async () => {
    setValidationRunning(true);
    setValidationResult('Validating...');
    try {
      const result = await validateContract(code);
      const diagnostics = result.diagnostics.join(', ');
      setValidationResult(
        result.valid
          ? diagnostics
            ? `Valid. Diagnostics: ${diagnostics}`
            : 'Valid with no diagnostics.'
          : diagnostics
            ? `Invalid. Diagnostics: ${diagnostics}`
            : 'Invalid with no diagnostics.'
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setValidationResult(`Validation failed: ${message}`);
    } finally {
      setValidationRunning(false);
    }
  };

  const handleSimulate = async () => {
    setSimulationRunning(true);
    setSimulationResult('Simulating...');
    try {
      const result = await simulateContract(code);
      const warnings = result.warnings.join(', ');
      setSimulationResult(
        warnings ? `${result.summary}. Warnings: ${warnings}` : `${result.summary}. No warnings.`
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setSimulationResult(`Simulation failed: ${message}`);
    } finally {
      setSimulationRunning(false);
    }
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
          <button
            className="ghost"
            type="button"
            onClick={handleConnectApi}
            disabled={apiStatus === 'connecting'}
          >
            Connect API
          </button>
          <span className={`api-status api-status--${apiStatus}`}>{apiMessage}</span>
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
                disabled={isValidationRunning}
              >
                Run validation stub
              </button>
              <p className="panel-result">{validationResult}</p>
            </div>
            <div className="panel-card">
              <h3>Simulation</h3>
              <p>Preview outcomes once the backend API is connected.</p>
              <button
                className="panel-action"
                type="button"
                onClick={handleSimulate}
                disabled={isSimulationRunning}
              >
                Run simulation stub
              </button>
              <p className="panel-result">{simulationResult}</p>
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
