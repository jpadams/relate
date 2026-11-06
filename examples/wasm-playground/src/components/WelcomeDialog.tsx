import { Box, Dialog, FilledButton, Flex, TextLink, Typography } from '@neo4j-ndl/react';
import { CypherEditor } from '@neo4j-cypher/react-codemirror';

import { useIsMobile } from '../hooks/useIsMobile';
import './cypher-editor.css';

interface Props {
  isOpen: boolean;
  seedScript: string;
  onRunSeed: () => void;
  isRunning: boolean;
  canRun: boolean;
}

export function WelcomeDialog({ isOpen, seedScript, onRunSeed, isRunning, canRun }: Props) {
  const isMobile = useIsMobile();
  return (
    <Dialog
      isOpen={isOpen}
      hasDisabledCloseButton
      size={isMobile ? 'small' : 'medium'}
      onClose={() => {}}
    >
      <Dialog.Header>Welcome to the Relate Playground</Dialog.Header>
      <Dialog.Content>
        <Flex flexDirection="column" gap="16">
          <Typography variant="body-medium">
            Relate is an embedded graph database that can run anywhere. Great for agents & embedded use-cases.
            This playground bundles a WebAssembly build of Relate, running directly in your browser.
          </Typography>
          <Typography variant="body-medium">
            Want to use Relate from the CLI, Python, or in your own app? Check out the{' '}
            <TextLink href="https://relatedb.ai/">documentation</TextLink>.
          </Typography>
          <Typography variant="body-medium">
            Run the queries below to load the sample dataset and get started.
          </Typography>
          <Box
            borderRadius="md"
            className="n-border n-border-neutral-border-weak n-bg-neutral-bg-weak relate-cm"
            style={{ maxHeight: isMobile ? 224 : 300, maxWidth: '100%', overflow: 'auto' }}
          >
            <CypherEditor value={seedScript} theme="dark" readonly lineWrap />
          </Box>
        </Flex>
      </Dialog.Content>
      <Dialog.Actions>
        <FilledButton
          onClick={onRunSeed}
          isDisabled={!canRun || isRunning}
          isLoading={isRunning}
          style={isMobile ? { width: '100%', minHeight: 44 } : undefined}
        >
          Load sample data
        </FilledButton>
      </Dialog.Actions>
    </Dialog>
  );
}
