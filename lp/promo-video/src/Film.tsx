import React from 'react';
import {AbsoluteFill, Audio, Img, interpolate, Sequence, staticFile, useCurrentFrame} from 'remotion';
import {BexMark} from './components/BexMark';
import {ProviderLogo} from './components/ProviderLogo';
import {b, C, clamp, FONT, FPS, FRAMES, Key, mix, pop, track, visible} from './theme';
import {Chip, Clip, Footnote, SCREEN, Segment, softShadow, speedAt, toScreen, Touch, Words} from './ui';

// One continuous take at 120 BPM: a teal signal becomes the iPhone, travels to the Mac, turns into
// the memory chart, the agent picker and finally the antenna of the logo. Beats are half seconds.

// Measured on 2026-10-31: data/ios-connections.jsonl and data/memory-macos.json.
const LATENCY_MS = 43.5;
const MEMORY = {bex: 108, claude: 835, codex: 2815};

// Mixkit "Hazy After Hours" (120 BPM) builds 15.88 s in; starting 7.88 s in lands it on beat 16.
const MUSIC_START_SECONDS = 7.88;

type Camera = [beat: number, x: number, y: number, scale: number];
const CAMERA: Camera[] = [
  [0, 960, 540, 1],
  [10, 960, 540, 1],
  [10.8, 1060, 380, 1.55],
  [11.6, 1060, 380, 1.55],
  [12.6, 960, 540, 1],
  [20, 960, 540, 1],
  [24, 2450, 540, 1],
  [26, 2450, 540, 1],
  [27, 2450, 520, 1.15],
  [27.6, 2450, 520, 1.15],
  [28.4, 2450, 540, 1],
  [40, 2450, 540, 1],
  [41.5, 960, 540, 1],
  [44, 960, 540, 1],
  [45, 600, 576, 1.9],
  [46.6, 600, 576, 1.9],
  [47.6, 960, 540, 1],
  [49.8, 960, 540, 1],
  [50.6, 600, 775, 1.9],
  [56.9, 600, 775, 1.9],
  [58.6, 600, 566, 1.75],
  [62, 600, 566, 1.75],
  [63.5, 960, 540, 1],
  [70, 960, 540, 1],
  [72, 960, 540, 0.55],
];
const axis = (index: 1 | 2 | 3): Key[] => CAMERA.map((key) => [key[0], key[index]]);

const PHONE_X: Key[] = [[0, 960], [12, 960], [13.5, 600], [62, 600], [63.5, 470]];
const PHONE_Y: Key[] = [[0, 540], [62, 540], [63.5, 560]];
const PHONE_AT_CLAUDE = {x: 600, y: 540};
const PHONE_AT_SYNC = {x: 470, y: 560};

// iphone-claude.mp4: send at 1.6 s; the docked Edit request appears at 13.3 s and is approved at 15.3 s,
// the Bash request that replaces it at 17.7 s; the answer arrives at 21.5 s.
const CLAUDE_AT = 44;
const CLAUDE: Segment[] = [
  {from: 0.6, to: 2.4},
  {from: 2.4, to: 13.0, rate: 8},
  {from: 13.0, to: 18.4, rate: 1.5},
  {from: 18.4, to: 21.4, rate: 8},
  {from: 21.4, to: 23.6},
];
// Both clips are cut to the same moment: the follow-up is sent at 2.1 s and answered at 4.0 s on each device.
const SYNC_AT = 63.5;
const SYNC: Segment[] = [{from: 0.5, to: 8, rate: 2}];

const TAPS = [
  {at: 11, ...toScreen(1020, 666, {x: 960, y: 540})},
  {at: 46, ...toScreen(1082, 1502, PHONE_AT_CLAUDE)},
  {at: 53.37, ...toScreen(169, 2175, PHONE_AT_CLAUDE)},
  {at: 56.48, ...toScreen(169, 2175, PHONE_AT_CLAUDE)},
  {at: 65, ...toScreen(1082, 1502, PHONE_AT_SYNC)},
];

const centered: React.CSSProperties = {position: 'absolute', transform: 'translateX(-50%)', textAlign: 'center'};

