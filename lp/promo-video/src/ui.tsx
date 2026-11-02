import React from 'react';
import {Freeze, interpolate, OffthreadVideo, Sequence, staticFile, useCurrentFrame} from 'remotion';
import {b, C, clamp, FONT, FPS, pop, track} from './theme';

export type Part = string | {t: string; color: string};

// Characters rise from a mask line on `at` and leave upwards on `out`.
export const Words: React.FC<{
  parts: Part[];
  at: number;
  out?: number;
  size: number;
  weight?: number;
  color?: string;
  style?: React.CSSProperties;
}> = ({parts, at, out, size, weight = 800, color = C.ink, style}) => {
  const frame = useCurrentFrame();
  const leave = out === undefined ? 0 : track(frame, [[out, 0], [out + 0.6, 1]]);
  let index = 0;
  return (
    <div style={{fontFamily: FONT, fontSize: size, fontWeight: weight, color, letterSpacing: '-0.03em', lineHeight: 1.18, whiteSpace: 'nowrap', overflow: 'hidden', paddingBottom: size * 0.08, ...style}}>
      {parts.map((part, partIndex) => {
        const {t, color: partColor} = typeof part === 'string' ? {t: part, color: undefined} : part;
        return Array.from(t).map((char, charIndex) => {
          const rise = pop(frame, at + index++ * 0.035);
          return (
            <span
              key={`${partIndex}-${charIndex}`}
              style={{display: 'inline-block', whiteSpace: 'pre', color: partColor, transform: `translateY(${(1 - rise + leave) * 110}%)`}}
            >
              {char}
            </span>
          );
        });
      })}
    </div>
  );
};

export const Chip: React.FC<{at: number; out?: number; children: React.ReactNode; style?: React.CSSProperties}> = ({at, out, children, style}) => {
  const frame = useCurrentFrame();
  const p = pop(frame, at);
  const leave = out === undefined ? 0 : track(frame, [[out, 0], [out + 0.5, 1]]);
  return (
    <div
      style={{
        position: 'absolute',
        display: 'flex',
        alignItems: 'center',
        gap: 14,
        padding: '14px 28px',
        borderRadius: 999,
        background: C.ink,
        color: '#fff',
        fontFamily: FONT,
        fontSize: 30,
        fontWeight: 700,
        whiteSpace: 'nowrap',
        boxShadow: '0 18px 40px rgba(20,20,20,0.16), 0 4px 10px rgba(20,20,20,0.08)',
        opacity: interpolate(p, [0, 0.3], [0, 1], clamp) * (1 - leave),
        transform: `translate(-50%, -50%) scale(${0.6 + 0.4 * p - 0.2 * leave})`,
        ...style,
      }}
    >
      {children}
    </div>
  );
};

export const Footnote: React.FC<{from: number; to: number; children: React.ReactNode}> = ({from, to, children}) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame, [b(from), b(from) + 12, b(to) - 12, b(to)], [0, 1, 1, 0], clamp);
  return (
    <div style={{position: 'absolute', left: 64, right: 64, bottom: 40, fontFamily: FONT, fontSize: 17, lineHeight: 1.6, color: C.muted, opacity}}>
      {children}
    </div>
  );
};

// A run of source seconds played at one speed.
export type Segment = {from: number; to: number; rate?: number};

const timed = (segments: Segment[]) => {
  let start = 0;
  return segments.map((segment) => {
    const rate = segment.rate ?? 1;
    const frames = Math.round(((segment.to - segment.from) / rate) * FPS);
    const part = {...segment, rate, start, frames};
    start += frames;
    return part;
  });
};

const fill: React.CSSProperties = {position: 'absolute', inset: 0, width: '100%', height: '100%', objectFit: 'cover'};

const Still: React.FC<{src: string; second: number}> = ({src, second}) => (
  <Freeze frame={0}>
    <OffthreadVideo src={staticFile(src)} startFrom={Math.round(second * FPS)} muted style={fill} />
  </Freeze>
);

// Real footage that starts playing at beat `at`, shows its first frame before and holds its last frame after.
export const Clip: React.FC<{src: string; at: number; segments: Segment[]}> = ({src, at, segments}) => {
  const frame = useCurrentFrame();
  const parts = timed(segments);
  const last = parts[parts.length - 1];
  const start = b(at);
  if (frame < start) {
    return <Still src={src} second={segments[0].from} />;
  }
  if (frame >= start + last.start + last.frames) {
    return <Still src={src} second={last.to - 1 / FPS} />;
  }
  return (
    <>
      {parts.map((part) => (
        <Sequence key={part.start} from={start + part.start} durationInFrames={part.frames} layout="none">
          <OffthreadVideo src={staticFile(src)} startFrom={Math.round(part.from * FPS)} playbackRate={part.rate} muted style={fill} />
        </Sequence>
      ))}
    </>
  );
};

export const speedAt = (frame: number, at: number, segments: Segment[]) =>
  timed(segments).find((part) => frame >= b(at) + part.start && frame < b(at) + part.start + part.frames)?.rate ?? 1;

// iPhone 17 Pro recordings are 1206×2622; the screen is drawn in world units.
export const SCREEN = {w: 400, h: (400 * 2622) / 1206};
export const toScreen = (x: number, y: number, center: {x: number; y: number}) => ({
  x: center.x - SCREEN.w / 2 + (x * SCREEN.w) / 1206,
  y: center.y - SCREEN.h / 2 + (y * SCREEN.w) / 1206,
});

// A finger: appears, presses on `at`, then lifts.
export const Touch: React.FC<{x: number; y: number; at: number}> = ({x, y, at}) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame, [b(at - 0.6), b(at - 0.3), b(at + 0.5), b(at + 0.9)], [0, 1, 1, 0], clamp);
  const press = interpolate(frame, [b(at - 0.15), b(at), b(at + 0.25)], [1, 0.82, 1], clamp);
  const ring = interpolate(frame, [b(at), b(at + 0.7)], [0, 1], clamp);
  return (
    <>
      <div style={{position: 'absolute', left: x - 22, top: y - 22, width: 44, height: 44, borderRadius: '50%', background: 'rgba(255,255,255,0.42)', border: '2px solid rgba(255,255,255,0.85)', boxShadow: '0 4px 14px rgba(0,0,0,0.35)', opacity, transform: `scale(${press})`}} />
      {frame >= b(at) ? (
        <div style={{position: 'absolute', left: x - 22, top: y - 22, width: 44, height: 44, borderRadius: '50%', border: '2px solid rgba(255,255,255,0.9)', opacity: 1 - ring, transform: `scale(${1 + ring * 1.4})`}} />
      ) : null}
    </>
  );
};

export const softShadow = '0 40px 80px rgba(20,20,20,0.18), 0 12px 24px rgba(20,20,20,0.10)';
