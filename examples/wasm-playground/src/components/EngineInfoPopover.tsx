import { useEffect, useRef, useState } from 'react';
import { Flex, OutlinedButton, Popover, TextLink, Typography } from '@neo4j-ndl/react';
import { CircleStackIconOutline } from '@neo4j-ndl/react/icons';

import { engine } from '../lib/engine';

interface Props {
  engineReady: boolean;
  resetting: boolean;
  onReset: () => void;
}

export function EngineInfoPopover({ engineReady, resetting, onReset }: Props) {
  const [isOpen, setIsOpen] = useState(false);
  const [memBytes, setMemBytes] = useState(0);
  const anchorRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (!isOpen) return;
    const tick = () => setMemBytes(engine.memoryBytes());
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
  }, [isOpen]);

  return (
    <div onMouseEnter={() => setIsOpen(true)}>
      <button
        ref={anchorRef}
        type="button"
        aria-label="Engine info"
        onClick={() => setIsOpen((v) => !v)}
        disabled={!engineReady}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 32,
          height: 32,
          padding: 0,
          border: 'none',
          borderRadius: 6,
          background: isOpen ? 'var(--theme-color-neutral-bg-strong)' : 'transparent',
          color: 'var(--theme-color-neutral-text-default)',
          cursor: engineReady ? 'pointer' : 'default',
          opacity: engineReady ? 1 : 0.5,
        }}
      >
        <CircleStackIconOutline style={{ width: 20, height: 20 }} />
      </button>
      <Popover
        isOpen={isOpen}
        onOpenChange={setIsOpen}
        placement="bottom-end-top-end"
        anchorElement={anchorRef.current}
      >
        <Popover.Content>
          <Flex flexDirection="column" gap="12" style={{ minWidth: 280, padding: 16 }}>
            <Typography variant="subheading-medium">Relate WASM</Typography>
            <Typography variant="body-medium" className="n-text-neutral-text-weak">
              Embedded graph database in your browser.
              <br />
              <TextLink href="https://relatedb.ai/">Relate Docs</TextLink>
            </Typography>
            <Flex flexDirection="row" gap="8" alignItems="baseline" justifyContent="space-between">
              <Typography variant="body-small" className="n-text-neutral-text-weak">
                Memory usage
              </Typography>
              <Typography variant="body-medium">{formatBytes(memBytes)}</Typography>
            </Flex>
            <OutlinedButton
              size="small"
              onClick={onReset}
              isDisabled={!engineReady || resetting}
              isLoading={resetting}
            >
              Clear database
            </OutlinedButton>
          </Flex>
        </Popover.Content>
      </Popover>
    </div>
  );
}

function formatBytes(n: number): string {
  if (n <= 0) return '—';
  const kb = n / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}
