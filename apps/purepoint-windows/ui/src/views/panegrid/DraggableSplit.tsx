// Port of apps/purepoint-macos/purepoint-macos/Views/PaneGrid/DraggableSplit.swift (WIN-7.4): a
// split view with a draggable hairline, nested recursively for the pane grid tree. The geometry is
// `splitSizes` / `splitDragRatio` in model/paneSplit.ts (ported with its tests in WIN-6.2).
//
// Both children get exact sizes and clip, so a child that cannot shrink to its share never paints
// over the divider or a neighbouring pane. The drag handle sits above both children, so a terminal
// beside the hairline never takes the drag.
//
// Differences from Swift: sizes are CSS (`calc((100% - 1px) * ratio)`, the same arithmetic as
// `sizes(total:ratio:)` without measuring); the split is measured only when a drag starts. The
// live ratio is committed when the drag ends, as on the Mac (each commit rewrites the registry).
// The Mac's accessibility adjustable action is the arrow keys on the focusable handle.
import { type CSSProperties, type ReactNode, useRef, useState } from 'react';
import {
  SPLIT_DIVIDER_THICKNESS,
  SPLIT_DRAG_INSET,
  splitDragRatio,
  type PaneAxis,
} from '../../model/paneSplit.ts';

/** The keyboard step: 5% of the split, as the Mac's adjustable action. */
const KEYBOARD_STEP = 0.05;

export interface DraggableSplitProps {
  axis: PaneAxis;
  ratio: number;
  onRatioChanged: (ratio: number) => void;
  first: ReactNode;
  second: ReactNode;
}

function capturePointer(element: Element, pointerId: number, capture: boolean): void {
  try {
    if (capture) element.setPointerCapture(pointerId);
    else element.releasePointerCapture(pointerId);
  } catch {
    // jsdom has no pointer capture, or the pointer is already gone.
  }
}

export function DraggableSplit({
  axis,
  ratio,
  onRatioChanged,
  first,
  second,
}: DraggableSplitProps) {
  const container = useRef<HTMLDivElement>(null);
  const drag = useRef<{ start: number; startRatio: number; total: number } | null>(null);
  /** Live ratio while dragging. */
  const [dragRatio, setDragRatio] = useState<number | null>(null);

  const isVertical = axis === 'vertical';
  const shown = dragRatio ?? ratio;
  const collapsed = shown >= 1 || shown <= 0;
  const firstSize =
    shown <= 0
      ? '0px'
      : shown >= 1
        ? '100%'
        : `calc((100% - ${String(SPLIT_DIVIDER_THICKNESS)}px) * ${String(shown)})`;

  const total = (): number => {
    const rect = container.current?.getBoundingClientRect();
    return rect === undefined ? 0 : isVertical ? rect.width : rect.height;
  };

  const handleOffset = `calc(${firstSize} + ${String(SPLIT_DIVIDER_THICKNESS / 2 - SPLIT_DRAG_INSET)}px)`;
  const handleStyle: CSSProperties = isVertical
    ? { left: handleOffset, width: SPLIT_DRAG_INSET * 2 }
    : { top: handleOffset, height: SPLIT_DRAG_INSET * 2 };

  return (
    <div
      ref={container}
      className="pane-split"
      data-axis={axis}
      data-testid="pane-split"
      style={{ flexDirection: isVertical ? 'row' : 'column' }}
    >
      <div className="pane-split-child" style={{ flex: `0 0 ${firstSize}` }}>
        {first}
      </div>
      {!collapsed && <div className="pane-split-hairline" data-dragging={dragRatio !== null} />}
      <div className="pane-split-child" style={{ flex: '1 1 0' }}>
        {second}
      </div>
      {!collapsed && (
        <div
          className="pane-split-handle"
          role="separator"
          aria-orientation={isVertical ? 'vertical' : 'horizontal'}
          aria-label="Resize pane"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(ratio * 100)}
          aria-valuetext={`${String(Math.round(ratio * 100))} percent`}
          tabIndex={0}
          data-testid="pane-divider"
          style={handleStyle}
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            drag.current = {
              start: isVertical ? event.clientX : event.clientY,
              startRatio: ratio,
              total: total(),
            };
            capturePointer(event.currentTarget, event.pointerId, true);
          }}
          onPointerMove={(event) => {
            const start = drag.current;
            if (start === null) return;
            const translation = (isVertical ? event.clientX : event.clientY) - start.start;
            setDragRatio(splitDragRatio(start.startRatio, translation, start.total));
          }}
          onPointerUp={(event) => {
            const start = drag.current;
            if (start === null) return;
            drag.current = null;
            capturePointer(event.currentTarget, event.pointerId, false);
            const translation = (isVertical ? event.clientX : event.clientY) - start.start;
            setDragRatio(null);
            if (translation !== 0) {
              onRatioChanged(splitDragRatio(start.startRatio, translation, start.total));
            }
          }}
          onPointerCancel={() => {
            drag.current = null;
            setDragRatio(null);
          }}
          onKeyDown={(event) => {
            const decrement = isVertical ? 'ArrowLeft' : 'ArrowUp';
            const increment = isVertical ? 'ArrowRight' : 'ArrowDown';
            if (event.key !== decrement && event.key !== increment) return;
            event.preventDefault();
            const length = total();
            const step = event.key === increment ? KEYBOARD_STEP : -KEYBOARD_STEP;
            onRatioChanged(splitDragRatio(ratio, step * length, length));
          }}
        />
      )}
    </div>
  );
}
