import { useRef } from 'react';
import { Box, FilledButton, Flex } from '@neo4j-ndl/react';
import { CypherEditor } from '@neo4j-cypher/react-codemirror';

import { PROMPT_HINT } from '../lib/ui-strings';
import { canRunQuery } from '../lib/query-eligibility';
import { useIsMobile } from '../hooks/useIsMobile';
import './cypher-editor.css';

interface Props {
  query: string;
  onQueryChange: (q: string) => void;
  onRun: (text: string) => void;
  engineReady: boolean;
  running: boolean;
  lastRun: string | null;
  // Past executed inputs, newest first; drives the editor's up/down REPL recall.
  history: string[];
}

export function QueryPanel({
  query,
  onQueryChange,
  onRun,
  engineReady,
  running,
  lastRun,
  history,
}: Props) {
  const isMobile = useIsMobile();
  // CypherEditor debounces onChange by 200ms, so `query` state can lag behind
  // what the user just typed. Read the live doc from CodeMirror when running.
  const editorRef = useRef<CypherEditor | null>(null);
  const canInteract = !running && engineReady;

  const currentText = (fallback: string): string => {
    const view = editorRef.current?.editorView?.current;
    return view ? view.state.doc.toString() : fallback;
  };

  const runNow = (text: string) => {
    if (canRunQuery(engineReady, running, text)) onRun(text);
  };

  return (
    <Box
      as="section"
      borderRadius="lg"
      className="n-shrink-0 n-flex n-flex-col n-gap-3 n-p-4 n-border n-border-neutral-border-weak n-bg-neutral-bg-weak"
    >
      <Box
        className="n-w-full relate-cm relate-cm-min3"
        style={isMobile ? { maxHeight: '35dvh', overflow: 'auto' } : undefined}
      >
        <CypherEditor
          ref={editorRef}
          value={query}
          onChange={onQueryChange}
          onExecute={runNow}
          history={history}
          newLineOnEnter
          lineWrap
          theme="dark"
          placeholder={lastRun ?? PROMPT_HINT}
        />
      </Box>
      <Flex flexDirection="row" justifyContent={isMobile ? 'stretch' : 'flex-end'}>
        <FilledButton
          onClick={() => runNow(currentText(query))}
          isDisabled={!canInteract}
          isLoading={running}
          size={isMobile ? 'small' : 'medium'}
          style={isMobile ? { width: '100%' } : undefined}
        >
          {isMobile ? 'Run' : 'Run  (⌘⏎)'}
        </FilledButton>
      </Flex>
    </Box>
  );
}
