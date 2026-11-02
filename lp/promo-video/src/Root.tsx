import React from 'react';
import {Composition} from 'remotion';
import {Film} from './Film';
import {FPS, FRAMES} from './theme';

export const Root: React.FC = () => (
  <Composition id="BexPromoJa" component={Film} durationInFrames={FRAMES} fps={FPS} width={1920} height={1080} />
);
