import { useEffect } from 'react';
import type NVL from '@neo4j-nvl/base';

type Point = { x: number; y: number };

// NVL 0.7 pan/zoom listen on MouseEvent/WheelEvent only, so mobile browsers
// get no interaction. This attaches a touch layer that drives setZoomAndPan
// directly: one-finger drag = pan, two-finger = pinch zoom + pan.
//
// Coordinate contract (mirrors @neo4j-nvl/interaction-handlers):
//   worldX = panX + (screenPxFromCanvasCenter * devicePixelRatio) / zoom
// so pan deltas are `-(dxCss * dpr) / zoom`, pinch midpoint stays anchored
// by solving the same mapping each frame against the *current* midpoint.
export function useNvlTouch(
  containerRef: React.RefObject<HTMLElement | null>,
  nvlRef: React.RefObject<NVL | null>,
  enabled: boolean,
) {
  useEffect(() => {
    if (!enabled) return;
    const container = containerRef.current;
    if (!container) return;

    let startPan: Point = { x: 0, y: 0 };
    let startZoom = 1;
    let startTouches: Point[] = [];
    let startDist = 0;
    let active = false;

    // Resolve the live canvas each frame — NVL mounts it async.
    const getCanvas = (): HTMLCanvasElement | null =>
      container.querySelector<HTMLCanvasElement>('.ndl-graph-visualization canvas');

    // Convert client coords into canvas-pixel offsets from the canvas center,
    // matching getCanvasCenterOffset() in @neo4j-nvl/interaction-handlers.
    const centerOffset = (
      canvas: HTMLCanvasElement,
      clientX: number,
      clientY: number,
    ): Point => {
      const rect = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      return {
        x: (clientX - rect.left - rect.width * 0.5) * dpr,
        y: (clientY - rect.top - rect.height * 0.5) * dpr,
      };
    };

    const touchOffsets = (
      canvas: HTMLCanvasElement,
      touches: TouchList,
    ): Point[] => {
      const out: Point[] = [];
      for (let i = 0; i < touches.length; i++) {
        out.push(centerOffset(canvas, touches[i].clientX, touches[i].clientY));
      }
      return out;
    };

    const dist = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);

    const midpoint = (a: Point, b: Point): Point => ({
      x: (a.x + b.x) / 2,
      y: (a.y + b.y) / 2,
    });

    // Did this touch originate on a floating control (zoom buttons, download,
    // sidepanel toggle, …)? Those live inside `.ndl-graph-visualization` but
    // need the browser's synthetic click, so we must not preventDefault.
    const isOnControl = (target: EventTarget | null): boolean => {
      if (!(target instanceof Element)) return false;
      return (
        target.closest(
          '.ndl-graph-visualization-interaction-island,' +
            '.ndl-graph-visualization-sidepanel-wrapper,' +
            '.ndl-graph-visualization-sidepanel-content,' +
            '.ndl-graph-visualization-overview-panel,' +
            'button,[role="button"],a,input,textarea,select',
        ) !== null
      );
    };

    const captureBaseline = (
      nvl: NVL,
      canvas: HTMLCanvasElement,
      touches: TouchList,
    ) => {
      startTouches = touchOffsets(canvas, touches);
      startPan = nvl.getPan();
      startZoom = nvl.getScale();
      if (startTouches.length >= 2) {
        startDist = dist(startTouches[0], startTouches[1]);
      }
    };

    const onStart = (e: TouchEvent) => {
      const nvl = nvlRef.current;
      if (!nvl) return;
      if (isOnControl(e.target)) return;
      const canvas = getCanvas();
      if (!canvas) return;
      active = true;
      captureBaseline(nvl, canvas, e.touches);
      e.preventDefault();
    };

    const onMove = (e: TouchEvent) => {
      const nvl = nvlRef.current;
      if (!active || !nvl) return;
      const canvas = getCanvas();
      if (!canvas) return;
      const touches = touchOffsets(canvas, e.touches);
      if (touches.length >= 2 && startTouches.length >= 2 && startDist > 0) {
        const d = dist(touches[0], touches[1]);
        const scale = startZoom * (d / startDist);
        const mid = midpoint(touches[0], touches[1]);
        // Anchor the current pinch midpoint to the world point that was under
        // the *initial* midpoint. Mapping: world = pan + offset / zoom, so
        //   worldAtStart = startPan + startMid / startZoom
        //   newPan       = worldAtStart - mid / scale
        const startMid = midpoint(startTouches[0], startTouches[1]);
        const worldX = startPan.x + startMid.x / startZoom;
        const worldY = startPan.y + startMid.y / startZoom;
        const panX = worldX - mid.x / scale;
        const panY = worldY - mid.y / scale;
        nvl.setZoomAndPan(scale, panX, panY);
      } else if (touches.length === 1 && startTouches.length >= 1) {
        // Match PanInteraction.handleMouseMove: pan -= dPx / zoom (deltas are
        // already in canvas pixels including devicePixelRatio).
        const dx = touches[0].x - startTouches[0].x;
        const dy = touches[0].y - startTouches[0].y;
        nvl.setPan(startPan.x - dx / startZoom, startPan.y - dy / startZoom);
      }
      e.preventDefault();
    };

    const onEnd = (e: TouchEvent) => {
      if (!active) return;
      const nvl = nvlRef.current;
      const canvas = getCanvas();
      if (nvl && canvas && e.touches.length > 0) {
        // Re-baseline when a finger lifts but others remain (avoids jumps).
        captureBaseline(nvl, canvas, e.touches);
        return;
      }
      active = false;
    };

    const opts: AddEventListenerOptions = { passive: false };
    container.addEventListener('touchstart', onStart, opts);
    container.addEventListener('touchmove', onMove, opts);
    container.addEventListener('touchend', onEnd, opts);
    container.addEventListener('touchcancel', onEnd, opts);

    // Suppress browser pinch/pan only on the canvas itself. Leaving the
    // islands with their default touch-action lets button taps fire normally.
    let canvasEl: HTMLCanvasElement | null = null;
    let prevTouchAction = '';
    const applyTouchAction = () => {
      const el = getCanvas();
      if (!el || el === canvasEl) return;
      canvasEl = el;
      prevTouchAction = el.style.touchAction;
      el.style.touchAction = 'none';
    };
    applyTouchAction();
    const mo = new MutationObserver(applyTouchAction);
    mo.observe(container, { childList: true, subtree: true });

    return () => {
      mo.disconnect();
      container.removeEventListener('touchstart', onStart);
      container.removeEventListener('touchmove', onMove);
      container.removeEventListener('touchend', onEnd);
      container.removeEventListener('touchcancel', onEnd);
      if (canvasEl) canvasEl.style.touchAction = prevTouchAction;
    };
  }, [containerRef, nvlRef, enabled]);
}