const PhoneContent: React.FC<{frame: number}> = ({frame}) => {
  if (frame < b(40)) {
    // The icon reads "bex" from 0.5 s, the moment it is tapped.
    return <Clip src="footage/iphone-launch.mp4" at={11} segments={[{from: 0.5, to: 2.3}]} />;
  }
  if (frame < b(44)) {
    return <Clip src="footage/iphone-picker.mp4" at={40.3} segments={[{from: 0, to: 2.4}]} />;
  }
  if (frame < b(62.5)) {
    return <Clip src="footage/iphone-claude.mp4" at={CLAUDE_AT} segments={CLAUDE} />;
  }
  return <Clip src="footage/iphone-followup.mp4" at={SYNC_AT} segments={SYNC} />;
};

// The opening dot grows into the phone; content swaps blur briefly.
const Phone: React.FC = () => {
  const frame = useCurrentFrame();
  const x = track(frame, PHONE_X);
  const y = track(frame, PHONE_Y);
  const morph = track(frame, [[8, 0], [9.5, 1]]);
  const w = 28 + (SCREEN.w - 28) * morph;
  const h = 28 + (SCREEN.h - 28) * morph;
  const radius = 14 + 44 * morph;
  // Before the morph the dot pulses on every beat.
  const pulse = morph > 0 ? 1 : pop(frame, 0) * (frame >= b(1) ? 1 + 0.22 * Math.exp(-(frame % b(1)) / 7) : 1);
  const bezel = 13 * track(frame, [[9, 0], [9.6, 1]]);
  const blur = [40, 44, 62.5].reduce((sum, at) => sum + interpolate(frame, [b(at) - 6, b(at), b(at) + 8], [0, 10, 0], clamp), 0);
  const opacity = track(frame, [[70, 1], [71.5, 0]]);
  return (
    <div style={{position: 'absolute', left: x - w / 2, top: y - h / 2, width: w, height: h, transform: `scale(${pulse})`, opacity}}>
      {bezel > 0 ? (
        <div style={{position: 'absolute', inset: -bezel, borderRadius: radius + bezel, background: '#1d1d1f', border: '1.5px solid #3c3c3e', boxShadow: softShadow}} />
      ) : null}
      <div style={{position: 'absolute', inset: 0, borderRadius: radius, overflow: 'hidden', background: mix(C.teal, '#000000', track(frame, [[8.2, 0], [8.9, 1]]))}}>
        {frame >= b(10.2) ? (
          <div style={{position: 'absolute', inset: 0, opacity: track(frame, [[10.2, 0], [10.5, 1]]), filter: `blur(${blur}px)`}}>
            <PhoneContent frame={frame} />
          </div>
        ) : null}
        <div style={{position: 'absolute', top: 12, left: '50%', width: 124, height: 35, marginLeft: -62, borderRadius: 999, background: '#000', opacity: morph}} />
      </div>
    </div>
  );
};

const Signal: React.FC = () => {
  const frame = useCurrentFrame();
  const p = track(frame, [[19.4, 0], [23.4, 1]]);
  const from = 813;
  const to = 1900;
  const head = from + (to - from) * p;
  if (frame < b(19.4) || frame > b(28)) {
    return null;
  }
  return (
    <div style={{opacity: track(frame, [[27.2, 1], [27.8, 0]])}}>
      <div style={{position: 'absolute', left: from, top: 538, width: head - from, height: 4, borderRadius: 2, background: C.teal}} />
      {p < 1 ? <div style={{position: 'absolute', left: head - 14, top: 526, width: 28, height: 28, borderRadius: '50%', background: C.teal}} /> : null}
      <Chip at={20.6} style={{left: 1130, top: 470}}>QUIC</Chip>
      <Chip at={21.4} style={{left: 1400, top: 470}}>iroh</Chip>
      <Chip at={22.2} style={{left: 1690, top: 470}}>TLS 1.3 で暗号化</Chip>
    </div>
  );
};

