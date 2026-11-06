import type { Resizable } from '../hooks/useResizable';

// `offset` is the distance in px between the wrapper's inner edge and the
// intended visible edge — used to align the handle over the sidebar's
// rounded card when the wrapper carries padding.
export function ResizeHandle({
  edge,
  handleProps,
  offset = 0,
}: {
  edge: 'left' | 'right';
  handleProps: Resizable['handleProps'];
  offset?: number;
}) {
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      {...handleProps}
      style={{
        position: 'absolute',
        top: 0,
        [edge]: offset - 3,
        width: 6,
        height: '100%',
        cursor: 'col-resize',
        zIndex: 1,
        touchAction: 'none',
      }}
    />
  );
}
