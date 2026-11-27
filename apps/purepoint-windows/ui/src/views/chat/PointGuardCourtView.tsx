// Port of apps/purepoint-macos/purepoint-macos/Views/Chat/PointGuardCourtView.swift (WIN-8.13):
// Point Guard's new-conversation screen. A half-court with the logo at the top of the key, the
// composer under it, and four plays that each send a whole prompt when passed to: the pointer
// lights the nearest play, the logo leans and turns toward it, and a click draws the pass and
// sends once it lands (450 ms; at once under reduced motion). A pane too small for the court, or
// a compact one, stacks the plays as pills under the composer instead. Right-click a play to
// replace it with a saved global prompt or reset it.
//
// Differences from Swift: the iPad's docked composer, portrait court and drag-to-pass are not
// ported (Windows has no touch-only pane; `CourtGeometry.docksComposer` is false). Court lines
// are an SVG scaled like `preserveAspectRatio="xMidYMid slice"`.
import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  CourtGeometry,
  courtLayout,
  courtPositions,
  facesLeft as nextFacing,
  isCustomPlay,
  logoPoint,
  logoTilt,
  nearestSlot,
  type Point,
  type PointGuardPlay,
  type PointGuardPrompt,
} from '../../model/pointGuardPlays.ts';
import { Icon } from '../../theme/Icon.tsx';
import logoLight from './assets/PurePointLogo.png';
import logoDark from './assets/PurePointLogo-Dark.png';
import {
  type MenuEntry,
  useContextMenu,
  useElementSize,
  useReducedMotion,
} from './pointGuardParts.tsx';

/** `PurePointTheme.chatContentWidth` and `chatHorizontalPadding`. */
const CHAT_CONTENT_WIDTH = 720;
const CHAT_HORIZONTAL_PADDING = 24;
const PASS_MS = 450;

export interface PointGuardCourtViewProps {
  plays: readonly PointGuardPlay[];
  prompts: readonly PointGuardPrompt[];
  /** False while disconnected or mid-session; plays are shown but can't be run. */
  canRun: boolean;
  connected: boolean;
  compact?: boolean;
  composer: ReactNode;
  onRun: (play: PointGuardPlay) => void;
  onAssign: (slot: number, prompt: PointGuardPrompt) => void;
  onReset: (slot: number) => void;
  onOpenSettings: () => void;
}

function playMenu(
  play: PointGuardPlay,
  prompts: readonly PointGuardPrompt[],
  onAssign: (slot: number, prompt: PointGuardPrompt) => void,
  onReset: (slot: number) => void,
): MenuEntry[] {
  const replace: MenuEntry = {
    label: 'Replace With',
    items:
      prompts.length === 0
        ? [{ label: 'No global prompts yet', disabled: true }]
        : prompts.map((prompt) => ({
            label: prompt.needsInput ? `${prompt.name} (needs input)` : prompt.name,
            disabled: prompt.needsInput,
            onSelect: () => {
              onAssign(play.slot, prompt);
            },
          })),
  };
  return isCustomPlay(play)
    ? [
        replace,
        {
          label: 'Reset to Default',
          separated: true,
          onSelect: () => {
            onReset(play.slot);
          },
        },
      ]
    : [replace];
}

export function PointGuardCourtView(props: PointGuardCourtViewProps) {
  const [ref, size] = useElementSize<HTMLDivElement>();
  const layout = courtLayout(size, props.compact === true, CourtGeometry.docksComposer);
  return (
    <div className="pg-court-host" ref={ref} data-testid="pg-court" data-layout={layout}>
      {size.width > 0 &&
        (layout === 'stacked' ? (
          <StackedCourt {...props} />
        ) : (
          <WideCourt {...props} width={size.width} height={size.height} />
        ))}
    </div>
  );
}

function LogoMark({
  diameter,
  tilt,
  facesLeft,
  reducedMotion,
}: {
  diameter: number;
  tilt: number;
  facesLeft: boolean;
  reducedMotion: boolean;
}) {
  return (
    <div
      className="pg-logo-mark"
      role="img"
      aria-label="Point Guard"
      style={{ width: diameter, height: diameter }}
    >
      <picture
        className="pg-logo-image"
        data-animated={!reducedMotion}
        style={{
          width: diameter * 0.6,
          height: diameter * 0.6,
          transform: `rotate(${String(tilt)}deg) scaleX(${facesLeft ? '-1' : '1'})`,
        }}
      >
        <source srcSet={logoDark} media="(prefers-color-scheme: dark)" />
        <img src={logoLight} alt="" draggable={false} />
      </picture>
    </div>
  );
}

function Heading({ size }: { size: number }) {
  return (
    <h2
      className="pg-court-heading"
      style={{ fontSize: size, letterSpacing: `${String(-size * 0.02)}px` }}
    >
      What play do you want to run?
    </h2>
  );
}

