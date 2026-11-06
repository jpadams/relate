import { useEffect, useState } from 'react';
import { Box, Divider, FilledButton, Flex, GraphLabel, Typography } from '@neo4j-ndl/react';
import { PlayIconSolid } from '@neo4j-ndl/react/icons';
import { calculateDefaultNodeColors } from '@neo4j-devtools/word-color';

import { useIsMobile } from '../hooks/useIsMobile';
import { useResizable } from '../hooks/useResizable';
import { loadSchema, type DbSchema } from '../lib/schema';
import { templates } from '../lib/query-templates';
import { SAMPLES } from '../lib/samples';
import { ResizeHandle } from './ResizeHandle';

interface Props {
  engineReady: boolean;
  refreshKey: number;
  onRunQuery: (query: string) => void;
}

export function SchemaSidebar({ engineReady, refreshKey, onRunQuery }: Props) {
  const isMobile = useIsMobile();
  const [schema, setSchema] = useState<DbSchema | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { width, handleProps } = useResizable({
    initial: 400,
    min: 220,
    max: 640,
    edge: 'right',
  });

  useEffect(() => {
    if (!engineReady) return;
    setLoading(true);
    setError(null);
    loadSchema()
      .then(setSchema)
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoading(false));
  }, [engineReady, refreshKey]);

  const body = (
    <Flex
      flexDirection="column"
      gap="32"
      className="n-w-full"
      style={{ padding: isMobile ? '16px 12px' : '20px 16px' }}
    >
      {loading && <Typography variant="body-medium">Loading…</Typography>}
      {error && (
        <Typography variant="body-medium" className="n-text-danger-text">
          {error}
        </Typography>
      )}
      {schema && <SchemaSections schema={schema} onRunQuery={onRunQuery} />}
      <Divider />
      <SampleList onRunQuery={onRunQuery} />
    </Flex>
  );

  if (isMobile) {
    return (
      <Box
        as="section"
        className="n-w-full n-h-full n-bg-neutral-bg-weak"
        style={{ overflowY: 'auto', overflowX: 'hidden' }}
      >
        {body}
      </Box>
    );
  }

  return (
    <div
      style={{
        position: 'relative',
        flexShrink: 0,
        width,
        height: '100%',
        padding: '10px 4px 10px 10px',
        boxSizing: 'border-box',
      }}
    >
      <Box
        as="section"
        borderRadius="lg"
        className="n-border n-border-neutral-border-weak n-bg-neutral-bg-weak"
        style={{ width: '100%', height: '100%', overflowY: 'auto', overflowX: 'hidden' }}
      >
        {body}
      </Box>
      <ResizeHandle edge="right" handleProps={handleProps} offset={4} />
    </div>
  );
}

function SchemaSections({
  schema,
  onRunQuery,
}: {
  schema: DbSchema;
  onRunQuery: (q: string) => void;
}) {
  return (
    <>
      <Section title={`Nodes (${schema.nodeCount})`}>
        <GraphLabel type="node" onClick={() => onRunQuery(templates.allNodes())}>
          *
        </GraphLabel>
        {schema.labels.map((l) => (
          <GraphLabel
            key={l}
            type="node"
            color={calculateDefaultNodeColors(l).backgroundColor}
            onClick={() => onRunQuery(templates.nodesByLabel(l))}
          >
            {l}
          </GraphLabel>
        ))}
      </Section>
      <Section title={`Relationships (${schema.relCount})`}>
        <GraphLabel type="relationship" onClick={() => onRunQuery(templates.allRels())}>
          *
        </GraphLabel>
        {schema.relTypes.map((t) => (
          <GraphLabel
            key={t}
            type="relationship"
            onClick={() => onRunQuery(templates.relsByType(t))}
          >
            {t}
          </GraphLabel>
        ))}
      </Section>
      <Section title="Property keys">
        {schema.propertyKeys.map((k) => (
          <GraphLabel
            key={k}
            type="propertyKey"
            onClick={() => onRunQuery(templates.propertyValues(k))}
          >
            {k}
          </GraphLabel>
        ))}
      </Section>
    </>
  );
}

function SampleList({ onRunQuery }: { onRunQuery: (q: string) => void }) {
  return (
    <Section title="Sample queries" direction="column">
      {SAMPLES.map((s) => (
        <FilledButton
          key={s.label}
          size="small"
          isFluid
          leadingVisual={<PlayIconSolid />}
          onClick={() => onRunQuery(templates.labeled(s.label, s.query))}
          style={{ justifyContent: 'flex-start' }}
        >
          {s.label}
        </FilledButton>
      ))}
    </Section>
  );
}

function Section({
  title,
  children,
  direction = 'row',
}: {
  title: string;
  children: React.ReactNode;
  direction?: 'row' | 'column';
}) {
  return (
    <Flex flexDirection="column" gap="12">
      <Typography variant="body-medium" className="n-text-neutral-text-weak">
        {title}
      </Typography>
      <Flex flexDirection={direction} flexWrap="wrap" gap="8">
        {children}
      </Flex>
    </Flex>
  );
}
