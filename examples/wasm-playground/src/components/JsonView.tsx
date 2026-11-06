import { useMemo } from 'react';
import { CodeBlock } from '@neo4j-ndl/react/code-block';

import type { WireResult } from '../lib/wire';

const JSON_CHAR_LIMIT = 200_000;

export function JsonView({ result }: { result: WireResult }) {
  const code = useMemo(() => {
    const full = JSON.stringify(result, null, 2);
    if (full.length <= JSON_CHAR_LIMIT) return full;
    return (
      full.slice(0, JSON_CHAR_LIMIT) +
      `\n\n… truncated at ${JSON_CHAR_LIMIT.toLocaleString()} characters ` +
      `(${full.length.toLocaleString()} total). Narrow the query to see the full JSON.`
    );
  }, [result]);
  return (
    <CodeBlock
      code={code}
      language="json"
      shouldShowLineNumbers
    />
  );
}
