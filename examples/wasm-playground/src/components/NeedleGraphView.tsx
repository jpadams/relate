import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type NVL from '@neo4j-nvl/base';
import { GraphVisualization } from '@neo4j-ndl/react-graph';

import type { WireResult } from '../lib/wire';
import { extractNeedleGraph } from '../lib/extract-graph';
import { useIsMobile } from '../hooks/useIsMobile';
import { useNvlTouch } from '../hooks/useNvlTouch';

interface Props {
  result: WireResult | null;
}

const SIDEPANEL_MIN_WIDTH = 230;

export function NeedleGraphView({ result }: Props) {
  const isMobile = useIsMobile();
  const { nodes, rels, limitHit } = useMemo(() => {
    if (!result) return { nodes: [], rels: [], limitHit: false };
    return extractNeedleGraph(result, { nodeLimit: 1000 });
  }, [result]);
  const [sidePanelWidth, setSidePanelWidth] = useState<number>(SIDEPANEL_MIN_WIDTH);
  const [isSidePanelOpen, setIsSidePanelOpen] = useState<boolean>(!isMobile);
  const nvlRef = useRef<NVL | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const fitToGraph = useCallback(() => {
    if (nodes.length === 0) return;
    nvlRef.current?.fit(
      nodes.map((n) => n.id),
      { animated: false },
    );
  }, [nodes]);

  // Refit whenever the container resizes (covers tab switches where the
  // container starts hidden with 0×0 and later becomes visible).
  useEffect(() => {
    const el = containerRef.current;
    if (!el || nodes.length === 0) return;
    let last = 0;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0].contentRect.width;
      if (w > 0 && Math.abs(w - last) > 4) {
        last = w;
        window.setTimeout(fitToGraph, 50);
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [nodes, fitToGraph]);

  useNvlTouch(containerRef, nvlRef, isMobile && nodes.length > 0);

  return (
    <div ref={containerRef} className="n-relative n-w-full n-h-full">
      {limitHit && (
        <div className="n-absolute n-top-2 n-left-2 n-z-10 n-px-2 n-py-1 n-rounded n-bg-neutral-bg-strong n-text-neutral-text-weak">
          Showing first 1000 nodes.
        </div>
      )}
      {!result && (
        <div className="n-flex n-h-full n-items-center n-justify-center n-text-neutral-text-weak">
          Run a query to see the graph.
        </div>
      )}
      {result && nodes.length === 0 && (
        <div className="n-flex n-h-full n-items-center n-justify-center n-text-neutral-text-weak">
          Query returned no graph entities.
        </div>
      )}
      {result && nodes.length > 0 && (
        <GraphVisualization
          nvlRef={nvlRef}
          nodes={nodes}
          rels={rels}
          className="n-w-full n-h-full n-border n-border-neutral-border-weak n-rounded-lg"
          sidepanel={{
            children: <GraphVisualization.SingleSelectionSidePanelContents />,
            isSidePanelOpen,
            setIsSidePanelOpen,
            sidePanelWidth,
            onSidePanelResize: setSidePanelWidth,
            minWidth: SIDEPANEL_MIN_WIDTH,
          }}
        />
      )}
    </div>
  );
}
