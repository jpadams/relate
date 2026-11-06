import { Flex, Logo, Typography } from '@neo4j-ndl/react';

import { useIsMobile } from '../hooks/useIsMobile';
import { EngineInfoPopover } from './EngineInfoPopover';

interface Props {
  engineReady: boolean;
  resetting: boolean;
  onReset: () => void;
}

export function Header({ engineReady, resetting, onReset }: Props) {
  const isMobile = useIsMobile();
  return (
    <Flex
      flexDirection="row"
      alignItems="center"
      justifyContent="space-between"
      gap="16"
      className="n-shrink-0 n-border-b n-border-neutral-border-weak n-bg-neutral-bg-weak"
      style={{ height: 56, paddingLeft: isMobile ? 12 : 24, paddingRight: isMobile ? 8 : 16 }}
    >
      <Flex flexDirection="row" gap="12" alignItems="center" className="n-shrink-0 n-min-w-0">
        <Logo className="n-shrink-0" style={{ width: 80, marginLeft: isMobile ? 0 : 10 }} />
        <Typography
          variant={isMobile ? 'subheading-medium' : 'title-4'}
          style={{ whiteSpace: 'nowrap' }}
        >
          Relate Playground
        </Typography>
      </Flex>
      <EngineInfoPopover
        engineReady={engineReady}
        resetting={resetting}
        onReset={onReset}
      />
    </Flex>
  );
}
