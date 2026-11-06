import { Banner, FilledButton, Typography } from '@neo4j-ndl/react';

export function FullScreenBanner({
  title,
  message,
  hint,
  onReload,
}: {
  title: string;
  message: string;
  hint: string;
  onReload?: () => void;
}) {
  return (
    <div className="n-h-full n-w-full n-flex n-items-center n-justify-center n-bg-neutral-bg-default n-p-8">
      <Banner variant="danger" hasIcon isCloseable={false} className="n-max-w-2xl">
        <Banner.Header>{title}</Banner.Header>
        <Banner.Description>
          <div className="n-flex n-flex-col n-gap-2">
            <code className="n-block n-text-danger-text">{message}</code>
            <Typography variant="body-medium">{hint}</Typography>
          </div>
        </Banner.Description>
        {onReload && (
          <Banner.Actions>
            <FilledButton onClick={onReload}>Reload</FilledButton>
          </Banner.Actions>
        )}
      </Banner>
    </div>
  );
}