// The pointer rested on the branch chip while the 1440×960 Mac window was captured; cover its tooltip
// with the background of the row it overlaps.
const TooltipCover: React.FC = () => (
  <div style={{position: 'absolute', left: `${(524 / 1440) * 100}%`, top: `${(717 / 960) * 100}%`, width: `${(158 / 1440) * 100}%`, height: `${(37 / 960) * 100}%`, background: '#141414'}} />
);

// The Mac window becomes the bex memory bar, then the agent pill that lands on the iPhone's agent control.
const MacMorph: React.FC = () => {
  const frame = useCurrentFrame();
  if (frame < b(23.4) || frame > b(41.8)) {
    return null;
  }
  const control = toScreen(602, 790, PHONE_AT_CLAUDE);
  const left = track(frame, [[28.4, 1900], [29.5, 2240], [35, 2240], [36.5, 2040], [40, 2040], [41.5, control.x - 84]]);
  const top = track(frame, [[28.4, 173.3], [29.5, 625], [35, 625], [36.5, 488], [40, 488], [41.5, control.y - 11]]);
  const width = track(frame, [[28.4, 1100], [29.5, 34.5], [35, 34.5], [36.5, 820], [40, 820], [41.5, 168]]);
  const height = track(frame, [[28.4, 733.3], [29.5, 30], [35, 30], [36.5, 104], [40, 104], [41.5, 22]]);
  const radius = track(frame, [[28.4, 12], [29.5, 15], [35, 15], [36.5, 52], [40, 52], [41.5, 11]]);
  const toTeal = track(frame, [[28.9, 0], [29.5, 1]]);
  const toInk = track(frame, [[35, 0], [36.5, 1]]);
  const color = toInk > 0 ? mix(C.teal, C.ink, toInk) : mix('#0f0f10', C.teal, toTeal);
  const enter = pop(frame, 23.4);
  const opacity = Math.min(1, enter * 1.5) * track(frame, [[41.2, 1], [41.7, 0]]);
  const scale = frame < b(28) ? 0.94 + 0.06 * enter : 1;
  return (
    <div style={{position: 'absolute', left, top, width, height, borderRadius: radius, background: color, overflow: 'hidden', opacity, transform: `scale(${scale})`, boxShadow: softShadow}}>
      <div style={{position: 'absolute', inset: 0, opacity: track(frame, [[28.2, 1], [28.7, 0]])}}>
        <Img src={staticFile('footage/mac-conversation.png')} style={{width: '100%', height: '100%'}} />
        <TooltipCover />
      </div>
    </div>
  );
};

const Bar: React.FC<{y: number; label: string; mb: number; at: number; accent?: boolean}> = ({y, label, mb, at, accent}) => {
  const frame = useCurrentFrame();
  const grow = track(frame, [[at, 0], [at + 1.5, 1], [35, 1], [36, 0]]);
  const fade = visible(frame, at, 35.6);
  const width = accent ? 34.5 : (900 * mb) / MEMORY.codex;
  const shown = accent ? mb : Math.round(mb * track(frame, [[at, 0], [at + 1.5, 1]]));
  const valueAt = accent ? pop(frame, at) : 1;
  return (
    <>
      <div style={{position: 'absolute', left: 1700, top: y - 22, width: 510, textAlign: 'right', fontFamily: FONT, fontSize: 30, fontWeight: 700, color: accent ? C.ink : C.muted, opacity: fade}}>
        {label}
      </div>
      {accent ? null : <div style={{position: 'absolute', left: 2240, top: y - 11, width: width * grow, height: 22, borderRadius: 11, background: C.bar}} />}
      <div
        style={{
          position: 'absolute',
          left: 2240 + width * (accent ? 1 : grow) + 24,
          top: y - (accent ? 40 : 24),
          fontFamily: FONT,
          fontSize: accent ? 64 : 38,
          fontWeight: 800,
          color: accent ? C.teal : C.muted,
          fontVariantNumeric: 'tabular-nums',
          opacity: fade * Math.min(1, valueAt),
          transform: `scale(${accent ? 0.8 + 0.2 * valueAt : 1})`,
          transformOrigin: 'left center',
          whiteSpace: 'nowrap',
        }}
      >
        {shown.toLocaleString('en-US')} MB
      </div>
    </>
  );
};

