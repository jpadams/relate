import { useCallback, useEffect, useLayoutEffect, useRef } from 'react';

import { PROMPT_HINT } from '../lib/ui-strings';
import { canRunQuery } from '../lib/query-eligibility';
import { useIsMobile } from '../hooks/useIsMobile';
import { useResizable } from '../hooks/useResizable';
import { ResizeHandle } from './ResizeHandle';

export type CliEntry =
  | { kind: 'input'; text: string }
  | { kind: 'output'; text: string }
  | { kind: 'error'; text: string };

interface Props {
  history: CliEntry[];
  engineReady: boolean;
  running: boolean;
  draft: string;
  onDraftChange: (v: string) => void;
  onSubmit: (text: string) => void;
  // Shown in mobile-tab context as a "switch to query" affordance.
  onSwitchToQuery: () => void;
}

const TERMINAL_BG = '#0a0a0a';
const TERMINAL_FG = '#d0d6d0';
const TERMINAL_DIM = '#6a6f6a';
const TERMINAL_ERR = '#ff6b6b';
const TERMINAL_ACCENT = '#7ee787';

const BOOT_LINES = [
  { text: '$ relate', color: TERMINAL_DIM },
  { text: '', color: TERMINAL_FG },
];

const FONT_STACK =
  'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace';

export function CliSidebar({
  history,
  engineReady,
  running,
  draft,
  onDraftChange,
  onSubmit,
  onSwitchToQuery,
}: Props) {
  const isMobile = useIsMobile();
  const { width, handleProps } = useResizable({
    initial: 380,
    min: 260,
    max: 1440,
    edge: 'left',
  });
  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const pinnedToBottom = useRef(true);

  useEffect(() => {
    const el = scrollerRef.current;
    if (el && pinnedToBottom.current) el.scrollTop = el.scrollHeight;
  }, [history, engineReady]);

  // Auto-grow textarea with the draft's line count.
  useLayoutEffect(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    ta.style.height = 'auto';
    ta.style.height = `${ta.scrollHeight}px`;
    if (document.activeElement === ta) ta.scrollIntoView({ block: 'nearest' });
  }, [draft]);

  const canSubmit = canRunQuery(engineReady, running, draft);
  const submit = () => {
    if (canSubmit) onSubmit(draft);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Match the CypherEditor: ⌘/Ctrl+Enter submits, plain Enter inserts a newline.
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    }
  };

  const statusReady = '● Running Relate... ';
  const statusLinkText = 'start writing queries';
  const statusLine = engineReady
    ? { text: statusReady + statusLinkText, color: TERMINAL_ACCENT }
    : { text: '● loading wasm module…', color: TERMINAL_DIM };

  const focusPrompt = () => textareaRef.current?.focus();
  const recall = useCallback(
    (text: string) => {
      onDraftChange(text);
      textareaRef.current?.focus();
    },
    [onDraftChange],
  );

  return (
    <div
      style={{
        position: 'relative',
        flexShrink: 0,
        width: isMobile ? '100%' : width,
        height: '100%',
        borderLeft: isMobile ? 'none' : '1px solid #000',
        background: TERMINAL_BG,
        color: TERMINAL_FG,
      }}
    >
      {!isMobile && <ResizeHandle edge="left" handleProps={handleProps} />}
      <div
        ref={scrollerRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          pinnedToBottom.current = el.scrollHeight - el.clientHeight - el.scrollTop <= 24;
        }}
        onClick={(e) => {
          // Clicking on empty terminal space focuses the prompt, but preserve
          // text selection and clicks on interactive children (history recall).
          if (e.target === e.currentTarget) focusPrompt();
        }}
        style={{
          position: 'absolute',
          inset: 0,
          overflowY: 'auto',
          overflowX: 'auto',
          padding: '16px 16px 16px',
          fontFamily: FONT_STACK,
          fontSize: 12.5,
          lineHeight: 1.5,
          whiteSpace: 'pre',
        }}
      >
        {BOOT_LINES.map((line, i) => (
          <div key={`boot-${i}`} style={{ color: line.color }}>
            {line.text || '\u00a0'}
          </div>
        ))}
        <div style={{ color: statusLine.color, fontWeight: 600 }}>
          {engineReady && isMobile ? (
            <>
              {statusReady}
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  onSwitchToQuery();
                }}
                style={{ color: 'inherit', textDecoration: 'underline' }}
              >
                {statusLinkText}
              </a>
            </>
          ) : (
            statusLine.text
          )}
        </div>
        <div style={{ color: TERMINAL_DIM }}>{'\u00a0'}</div>
        {history.map((entry, i) => (
          <EntryLine
            key={i}
            entry={entry}
            onRecall={recall}
          />
        ))}
        <PromptRow
          textareaRef={textareaRef}
          value={draft}
          onChange={onDraftChange}
          onKeyDown={onKeyDown}
          onSubmit={submit}
          disabled={!engineReady}
          running={running}
          canSubmit={canSubmit}
        />
      </div>
    </div>
  );
}

