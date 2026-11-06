import { lazy, Suspense } from 'react';
import { Banner, Box, Flex, Tabs, Typography } from '@neo4j-ndl/react';

import type { ScriptRun } from '../lib/engine';
import type { QueryStats } from '../lib/wire';
import { useIsMobile } from '../hooks/useIsMobile';
import { NeedleResultsTable } from './NeedleResultsTable';

// NVL + Prism are multi-MB. Load only when their tab is opened.
const NeedleGraphView = lazy(() =>
  import('./NeedleGraphView').then((m) => ({ default: m.NeedleGraphView })),
);
const JsonView = lazy(() => import('./JsonView').then((m) => ({ default: m.JsonView })));

const TabFallback = () => (
  <div className="n-flex n-h-full n-items-center n-justify-center n-text-neutral-text-weak">
    Loading…
  </div>
);

export type ViewTab = 'graph' | 'table' | 'json';

interface Props {
  tab: ViewTab;
  onTabChange: (tab: ViewTab) => void;
  /** The last script run: its last statement's result, and its counters. */
  run: ScriptRun | null;
  error: string | null;
  elapsed: number | null;
}

export function ResultsPanel({
  tab,
  onTabChange,
  run,
  error,
  elapsed,
}: Props) {
  const isMobile = useIsMobile();
  return (
    <Box
      as="section"
      borderRadius="lg"
      className="n-flex n-flex-col n-flex-1 n-gap-3 n-p-4 n-border n-border-neutral-border-weak n-bg-neutral-bg-weak"
      style={{ minHeight: isMobile ? '60dvh' : 0 }}
    >
      {error && (
        <Banner variant="danger" hasIcon isCloseable={false}>
          <Banner.Header>Query error</Banner.Header>
          <Banner.Description>{error}</Banner.Description>
        </Banner>
      )}
      <Box style={{ paddingTop: 16 }}>
        <Tabs value={tab} onChange={(v) => onTabChange(v as ViewTab)} fill="underline" size="large">
          <Tabs.Tab id="graph">Graph</Tabs.Tab>
          <Tabs.Tab id="table">Table</Tabs.Tab>
          <Tabs.Tab id="json">JSON</Tabs.Tab>
        </Tabs>
      </Box>
      <Box className="n-flex-1 relate-scroll" style={{ overflow: 'auto', minHeight: 0 }}>
        <Suspense fallback={<TabFallback />}>
          {tab === 'graph' && <NeedleGraphView result={run?.last ?? null} />}
          {tab === 'table' && run && <NeedleResultsTable result={run.last} />}
          {tab === 'json' && run && <JsonView result={run.last} />}
        </Suspense>
      </Box>
      {run && <StatsRow stats={run.stats} rowCount={run.last.rows.length} elapsedMs={elapsed} />}
    </Box>
  );
}

function StatsRow({
  stats,
  rowCount,
  elapsedMs,
}: {
  stats: QueryStats;
  rowCount: number;
  elapsedMs: number | null;
}) {
  const active = Object.entries(stats).filter(([, v]) => typeof v === 'number' && v > 0);
  return (
    <Flex
      flexDirection="row"
      gap="16"
      alignItems="center"
      flexWrap="wrap"
      className="n-text-neutral-text-weak"
    >
      <Typography variant="body-small">
        {rowCount} record{rowCount === 1 ? '' : 's'}
      </Typography>
      {elapsedMs !== null && (
        <Typography variant="body-small">{elapsedMs.toFixed(1)} ms</Typography>
      )}
      {active.map(([k, v]) => (
        <Typography key={k} variant="body-small">
          {v} {k.replace(/_/g, ' ')}
        </Typography>
      ))}
    </Flex>
  );
}
