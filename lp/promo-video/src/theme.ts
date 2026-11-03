import {loadFont as loadGeist} from '@remotion/google-fonts/Geist';
import {loadFont as loadNotoSansJP} from '@remotion/google-fonts/NotoSansJP';
import {Easing, interpolate, spring} from 'remotion';

const geist = loadGeist('normal', {weights: ['500', '600', '700', '800'], subsets: ['latin']});
const notoSansJP = loadNotoSansJP('normal', {weights: ['500', '700', '900'], subsets: ['japanese', 'latin'], ignoreTooManyRequestsWarning: true});

export const FONT = `${geist.fontFamily}, ${notoSansJP.fontFamily}, sans-serif`;

// Warm off-white canvas, black UI and one accent: the teal of the logo's antenna.
export const C = {
  canvas: '#ECE9E3',
  ink: '#141414',
  muted: '#77716A',
  bar: '#B9B3AA',
  teal: '#19C2B8',
  claude: '#D97757',
};

export const FPS = 60;
export const FRAMES = 40 * FPS;
// The track runs at 120 BPM: one beat is half a second.
export const b = (beats: number) => Math.round((beats * FPS) / 2);

export const clamp = {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'} as const;
// Every morph and camera move shares one curve.
const EASE = Easing.bezier(0.45, 0, 0.15, 1);

export type Key = [beat: number, value: number];

// Holds before the first key and after the last; eases between neighbouring keys.
export const track = (frame: number, keys: Key[]) => {
  for (let i = 1; i < keys.length; i++) {
    const [b0, v0] = keys[i - 1];
    const [b1, v1] = keys[i];
    if (frame < b(b1)) {
      return interpolate(frame, [b(b0), b(b1)], [v0, v1], {...clamp, easing: EASE});
    }
  }
  return keys[keys.length - 1][1];
};

// Springs with only a slight overshoot.
export const pop = (frame: number, beat: number) =>
  spring({frame: frame - b(beat), fps: FPS, config: {damping: 20, stiffness: 170, mass: 0.8}});

// 1 between the two beats, with short fades at both ends.
export const visible = (frame: number, from: number, to: number, fade = 8) =>
  interpolate(frame, [b(from), b(from) + fade, b(to) - fade, b(to)], [0, 1, 1, 0], clamp);

export const mix = (from: string, to: string, p: number) => {
  const channel = (hex: string, i: number) => parseInt(hex.slice(1 + i * 2, 3 + i * 2), 16);
  const value = [0, 1, 2].map((i) => Math.round(channel(from, i) + (channel(to, i) - channel(from, i)) * p));
  return `rgb(${value.join(',')})`;
};
