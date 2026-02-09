import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Editor from '@monaco-editor/react';
import {
  simulateContract,
  simulateStep,
  simulateTimeoutStep,
  validateContract
} from './api/client';
import type {
  SimulationInput,
  SimulationResponse,
  SimulationSourceSpan,
  SimulationTraceEvent,
  ValidationDiagnostic,
  ValidationExplainItem,
  ValidationSummary
} from './api/client';
import type * as Monaco from 'monaco-editor';
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
  | {
      status: 'success';
      valid: boolean;
      diagnostics: ValidationDiagnostic[];
      readyToRun?: boolean;
      summary?: ValidationSummary;
      blocking?: ValidationExplainItem[];
      warnings?: ValidationExplainItem[];
    }
  | { status: 'error'; message: string };

type SimulationState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'success'; result: SimulationResponse }
  | { status: 'error'; message: string };

type UnresolvedItem = {
  name: string;
  expectedType?: string;
};

function stableStringify(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function summarizeStateDiff(
  previousState: Record<string, unknown> | null,
  nextState: Record<string, unknown> | null
): string[] {
  if (!previousState && !nextState) {
    return [];
  }

  if (!previousState && nextState) {
    return ['Initialized simulation state'];
  }

  if (!nextState) {
    return ['State cleared'];
  }

  const changes: string[] = [];
  const previousMinTime =
    typeof previousState?.min_time === 'string' ? previousState.min_time : '0';
  const nextMinTime = typeof nextState.min_time === 'string' ? nextState.min_time : '0';
  if (previousMinTime !== nextMinTime) {
    changes.push(`Min time changed: ${previousMinTime} -> ${nextMinTime}`);
  }

  const previousAccounts = Array.isArray(previousState?.accounts) ? previousState.accounts : [];
  const nextAccounts = Array.isArray(nextState.accounts) ? nextState.accounts : [];
  if (stableStringify(previousAccounts) !== stableStringify(nextAccounts)) {
    changes.push(`Accounts changed (${nextAccounts.length} entries)`);
  }

  const previousChoices = Array.isArray(previousState?.choices) ? previousState.choices : [];
  const nextChoices = Array.isArray(nextState.choices) ? nextState.choices : [];
  if (stableStringify(previousChoices) !== stableStringify(nextChoices)) {
    changes.push(`Choices changed (${nextChoices.length} entries)`);
  }

  const previousBoundValues =
    previousState?.bound_values && typeof previousState.bound_values === 'object'
      ? (previousState.bound_values as Record<string, unknown>)
      : {};
  const nextBoundValues =
    nextState.bound_values && typeof nextState.bound_values === 'object'
      ? (nextState.bound_values as Record<string, unknown>)
      : {};
  if (stableStringify(previousBoundValues) !== stableStringify(nextBoundValues)) {
    changes.push(`Bound values changed (${Object.keys(nextBoundValues).length} entries)`);
  }

  return changes.length > 0 ? changes : ['No state changes'];
}

function renderStateValue(value: unknown): string {
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return stableStringify(value);
}

function formatWarningFieldName(name: string): string {
  return name.replace(/_/g, ' ');
}

function findLineForContractPath(contractPath: string, sourceCode: string): number | null {
  const lines = sourceCode.split('\n');
  const tokens = (contractPath.match(/[A-Za-z_][A-Za-z0-9_]*/g) ?? []).filter(
    (token) => token !== '$'
  );

  if (tokens.length === 0) {
    return null;
  }

  for (const token of tokens.slice().reverse()) {
    const pattern = new RegExp(`\\b${token}\\b`);
    for (let index = 0; index < lines.length; index += 1) {
      if (pattern.test(lines[index])) {
        return index + 1;
      }
    }
  }

  return null;
}

function findNextTimeout(contractYaml: string, minTime: string): string | null {
  const current = Number.parseInt(minTime, 10);
  if (!Number.isFinite(current)) {
    return null;
  }

  const candidates = new Set<number>();
  const timeoutObjectPattern = /timeout:\s*\{\s*Timeout:\s*"?(\d+)"?\s*\}/g;
  const timeoutNestedPattern = /timeout:\s*\n[ \t]+Timeout:\s*"?(\d+)"?/g;
  const timeoutScalarPattern = /timeout:\s*"?(\d+)"?/g;

  for (const match of contractYaml.matchAll(timeoutObjectPattern)) {
    const value = Number.parseInt(match[1], 10);
    if (Number.isFinite(value)) {
      candidates.add(value);
    }
  }

  for (const match of contractYaml.matchAll(timeoutNestedPattern)) {
    const value = Number.parseInt(match[1], 10);
    if (Number.isFinite(value)) {
      candidates.add(value);
    }
  }

  for (const match of contractYaml.matchAll(timeoutScalarPattern)) {
    const value = Number.parseInt(match[1], 10);
    if (Number.isFinite(value)) {
      candidates.add(value);
    }
  }

  const nextValues = Array.from(candidates).filter((value) => value > current);
  if (nextValues.length === 0) {
    return null;
  }

  return String(Math.min(...nextValues));
}

function getDetailString(diagnostic: ValidationDiagnostic, key: string): string | undefined {
  const value = diagnostic.details?.[key];
  return typeof value === 'string' ? value : undefined;
}

function normalizeUnresolvedName(
  value: string | undefined,
  preferredPrefix: '?' | '$'
): string | undefined {
  if (!value) {
    return undefined;
  }
  if (value.startsWith('?') || value.startsWith('$')) {
    return value;
  }
  return `${preferredPrefix}${value}`;
}

function isParameterDiagnostic(diagnostic: ValidationDiagnostic): boolean {
  const subcode = diagnostic.subcode?.toLowerCase() ?? '';
  if (subcode.includes('param')) {
    return true;
  }
  return diagnostic.message.toLowerCase().includes("parameter '");
}

function getUnresolvedName(diagnostic: ValidationDiagnostic): string | undefined {
  const preferredPrefix: '?' | '$' = isParameterDiagnostic(diagnostic) ? '$' : '?';
  const detailName = normalizeUnresolvedName(getDetailString(diagnostic, 'name'), preferredPrefix);
  if (detailName) {
    return detailName;
  }

  const fromMessage =
    diagnostic.message.match(/([?$][A-Za-z_][A-Za-z0-9_]*)/)?.[1] ??
    diagnostic.message.match(/parameter ['"`]([A-Za-z_][A-Za-z0-9_]*)['"`]/i)?.[1];
  if (fromMessage) {
    return normalizeUnresolvedName(fromMessage, preferredPrefix);
  }

  const fromPath = diagnostic.path?.match(/([?$][A-Za-z_][A-Za-z0-9_]*)/)?.[1];
  return normalizeUnresolvedName(fromPath, preferredPrefix);
}

function getExpectedType(diagnostic: ValidationDiagnostic): string | undefined {
  const detailType = getDetailString(diagnostic, 'type');
  if (detailType) {
    return detailType;
  }

  const fromMessage =
    diagnostic.message.match(/inferred type ([A-Za-z_][A-Za-z0-9_]*)/i)?.[1] ??
    diagnostic.message.match(/\btype:\s*([A-Za-z_][A-Za-z0-9_]*)/i)?.[1];
  return fromMessage;
}

function isHoleDiagnostic(diagnostic: ValidationDiagnostic): boolean {
  if (getUnresolvedName(diagnostic)) {
    return true;
  }

  const subcode = diagnostic.subcode?.toLowerCase() ?? '';
  if (subcode.includes('param') || subcode.includes('hole') || subcode.includes('unresolved')) {
    return true;
  }

  return diagnostic.message.toLowerCase().includes('must be instantiated');
}

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
  const [simulationHistory, setSimulationHistory] = useState<SimulationResponse[]>([]);
  const [simulationTraceChunks, setSimulationTraceChunks] = useState<SimulationTraceEvent[][]>([]);
  const [simulationCursorSpan, setSimulationCursorSpan] = useState<SimulationSourceSpan | null>(
    null
  );
  const [stateChanges, setStateChanges] = useState<string[]>([]);
  const [isSimulationRunning, setSimulationRunning] = useState(false);
  const [hasUserEdited, setHasUserEdited] = useState(false);
  const [selectedInputKey, setSelectedInputKey] = useState<string>('');
  const [choiceValue, setChoiceValue] = useState<string>('0');
  const [timeoutTarget, setTimeoutTarget] = useState<string>('');
  const validationRunIdRef = useRef(0);
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<typeof Monaco | null>(null);
  const simulationDecorationIdsRef = useRef<string[]>([]);

  const activeLanguage = selectedExample?.language ?? 'yaml';

  const checkApiConnection = useCallback(async (): Promise<boolean> => {
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
  }, []);

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
  }, [checkApiConnection]);

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
            setHasUserEdited(false);
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
    setSimulationHistory((previousHistory) => {
      const previousState =
        previousHistory.length > 0
          ? (previousHistory[previousHistory.length - 1].context.state ?? null)
          : null;
      setStateChanges(summarizeStateDiff(previousState, result.context.state ?? null));
      return [...previousHistory, result];
    });
    const suggestedTimeout = findNextTimeout(result.context.contractYaml, result.context.minTime);
    setTimeoutTarget((previousTarget) => {
      if (suggestedTimeout) {
        return suggestedTimeout;
      }
      if (previousTarget) {
        return previousTarget;
      }
      return result.context.minTime;
    });

    if (result.inputs[0]) {
      setSelectedInputKey(JSON.stringify(result.inputs[0]));
      if (result.inputs[0].kind === 'choice') {
        setChoiceValue(result.inputs[0].bounds[0]?.from ?? '0');
      }
    } else {
      setSelectedInputKey('');
    }
  };

  const ensureApiConnection = useCallback(async (): Promise<boolean> => {
    if (apiStatus === 'connected') {
      return true;
    }

    return checkApiConnection();
  }, [apiStatus, checkApiConnection]);

  const runValidation = useCallback(
    async (sourceCode: string) => {
      const connected = await ensureApiConnection();
      if (!connected) {
        setValidationState({ status: 'error', message: 'API not connected.' });
        return;
      }

      const runId = ++validationRunIdRef.current;
      setValidationState({ status: 'loading' });
      try {
        const result = await validateContract(sourceCode);
        if (runId !== validationRunIdRef.current) {
          return;
        }

        setValidationState({
          status: 'success',
          valid: result.valid,
          diagnostics: result.diagnostics,
          readyToRun: result.readyToRun,
          summary: result.summary,
          blocking: result.blocking,
          warnings: result.warnings
        });
      } catch (error) {
        if (runId !== validationRunIdRef.current) {
          return;
        }

        const message = error instanceof Error ? error.message : 'Unknown error';
        setValidationState({ status: 'error', message });
      }
    },
    [ensureApiConnection]
  );

  const handleSimulate = async () => {
    const connected = await ensureApiConnection();
    if (!connected) {
      setSimulationState({ status: 'error', message: 'API not connected.' });
      return;
    }

    setSimulationRunning(true);
    setSimulationState({ status: 'loading' });
    setSimulationHistory([]);
    setSimulationTraceChunks([]);
    setStateChanges([]);
    setTimeoutTarget('');
    setSimulationCursorSpan({ line: 1, column: 1, endLine: 1, endColumn: 1 });
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

  const simulationTrace = useMemo(
    () => simulationTraceChunks.flatMap((chunk) => chunk),
    [simulationTraceChunks]
  );
  const currentSimulationPath = useMemo(
    () => simulationTrace[simulationTrace.length - 1]?.contractPath ?? null,
    [simulationTrace]
  );
  const currentSimulationSpan = useMemo(
    () => simulationTrace[simulationTrace.length - 1]?.span ?? simulationCursorSpan,
    [simulationTrace, simulationCursorSpan]
  );

  const currentSimState =
    simulationState.status === 'success' ? simulationState.result.context.state : null;
  const stateAccounts =
    currentSimState && Array.isArray(currentSimState.accounts) ? currentSimState.accounts : [];
  const stateChoices =
    currentSimState && Array.isArray(currentSimState.choices) ? currentSimState.choices : [];
  const stateBoundValues =
    currentSimState &&
    currentSimState.bound_values &&
    typeof currentSimState.bound_values === 'object'
      ? (currentSimState.bound_values as Record<string, unknown>)
      : {};
  const stateMinTime =
    currentSimState && typeof currentSimState.min_time === 'string'
      ? currentSimState.min_time
      : '0';

  const unresolvedItems = useMemo<UnresolvedItem[]>(() => {
    if (validationState.status !== 'success' || validationState.valid) {
      return [];
    }

    const entries = new Map<string, string | undefined>();

    for (const diagnostic of validationState.diagnostics) {
      if (!isHoleDiagnostic(diagnostic)) {
        continue;
      }

      const name = getUnresolvedName(diagnostic);
      if (!name) {
        continue;
      }

      const expectedType = getExpectedType(diagnostic);
      if (!entries.has(name) || (!entries.get(name) && expectedType)) {
        entries.set(name, expectedType);
      }
    }

    return Array.from(entries.entries()).map(([name, expectedType]) => ({
      name,
      expectedType
    }));
  }, [validationState]);

  const validationSummary = useMemo(() => {
    if (validationState.status !== 'success') {
      return null;
    }

    if (validationState.readyToRun === true && validationState.diagnostics.length === 0) {
      return { kind: 'valid' as const, label: 'Valid contract' };
    }

    if (validationState.readyToRun === false) {
      if ((validationState.summary?.errorCount ?? 0) === 0) {
        return { kind: 'incomplete' as const, label: 'Incomplete contract' };
      }
      return { kind: 'invalid' as const, label: 'Invalid contract' };
    }

    if (validationState.valid) {
      return { kind: 'valid' as const, label: 'Valid contract' };
    }

    const hasOnlyHoles =
      validationState.diagnostics.length > 0 &&
      validationState.diagnostics.every((diagnostic) => isHoleDiagnostic(diagnostic));

    if (hasOnlyHoles) {
      return { kind: 'incomplete' as const, label: 'Incomplete contract' };
    }

    return { kind: 'invalid' as const, label: 'Invalid contract' };
  }, [validationState]);

  const explainBlockingItems =
    validationState.status === 'success' ? (validationState.blocking ?? []) : [];
  const explainWarningItems =
    validationState.status === 'success' ? (validationState.warnings ?? []) : [];

  const canRunSimulation =
    validationState.status === 'success' &&
    (typeof validationState.readyToRun === 'boolean'
      ? validationState.readyToRun
      : validationState.valid && validationState.diagnostics.length === 0);

  const nextTimeout =
    simulationState.status === 'success'
      ? findNextTimeout(
          simulationState.result.context.contractYaml,
          simulationState.result.context.minTime
        )
      : null;
  const canAdvanceTimeout =
    simulationState.status === 'success' &&
    (() => {
      const target = Number.parseInt(timeoutTarget, 10);
      const current = Number.parseInt(simulationState.result.context.minTime, 10);
      return Number.isFinite(target) && Number.isFinite(current) && target >= current;
    })();

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
      const nextCursor =
        stepResult.traceEvents[stepResult.traceEvents.length - 1]?.span ??
        stepResult.initialPosition ??
        null;
      if (nextCursor) {
        setSimulationCursorSpan(nextCursor);
      }
      if (stepResult.traceEvents.length > 0) {
        setSimulationTraceChunks((previous) => [...previous, stepResult.traceEvents]);
      }
      await runPreview(stepResult.context.contractYaml, stepResult.context.state);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setSimulationState({ status: 'error', message });
    } finally {
      setSimulationRunning(false);
    }
  };

  const handleAdvanceTimeout = async () => {
    if (simulationState.status !== 'success' || !canAdvanceTimeout) {
      return;
    }

    const parsedTarget = Number.parseInt(timeoutTarget, 10);
    const targetTimeout = String(parsedTarget);

    setSimulationRunning(true);
    setSimulationState({ status: 'loading' });
    try {
      const stepResult = await simulateTimeoutStep(simulationState.result.context, targetTimeout);
      const nextCursor =
        stepResult.traceEvents[stepResult.traceEvents.length - 1]?.span ??
        stepResult.initialPosition ??
        null;
      if (nextCursor) {
        setSimulationCursorSpan(nextCursor);
      }
      if (stepResult.traceEvents.length > 0) {
        setSimulationTraceChunks((previous) => [...previous, stepResult.traceEvents]);
      }
      await runPreview(stepResult.context.contractYaml, stepResult.context.state);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      setSimulationState({ status: 'error', message });
    } finally {
      setSimulationRunning(false);
    }
  };

  const handleUndoStep = () => {
    setSimulationHistory((previousHistory) => {
      if (previousHistory.length <= 1) {
        return previousHistory;
      }

      const nextHistory = previousHistory.slice(0, -1);
      const restored = nextHistory[nextHistory.length - 1];
      setSimulationState({ status: 'success', result: restored });
      setSimulationTraceChunks((previousChunks) => {
        const nextChunks = previousChunks.length > 0 ? previousChunks.slice(0, -1) : previousChunks;
        const nextCursor = nextChunks[nextChunks.length - 1]?.[
          nextChunks[nextChunks.length - 1].length - 1
        ]?.span ?? { line: 1, column: 1, endLine: 1, endColumn: 1 };
        setSimulationCursorSpan(nextCursor);
        return nextChunks;
      });
      setTimeoutTarget(
        findNextTimeout(restored.context.contractYaml, restored.context.minTime) ??
          restored.context.minTime
      );
      if (restored.inputs[0]) {
        setSelectedInputKey(JSON.stringify(restored.inputs[0]));
        if (restored.inputs[0].kind === 'choice') {
          setChoiceValue(restored.inputs[0].bounds[0]?.from ?? '0');
        }
      } else {
        setSelectedInputKey('');
      }

      setStateChanges(['Reverted one step']);
      return nextHistory;
    });
  };

  const handleRestartSimulation = async () => {
    if (!canRunSimulation) {
      return;
    }

    await handleSimulate();
  };

  useEffect(() => {
    if (!hasUserEdited) {
      return;
    }

    const timer = setTimeout(() => {
      void runValidation(code);
    }, 500);

    return () => {
      clearTimeout(timer);
    };
  }, [code, hasUserEdited, runValidation]);

  useEffect(() => {
    if (hasUserEdited || !code.trim() || apiStatus !== 'connected') {
      return;
    }

    void runValidation(code);
  }, [code, hasUserEdited, apiStatus, runValidation]);

  useEffect(() => {
    const editor = editorRef.current;
    const monaco = monacoRef.current;
    if (!editor || !monaco) {
      return;
    }

    const model = editor.getModel();
    if (!model) {
      return;
    }

    if (validationState.status !== 'success' || validationState.valid) {
      monaco.editor.setModelMarkers(model, 'validation', []);
      return;
    }

    const lines = code.split('\n');
    const escapeRegExp = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const findTokenPosition = (token: string) => {
      if (!token) {
        return null;
      }

      const pattern = new RegExp(`\\b${escapeRegExp(token)}\\b`);
      for (let index = 0; index < lines.length; index += 1) {
        const lineText = lines[index];
        const match = pattern.exec(lineText);
        if (match && typeof match.index === 'number') {
          const startColumn = match.index + 1;
          return {
            line: index + 1,
            column: startColumn,
            endLine: index + 1,
            endColumn: startColumn + token.length
          };
        }
      }

      for (let index = 0; index < lines.length; index += 1) {
        const lineText = lines[index];
        const charIndex = lineText.indexOf(token);
        if (charIndex >= 0) {
          const startColumn = charIndex + 1;
          return {
            line: index + 1,
            column: startColumn,
            endLine: index + 1,
            endColumn: startColumn + token.length
          };
        }
      }

      return null;
    };

    const markers: Monaco.editor.IMarkerData[] =
      validationState.diagnostics.length > 0
        ? validationState.diagnostics.map((diagnostic) => {
            const fromMessage = diagnostic.message.match(/line\s+(\d+)(?:\D+column\s+(\d+))?/i);

            const hasExplicitEndColumn = typeof diagnostic.endColumn === 'number';
            let line = diagnostic.line ?? Number(fromMessage?.[1] ?? 0);
            let column = diagnostic.column ?? Number(fromMessage?.[2] ?? 0);
            let endLine = diagnostic.endLine ?? line;
            let endColumn = diagnostic.endColumn ?? column;

            if (!line) {
              const unresolvedName = getUnresolvedName(diagnostic);
              if (unresolvedName) {
                const unresolvedPosition = findTokenPosition(unresolvedName);
                if (unresolvedPosition) {
                  line = unresolvedPosition.line;
                  column = unresolvedPosition.column;
                  endLine = unresolvedPosition.endLine;
                  endColumn = unresolvedPosition.endColumn;
                }
              }
            }

            if (!line) {
              const messageTokens = Array.from(
                diagnostic.message.matchAll(/['"`]([?A-Za-z_][?A-Za-z0-9_]*)['"`]/g)
              ).map((match) => match[1]);

              const pathTokens = diagnostic.path
                ? (diagnostic.path.match(/[A-Za-z_][A-Za-z0-9_]*/g) ?? []).filter(
                    (token) => token !== '$'
                  )
                : [];

              const tokenCandidates = Array.from(
                new Set([...messageTokens, ...pathTokens.reverse()])
              );

              for (const token of tokenCandidates) {
                const tokenPosition = findTokenPosition(token);
                if (tokenPosition) {
                  line = tokenPosition.line;
                  column = tokenPosition.column;
                  endLine = tokenPosition.endLine;
                  endColumn = tokenPosition.endColumn;
                  break;
                }
              }
            }

            const safeLine = line > 0 ? line : 1;
            const safeColumn = column > 0 ? column : 1;
            const safeEndLine = endLine && endLine > 0 ? endLine : safeLine;
            const endLineText = lines[safeEndLine - 1] ?? '';
            const safeEndColumn =
              endColumn && endColumn > 0
                ? hasExplicitEndColumn
                  ? endColumn + 1
                  : endColumn
                : Math.max(safeColumn + 1, endLineText.length + 1);

            return {
              severity: isHoleDiagnostic(diagnostic)
                ? monaco.MarkerSeverity.Warning
                : monaco.MarkerSeverity.Error,
              message: isHoleDiagnostic(diagnostic)
                ? `${getUnresolvedName(diagnostic) ?? 'Placeholder'} type: ${getExpectedType(diagnostic) ?? 'Unknown'}`
                : diagnostic.message,
              startLineNumber: safeLine,
              startColumn: safeColumn,
              endLineNumber: safeEndLine,
              endColumn: Math.max(safeEndColumn, safeColumn + 1)
            };
          })
        : [
            {
              severity: monaco.MarkerSeverity.Error,
              message: 'Invalid contract',
              startLineNumber: 1,
              startColumn: 1,
              endLineNumber: 1,
              endColumn: 2
            }
          ];

    monaco.editor.setModelMarkers(model, 'validation', markers);
  }, [validationState, code]);

  useEffect(() => {
    const editor = editorRef.current;
    const monaco = monacoRef.current;
    if (!editor || !monaco) {
      return;
    }

    const model = editor.getModel();
    if (!model) {
      return;
    }

    const resolvedRange =
      simulationState.status === 'success'
        ? (() => {
            if (currentSimulationSpan?.line && currentSimulationSpan?.column) {
              const startLine = currentSimulationSpan.line;
              const startColumn = currentSimulationSpan.column;
              const endLine = currentSimulationSpan.endLine ?? startLine;
              const endColumn = currentSimulationSpan.endColumn ?? startColumn + 1;
              return { startLine, startColumn, endLine, endColumn };
            }

            if (currentSimulationPath) {
              const lineNumber = findLineForContractPath(currentSimulationPath, code);
              if (lineNumber) {
                return {
                  startLine: lineNumber,
                  startColumn: 1,
                  endLine: lineNumber,
                  endColumn: 1
                };
              }
            }

            return null;
          })()
        : null;

    if (!resolvedRange) {
      simulationDecorationIdsRef.current = editor.deltaDecorations(
        simulationDecorationIdsRef.current,
        []
      );
      return;
    }

    simulationDecorationIdsRef.current = editor.deltaDecorations(
      simulationDecorationIdsRef.current,
      [
        {
          range: new monaco.Range(
            resolvedRange.startLine,
            resolvedRange.startColumn,
            resolvedRange.endLine,
            resolvedRange.endColumn
          ),
          options: {
            isWholeLine: resolvedRange.startLine === resolvedRange.endLine,
            className: 'simulation-current-line'
          }
        }
      ]
    );
  }, [simulationState.status, currentSimulationPath, currentSimulationSpan, code]);

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
        <h1>Marlowe Studio</h1>
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
                onMount={(editor, monaco) => {
                  editorRef.current = editor;
                  monacoRef.current = monaco;
                }}
                onChange={(value) => {
                  setCode(value ?? '');
                  setHasUserEdited(true);
                }}
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
          <section className="tool-section">
            <h2>Validation</h2>
            {validationState.status === 'idle' ? (
              <p className="panel-result">Validation runs automatically while you type.</p>
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
                    validationSummary?.kind === 'valid'
                      ? 'panel-badge--ok'
                      : validationSummary?.kind === 'incomplete'
                        ? 'panel-badge--warn'
                        : 'panel-badge--error'
                  }`}
                >
                  {validationSummary?.label}
                </div>
                {validationState.summary ? (
                  <div className="panel-block">
                    <p className="panel-result">Typecheck summary</p>
                    <div className="summary-grid">
                      <span>Errors: {validationState.summary.errorCount}</span>
                      <span>Blocking: {validationState.summary.blockingCount}</span>
                      <span>Warnings: {validationState.summary.warningCount}</span>
                      <span>Holes: {validationState.summary.holeCount}</span>
                      <span>Params: {validationState.summary.paramCount}</span>
                    </div>
                  </div>
                ) : null}
                {unresolvedItems.length > 0 ? (
                  <div className="panel-block">
                    <p className="panel-result">Unresolved placeholders</p>
                    <ul className="panel-list hole-list">
                      {unresolvedItems.map((item) => (
                        <li key={item.name}>
                          <code>{item.name}</code>
                          {` type: ${item.expectedType ?? 'Unknown'}`}
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
                {explainBlockingItems.length > 0 ? (
                  <div className="panel-block">
                    <p className="panel-result">Blocking checks</p>
                    <ul className="panel-list">
                      {explainBlockingItems.map((item, index) => (
                        <li key={`${item.code}-${item.path}-${index}`}>
                          <strong>{item.message}</strong>
                          <span className="panel-hint">{item.hint}</span>
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
                {explainWarningItems.length > 0 ? (
                  <div className="panel-block">
                    <p className="panel-result">Warnings</p>
                    <ul className="panel-list">
                      {explainWarningItems.map((item, index) => (
                        <li key={`${item.code}-${item.path}-${index}`}>
                          <strong>{item.message}</strong>
                          <span className="panel-hint">{item.hint}</span>
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
                {validationState.diagnostics.length === 0 ? (
                  <p className="panel-result">No diagnostics.</p>
                ) : validationSummary?.kind === 'incomplete' ? null : (
                  <ul className="panel-list">
                    {validationState.diagnostics.map((diagnostic, index) => (
                      <li key={`${diagnostic.message}-${index}`}>{diagnostic.message}</li>
                    ))}
                  </ul>
                )}
                {validationSummary?.kind === 'incomplete' && unresolvedItems.length === 0 ? (
                  <ul className="panel-list">
                    {validationState.diagnostics.map((diagnostic, index) => (
                      <li key={`${diagnostic.message}-${index}`}>{diagnostic.message}</li>
                    ))}
                  </ul>
                ) : null}
              </div>
            ) : null}
          </section>

          <section className="tool-section">
            <h2>Simulation</h2>
            {canRunSimulation ? (
              <div className="choice-form">
                <button
                  className="panel-action"
                  type="button"
                  onClick={handleSimulate}
                  disabled={apiStatus !== 'connected' || isSimulationRunning}
                >
                  Run simulation
                </button>
                <button
                  className="panel-action"
                  type="button"
                  onClick={handleAdvanceTimeout}
                  disabled={!canAdvanceTimeout || apiStatus !== 'connected' || isSimulationRunning}
                >
                  Set simulation time
                </button>
                <label className="choice-form__label" htmlFor="timeout-target">
                  Timeout target
                </label>
                <input
                  id="timeout-target"
                  className="choice-form__input"
                  value={timeoutTarget}
                  onChange={(event) => setTimeoutTarget(event.target.value)}
                  placeholder={nextTimeout ?? 'Enter POSIX time'}
                />
                {nextTimeout ? (
                  <button
                    className="panel-action"
                    type="button"
                    onClick={() => setTimeoutTarget(nextTimeout)}
                    disabled={apiStatus !== 'connected' || isSimulationRunning}
                  >
                    Use next timeout ({nextTimeout})
                  </button>
                ) : null}
                <button
                  className="panel-action"
                  type="button"
                  onClick={handleUndoStep}
                  disabled={simulationHistory.length <= 1 || isSimulationRunning}
                >
                  Undo step
                </button>
                <button
                  className="panel-action"
                  type="button"
                  onClick={() => void handleRestartSimulation()}
                  disabled={apiStatus !== 'connected' || isSimulationRunning}
                >
                  Restart simulation
                </button>
              </div>
            ) : (
              <p className="panel-result">Fix validation issues to enable simulation.</p>
            )}
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
                {currentSimulationPath ? (
                  <p className="panel-result">Current path: {currentSimulationPath}</p>
                ) : null}
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
                      <li key={`${warning.code}-${index}`}>
                        <strong>{warning.message}</strong>
                        <span className="panel-hint">Code: {warning.code}</span>
                        {warning.fields.map((field) => (
                          <span
                            key={`${warning.code}-${index}-${field.name}`}
                            className="panel-hint"
                          >
                            {formatWarningFieldName(field.name)}: {field.value}
                          </span>
                        ))}
                      </li>
                    ))}
                  </ul>
                )}
                {simulationTrace.length > 0 ? (
                  <div className="panel-block">
                    <p className="panel-result">Trace</p>
                    <ul className="panel-list">
                      {simulationTrace.map((event) => (
                        <li key={event.eventId}>
                          <strong>{event.label}</strong>
                          <span className="panel-hint">{event.contractPath}</span>
                          {event.warningCode ? (
                            <span className="panel-hint">Warning: {event.warningCode}</span>
                          ) : null}
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
                <div className="panel-block">
                  <p className="panel-result">State</p>
                  <ul className="panel-list">
                    <li>Min time: {stateMinTime}</li>
                    <li>Accounts: {stateAccounts.length}</li>
                    <li>Choices: {stateChoices.length}</li>
                    <li>Bound values: {Object.keys(stateBoundValues).length}</li>
                  </ul>
                  {stateAccounts.length > 0 ? (
                    <div className="state-detail">
                      <p className="panel-result">Accounts detail</p>
                      <ul className="panel-list">
                        {stateAccounts.map((account, index) => (
                          <li key={`account-${index}`}>
                            <code>{renderStateValue(account)}</code>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                  {stateChoices.length > 0 ? (
                    <div className="state-detail">
                      <p className="panel-result">Choices detail</p>
                      <ul className="panel-list">
                        {stateChoices.map((choice, index) => (
                          <li key={`choice-${index}`}>
                            <code>{renderStateValue(choice)}</code>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                  {Object.keys(stateBoundValues).length > 0 ? (
                    <div className="state-detail">
                      <p className="panel-result">Bound values detail</p>
                      <ul className="panel-list">
                        {Object.entries(stateBoundValues).map(([key, value]) => (
                          <li key={key}>
                            <code>{key}</code>: <code>{renderStateValue(value)}</code>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                </div>
                {stateChanges.length > 0 ? (
                  <div className="panel-block">
                    <p className="panel-result">State changes</p>
                    <ul className="panel-list">
                      {stateChanges.map((change, index) => (
                        <li key={`state-change-${index}`}>{change}</li>
                      ))}
                    </ul>
                  </div>
                ) : null}
              </div>
            ) : null}
          </section>
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