const AGENTS = [
  {name: 'Claude Code', logo: 'claude', color: C.claude},
  {name: 'Codex', logo: 'openai', color: '#ffffff'},
  {name: 'DSH', logo: 'deepseek', color: '#8FA3FF'},
] as const;

const AgentPill: React.FC = () => {
  const frame = useCurrentFrame();
  const leave = track(frame, [[40, 0], [40.4, 1]]);
  if (frame < b(36.3) || frame > b(40.5)) {
    return null;
  }
  return (
    <div style={{position: 'absolute', left: 2040, top: 488, width: 820, height: 104, display: 'flex', alignItems: 'center', justifyContent: 'space-evenly', opacity: 1 - leave}}>
      {AGENTS.map((agent, i) => {
        const p = pop(frame, 36.5 + i * 0.4);
        return (
          <div key={agent.name} style={{display: 'flex', alignItems: 'center', gap: 12, fontFamily: FONT, fontSize: 34, fontWeight: 700, color: '#fff', opacity: Math.min(1, p * 1.5), transform: `translateY(${(1 - p) * 30}px)`}}>
            <ProviderLogo provider={agent.logo} size={36} color={agent.color} />
            {agent.name}
          </div>
        );
      })}
    </div>
  );
};

const MacSync: React.FC = () => {
  const frame = useCurrentFrame();
  if (frame < b(62.4)) {
    return null;
  }
  const x = track(frame, [[62.5, 2100], [64, 1330]]);
  const opacity = track(frame, [[62.5, 0], [63.2, 1], [70, 1], [71.5, 0]]);
  return (
    <div style={{position: 'absolute', left: x - 500, top: 560 - 333.3, width: 1000, height: 666.7, borderRadius: 12, overflow: 'hidden', boxShadow: softShadow, opacity}}>
      <Clip src="footage/mac-followup.mp4" at={SYNC_AT} segments={SYNC} />
      <TooltipCover />
    </div>
  );
};

const World: React.FC = () => {
  const frame = useCurrentFrame();
  const x = track(frame, axis(1));
  const y = track(frame, axis(2));
  const s = track(frame, axis(3));
  return (
    <AbsoluteFill style={{transformOrigin: '0 0', transform: `translate(960px, 540px) scale(${s}) translate(${-x}px, ${-y}px)`}}>
      <Signal />
      <MacMorph />
      <Bar y={400} label="Codex（ChatGPT アプリ）" mb={MEMORY.codex} at={29} />
      <Bar y={520} label="Claude デスクトップ" mb={MEMORY.claude} at={29.75} />
      <Bar y={640} label="bex（デスクトップ＋Host）" mb={MEMORY.bex} at={31.5} accent />
      <Chip at={32.5} out={35} style={{left: 2380, top: 780}}>
        Claude の約 <span style={{color: C.teal}}>1/{Math.round(MEMORY.claude / MEMORY.bex)}</span>
      </Chip>
      <Chip at={33.25} out={35} style={{left: 2740, top: 780}}>
        Codex の約 <span style={{color: C.teal}}>1/{Math.round(MEMORY.codex / MEMORY.bex)}</span>
      </Chip>
      <AgentPill />
      <div style={{...centered, left: 2450, top: 330}}>
        <Words parts={['エージェントは、どれでも。']} at={37} out={40.2} size={64} />
      </div>
      <div style={{...centered, left: 2450, top: 640}}>
        <Words parts={['いつものアカウントや API キーで']} at={37.8} out={40.2} size={30} weight={500} color={C.muted} />
      </div>
      <MacSync />
      <Phone />
      {TAPS.map((tap) => (
        <Touch key={tap.at} x={tap.x} y={tap.y} at={tap.at} />
      ))}
    </AbsoluteFill>
  );
};