function PromptRow({
  textareaRef,
  value,
  onChange,
  onKeyDown,
  onSubmit,
  disabled,
  running,
  canSubmit,
}: {
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
  value: string;
  onChange: (v: string) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onSubmit: () => void;
  disabled: boolean;
  running: boolean;
  canSubmit: boolean;
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'flex-start', color: TERMINAL_FG }}>
      <span
        style={{
          color: running ? TERMINAL_DIM : TERMINAL_ACCENT,
          flexShrink: 0,
          userSelect: 'none',
        }}
      >
        {running ? '… ' : '> '}
      </span>
      <textarea
        ref={textareaRef}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        disabled={disabled}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        rows={1}
        placeholder={disabled ? '' : PROMPT_HINT}
        style={{
          flex: 1,
          minWidth: 0,
          background: 'transparent',
          color: 'inherit',
          border: 'none',
          outline: 'none',
          resize: 'none',
          padding: 0,
          margin: 0,
          fontFamily: FONT_STACK,
          fontSize: 'inherit',
          lineHeight: 'inherit',
          whiteSpace: 'pre',
          overflow: 'hidden',
        }}
      />
      <button
        type="button"
        onClick={onSubmit}
        disabled={!canSubmit}
        aria-label="Run"
        title="Run (⌘⏎)"
        style={{
          flexShrink: 0,
          marginLeft: 8,
          padding: '2px 8px',
          background: canSubmit ? TERMINAL_ACCENT : 'transparent',
          color: canSubmit ? '#0a0a0a' : TERMINAL_DIM,
          border: `1px solid ${canSubmit ? TERMINAL_ACCENT : TERMINAL_DIM}`,
          borderRadius: 4,
          font: 'inherit',
          fontWeight: 600,
          cursor: canSubmit ? 'pointer' : 'default',
        }}
      >
        ⏎
      </button>
    </div>
  );
}

function EntryLine({
  entry,
  onRecall,
}: {
  entry: CliEntry;
  onRecall: (text: string) => void;
}) {
  if (entry.kind === 'input') {
    // REPL echo: prompt on line 1, indent on continuations, trailing `;`.
    const lines = entry.text.split('\n');
    return (
      <div
        onClick={() => onRecall(entry.text)}
        title="Click to reuse"
        style={{ color: TERMINAL_FG, cursor: 'pointer' }}
      >
        {lines.map((line, i) => (
          <div key={i}>
            <span style={{ color: TERMINAL_ACCENT }}>{i === 0 ? '> ' : '  '}</span>
            {line}
            {i === lines.length - 1 && !/;\s*$/.test(line) ? ';' : ''}
          </div>
        ))}
      </div>
    );
  }
  return (
    <div style={{ color: entry.kind === 'error' ? TERMINAL_ERR : TERMINAL_FG }}>
      {entry.text || '\u00a0'}
    </div>
  );
}