/** The composer stays put while disconnected, so a reconnecting session keeps its draft in view. */
function Slot({
  connected,
  composer,
  onOpenSettings,
}: {
  connected: boolean;
  composer: ReactNode;
  onOpenSettings: () => void;
}) {
  return (
    <div className="pg-court-slot">
      {!connected && (
        <div className="pg-court-setup">
          <span>Finish setup to start.</span>
          <button type="button" className="pg-link-button" onClick={onOpenSettings}>
            Open Settings
          </button>
        </div>
      )}
      {composer}
    </div>
  );
}

function PlayButton({
  play,
  lit,
  dimmed,
  canRun,
  position,
  menu,
  onClick,
}: {
  play: PointGuardPlay;
  lit: boolean;
  dimmed: boolean;
  canRun: boolean;
  position: Point;
  menu: () => MenuEntry[];
  onClick: () => void;
}) {
  const context = useContextMenu(menu);
  const hint = play.prompt === '' ? 'Sends your saved prompt' : play.prompt;
  return (
    <>
      <button
        type="button"
        className="pg-play"
        data-lit={lit}
        data-testid={`pg-play-${String(play.slot)}`}
        disabled={!canRun}
        title={play.prompt}
        aria-label={play.title}
        aria-description={hint}
        style={{
          left: position.x,
          top: position.y,
          width: 2 * CourtGeometry.labelHalfWidth,
          opacity: dimmed ? 0.35 : 1,
        }}
        onClick={onClick}
        onContextMenu={context.onContextMenu}
      >
        <span className="pg-play-icon">
          <Icon name={play.symbol} size={16} />
        </span>
        <span className="pg-play-title">{play.title}</span>
      </button>
      {context.element}
    </>
  );
}

/** The pass: a solid line to the lit play. */
function PassLine({ from, to }: { from: Point; to: Point | null }) {
  if (to === null) return null;
  return (
    <line
      className="pg-pass-line"
      x1={from.x}
      y1={from.y}
      x2={to.x}
      y2={to.y}
      strokeLinecap="round"
    />
  );
}

/** Half-court markings in a 1000×640 space, scaled to fill like `slice`. */
function CourtLines({ width, height }: { width: number; height: number }) {
  return (
    <g className="pg-court-lines">
      <svg
        width={width}
        height={height}
        viewBox="0 0 1000 640"
        preserveAspectRatio="xMidYMid slice"
        overflow="visible"
      >
        <g fill="none" vectorEffect="non-scaling-stroke">
          <rect x="24" y="24" width="952" height="592" rx="6" />
          <path d="M24 192 H976" />
          <circle cx="500" cy="192" r="96" />
          <circle cx="500" cy="192" r="34" />
          <path d="M210 616 V560 A290 290 0 0 1 790 560 V616" />
          <rect x="436" y="456" width="128" height="160" />
          <circle cx="500" cy="456" r="64" />
          <path d="M210 24 V40 Q500 280 790 40 V24" />
        </g>
      </svg>
    </g>
  );
}