const Caption: React.FC<{lines: [string, string]; at: number; out: number; left?: number}> = ({lines, at, out, left = 1400}) => (
  <div style={{position: 'absolute', left, top: 400}}>
    <Words parts={[lines[0]]} at={at} out={out} size={52} weight={700} color={C.muted} />
    <Words parts={[lines[1]]} at={at + 0.3} out={out} size={76} />
  </div>
);

const Connection: React.FC = () => {
  const frame = useCurrentFrame();
  // The figure counts in real time on the drop: 43.5 ms is under three frames, so it lands at once.
  const elapsed = ((frame - b(16)) / FPS) * 1000;
  const count = Math.min(LATENCY_MS, elapsed);
  const punch = 1 + 0.05 * Math.exp(-Math.max(0, frame - b(16)) / 8) * (frame >= b(16) ? 1 : 0);
  const shown = visible(frame, 13.6, 19.8, 12);
  return (
    <div style={{position: 'absolute', left: 1010, top: 290, opacity: shown}}>
      <Words parts={['iPhone → PC の接続']} at={13} size={40} weight={700} color={C.muted} />
      <div style={{display: 'flex', alignItems: 'baseline', gap: 14, fontFamily: FONT, fontWeight: 800, color: C.ink, opacity: elapsed >= 0 ? 1 : 0, transform: `scale(${punch})`, transformOrigin: 'left center'}}>
        <span style={{fontSize: 230, letterSpacing: '-0.05em', fontVariantNumeric: 'tabular-nums', lineHeight: 1}}>{count.toFixed(1)}</span>
        <span style={{fontSize: 84}}>ms</span>
      </div>
      <div style={{width: 470 * track(frame, [[16, 0], [16.8, 1]]), height: 8, borderRadius: 4, background: C.teal, marginTop: 6}} />
      <Words parts={['中央値（実機 iPhone・新規接続 42 回）']} at={16.5} size={30} weight={500} color={C.muted} style={{marginTop: 24}} />
      <Chip at={18} style={{left: 250, top: 470}}>
        <span style={{color: C.teal}}>98%</span> が P2P で直結
      </Chip>
    </div>
  );
};

const Opening: React.FC = () => (
  <>
    <div style={{...centered, left: 960, top: 610}}>
      <Words parts={['PCで動くエージェントを、']} at={1.5} out={4.6} size={64} />
    </div>
    <div style={{...centered, left: 960, top: 610}}>
      <Words parts={['ポケットから動かす。']} at={5} out={7.6} size={64} />
    </div>
  </>
);

const SpeedBadge: React.FC = () => {
  const frame = useCurrentFrame();
  const clips = [
    {at: CLAUDE_AT, segments: CLAUDE},
    {at: SYNC_AT, segments: SYNC},
  ];
  const rate = clips.map((clip) => speedAt(frame, clip.at, clip.segments)).find((value) => value !== 1);
  if (!rate) {
    return null;
  }
  return (
    <div style={{position: 'absolute', left: 64, top: 56, padding: '8px 18px', borderRadius: 999, background: C.ink, color: '#fff', fontFamily: FONT, fontSize: 24, fontWeight: 700}}>
      ▶▶ 早送り ×{rate}
    </div>
  );
};

// The signal returns as the antenna of the logo.
const Ending: React.FC = () => {
  const frame = useCurrentFrame();
  if (frame < b(70.8)) {
    return null;
  }
  const mark = {x: 640, y: 360, size: 220};
  const antenna = {x: mark.x + mark.size * (660 / 1024), y: mark.y + mark.size * (236 / 1024)};
  const travel = track(frame, [[71.5, 0], [72.5, 1]]);
  const dot = 28 + (mark.size * (144 / 1024) - 28) * travel;
  const wipe = track(frame, [[72.4, 100], [73.3, 0]]);
  return (
    <>
      <div style={{position: 'absolute', left: mark.x, top: mark.y, clipPath: `inset(${wipe}% 0 0 0)`}}>
        <BexMark size={mark.size} body={C.ink} face={C.canvas} dot="transparent" />
      </div>
      <div
        style={{
          position: 'absolute',
          left: 960 + (antenna.x - 960) * travel - dot / 2,
          top: 540 + (antenna.y - 540) * travel - dot / 2,
          width: dot,
          height: dot,
          borderRadius: '50%',
          background: C.teal,
          transform: `scale(${pop(frame, 70.8)})`,
        }}
      />
      <div style={{position: 'absolute', left: mark.x + mark.size + 36, top: mark.y + 6}}>
        <Words parts={['bex']} at={73.2} size={210} />
      </div>
      <div style={{...centered, left: 960, top: 640}}>
        <Words parts={['どこでも、これひとつで。']} at={74} size={58} />
      </div>
      <Chip at={75} style={{left: 960, top: 800, fontSize: 34}}>
        getbex.dev
      </Chip>
      <div style={{...centered, left: 960, top: 870}}>
        <Words parts={['macOS · Windows · Linux · iPhone · Android']} at={75.6} size={24} weight={600} color={C.muted} />
      </div>
    </>
  );
};

