import React from 'react';

// Vector redraw of apps/nsh/assets/logo.png so each part can take its own color.
export const BexMark: React.FC<{size: number; body: string; face: string; dot: string; style?: React.CSSProperties}> = ({
  size,
  body,
  face,
  dot,
  style,
}) => (
  <svg width={size} height={size} viewBox="0 0 1024 1024" style={{overflow: 'visible', ...style}}>
    <rect x={637} y={280} width={44} height={140} fill={body} />
    <path d="M316 236 H418 Q476 236 476 294 V386 H560 A235 235 0 0 1 560 856 H316 Q258 856 258 798 V294 Q258 236 316 236 Z" fill={body} />
    <rect x={348} y={502} width={337} height={222} rx={100} fill={face} />
    <rect x={413} y={567} width={54} height={92} rx={27} fill={body} />
    <rect x={560} y={567} width={54} height={92} rx={27} fill={body} />
    <circle cx={660} cy={236} r={72} fill={dot} />
  </svg>
);