function WideCourt({
  plays,
  prompts,
  canRun,
  connected,
  composer,
  onRun,
  onAssign,
  onReset,
  onOpenSettings,
  width,
  height,
}: PointGuardCourtViewProps & { width: number; height: number }) {
  const reducedMotion = useReducedMotion();
  const size = { width, height };
  const court = useRef<HTMLDivElement>(null);
  const column = useRef<HTMLDivElement>(null);
  const [columnBottom, setColumnBottom] = useState(0);
  const [hover, setHover] = useState<Point | null>(null);
  const [passing, setPassing] = useState<number | null>(null);
  const [facesLeft, setFacesLeft] = useState(false);
  const passTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useLayoutEffect(() => {
    const element = column.current;
    const host = court.current;
    if (element === null || host === null) return;
    const measure = () => {
      const bottom = element.getBoundingClientRect().bottom - host.getBoundingClientRect().top;
      setColumnBottom((old) => (old === bottom ? old : bottom));
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [width, height]);

  // A pass interrupted by the court going away (a new chat, a resumed one) never sends.
  useEffect(
    () => () => {
      if (passTimer.current !== null) clearTimeout(passTimer.current);
    },
    [],
  );

  const positions = courtPositions(size, CHAT_CONTENT_WIDTH, columnBottom);
  const pointer = reducedMotion ? null : hover;
  const drifted = positions.map((point) =>
    pointer === null
      ? point
      : { x: point.x + (pointer.x - point.x) * 0.05, y: point.y + (pointer.y - point.y) * 0.05 },
  );
  const near = pointer === null ? null : nearestSlot(pointer, drifted);
  const lit = passing ?? near;
  const logo = logoPoint(size);
  const facing = lit !== null ? (drifted[lit] ?? null) : hover;
  const facingLeft = nextFacing(facing, logo.x, facesLeft);
  if (facingLeft !== facesLeft) setFacesLeft(facingLeft);

  const diameter = 128;
  const run = (play: PointGuardPlay) => {
    if (!canRun || passing !== null) return;
    if (reducedMotion) {
      onRun(play);
      return;
    }
    setPassing(play.slot);
    passTimer.current = setTimeout(() => {
      passTimer.current = null;
      setPassing(null);
      onRun(play);
    }, PASS_MS);
  };

  return (
    <div
      ref={court}
      className="pg-court"
      style={{ width, height }}
      onPointerMove={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        setHover({ x: event.clientX - rect.left, y: event.clientY - rect.top });
      }}
      onPointerLeave={() => {
        setHover(null);
      }}
    >
      <svg className="pg-court-svg" width={width} height={height} aria-hidden="true">
        <CourtLines width={width} height={height} />
        {pointer !== null && (
          <>
            <defs>
              <radialGradient
                id="pg-court-glow"
                cx={pointer.x}
                cy={pointer.y}
                r={420}
                gradientUnits="userSpaceOnUse"
              >
                <stop offset="0" className="pg-glow-inner" />
                <stop offset="1" className="pg-glow-outer" />
              </radialGradient>
            </defs>
            <rect width={width} height={height} fill="url(#pg-court-glow)" />
          </>
        )}
        <PassLine from={logo} to={lit === null ? null : (drifted[lit] ?? null)} />
      </svg>
      {plays.map((play) => (
        <PlayButton
          key={play.slot}
          play={play}
          lit={lit === play.slot}
          dimmed={passing !== null && passing !== play.slot}
          canRun={canRun}
          position={drifted[play.slot] ?? { x: 0, y: 0 }}
          menu={() => playMenu(play, prompts, onAssign, onReset)}
          onClick={() => {
            run(play);
          }}
        />
      ))}
      {/* Above the plays, so a play squeezed by a short pane can never take a composer click. */}
      <div
        className="pg-court-column"
        style={{
          width: Math.min(width, CHAT_CONTENT_WIDTH + 2 * CHAT_HORIZONTAL_PADDING),
          top: Math.max(16, logo.y - diameter / 2),
        }}
      >
        <div ref={column} className="pg-court-column-inner">
          <LogoMark
            diameter={diameter}
            tilt={pointer === null ? 0 : logoTilt(pointer, size)}
            facesLeft={facesLeft}
            reducedMotion={reducedMotion}
          />
          <div style={{ paddingTop: 36 }}>
            <Heading size={40} />
          </div>
          <div style={{ paddingTop: 24, width: '100%' }}>
            <Slot connected={connected} composer={composer} onOpenSettings={onOpenSettings} />
          </div>
        </div>
      </div>
    </div>
  );
}

function PlayPill({
  play,
  canRun,
  menu,
  onRun,
}: {
  play: PointGuardPlay;
  canRun: boolean;
  menu: () => MenuEntry[];
  onRun: (play: PointGuardPlay) => void;
}) {
  const context = useContextMenu(menu);
  return (
    <>
      <button
        type="button"
        className="pg-play-pill"
        data-testid={`pg-play-${String(play.slot)}`}
        disabled={!canRun}
        aria-description={play.prompt === '' ? 'Sends your saved prompt' : play.prompt}
        onClick={() => {
          if (canRun) onRun(play);
        }}
        onContextMenu={context.onContextMenu}
      >
        <Icon name={play.symbol} size={14} />
        <span>{play.title}</span>
      </button>
      {context.element}
    </>
  );
}

/** Narrow and sidebar: the logo, heading and composer, with the plays as pills under them. */
function StackedCourt({
  plays,
  prompts,
  canRun,
  connected,
  compact = false,
  composer,
  onRun,
  onAssign,
  onReset,
  onOpenSettings,
}: PointGuardCourtViewProps) {
  const reducedMotion = useReducedMotion();
  return (
    <div className="pg-court-stacked">
      <div className="pg-court-stacked-column">
        <LogoMark diameter={88} tilt={0} facesLeft={false} reducedMotion={reducedMotion} />
        <div style={{ paddingTop: 24, paddingInline: 16 }}>
          <Heading size={26} />
        </div>
        <div style={{ paddingTop: 16, width: '100%' }}>
          <Slot connected={connected} composer={composer} onOpenSettings={onOpenSettings} />
        </div>
        <div
          className="pg-play-pills"
          style={{ paddingInline: compact ? 8 : CHAT_HORIZONTAL_PADDING }}
        >
          {plays.map((play) => (
            <PlayPill
              key={play.slot}
              play={play}
              canRun={canRun}
              menu={() => playMenu(play, prompts, onAssign, onReset)}
              onRun={onRun}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