const SOUNDS: [beat: number, name: 'tap' | 'pop' | 'whoosh'][] = [
  [0, 'pop'],
  [8, 'whoosh'],
  ...TAPS.map((tap) => [tap.at, 'tap'] as [number, 'tap']),
  [18, 'pop'],
  [19.4, 'whoosh'],
  [20.6, 'pop'],
  [21.4, 'pop'],
  [22.2, 'pop'],
  [32.5, 'pop'],
  [33.25, 'pop'],
  [35, 'whoosh'],
  [40, 'whoosh'],
  [62.5, 'whoosh'],
  [70, 'whoosh'],
  [75, 'pop'],
];

export const Film: React.FC = () => (
    <AbsoluteFill style={{background: `radial-gradient(ellipse 75% 70% at 50% 45%, #F3F1EC, ${C.canvas})`}}>
      <World />
      <Opening />
      <Connection />
      <div style={{...centered, left: 960, top: 70}}>
        <Words parts={['Mac 版も、Rust + GPUI で軽い。']} at={24.5} out={28} size={56} />
      </div>
      <div style={{...centered, left: 960, top: 70}}>
        <Words parts={['メモリ使用量、実測。']} at={29} out={35} size={56} />
      </div>
      <Caption lines={['使い分けも、', 'ひとつのアプリで。']} at={41.6} out={43.8} left={1060} />
      <Caption lines={['iPhone から、', '指示して。']} at={44.5} out={49.8} />
      <Caption lines={['承認も、', 'その場で。']} at={50.6} out={57.2} />
      <Caption lines={['テストが通るまで、', '手のひらで。']} at={58.6} out={62} />
      <div style={{position: 'absolute', left: 830, top: 132}}>
        <Words parts={['iPhone で送ると、Mac にもすぐ届く。']} at={63.6} out={69.6} size={52} />
      </div>
      <SpeedBadge />
      <Ending />
      <Footnote from={14} to={20}>
        ※ 実機 iPhone から bex の Host への接続時間（暗号化した接続と認証が完了するまで）。2026年10月31日、直結経路の新規接続 42 回の中央値。直結は 52 回中 51 回。画面は実際のアプリを iOS Simulator で録画したものです。
      </Footnote>
      <Footnote from={29} to={35.6}>
        ※ 2026年10月31日、同じ Mac で普段どおり使用中に計測した物理メモリ（top の MEM）。各アプリの全プロセスの合計で、エージェントの CLI は除外、bex はデスクトップと Host の合計。10秒間隔5回の中央値。開いている会話や機能はアプリごとに異なります。
      </Footnote>
      <Audio
        src={staticFile('music.mp3')}
        startFrom={Math.round(MUSIC_START_SECONDS * 60)}
        volume={(f) => interpolate(f, [0, 12, b(76), FRAMES], [0, 0.7, 0.7, 0], clamp)}
      />
      {SOUNDS.map(([beat, name]) => (
        <Sequence key={`${beat}-${name}`} from={b(beat)} durationInFrames={60} layout="none">
          <Audio src={staticFile(`sfx/${name}.wav`)} volume={name === 'whoosh' ? 0.3 : 0.4} />
        </Sequence>
      ))}
    </AbsoluteFill>
);
