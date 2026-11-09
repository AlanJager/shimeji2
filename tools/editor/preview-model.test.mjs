import test from 'node:test';
import assert from 'node:assert/strict';
import {currentPose, onionSamples, poseImages, poseMarkup} from './preview-model.mjs';

const frame = (id, ticks, x = 0, y = 0) => ({id, ticks, x, y, assetId: id});
const action = (loop = false) => ({id: 'walk', kind: 'frames', loop, frames: [frame('a', 2), frame('b', 7), frame('c', 3), frame('d', 1)]});
const assets = ['a', 'b', 'c', 'd'].map(id => ({id, src: `/assets/${id}.png`}));
const project = () => ({actions: [action()], assets});
const indexes = poses => poses.map(pose => pose.index);
const ticks = poses => poses.map(pose => pose.tick);

test('frame neighbors use actual duration boundaries, not neighboring ticks of the same pose', () => {
  const a = action();
  assert.deepEqual(onionSamples(a, 1, {count: 1, wrap: false}), {
    previous: [], next: [{index: 1, tick: 2, distance: 1}],
  });
  const samples = onionSamples(a, 4, {count: 2, wrap: false});
  assert.deepEqual(ticks(samples.previous), [0]);
  assert.deepEqual(ticks(samples.next), [9, 12]);
  assert.equal(poseImages(project(), a, 8)[0].assetId, 'b');
  assert.equal(poseImages(project(), a, 9)[0].assetId, 'c');
});

test('first and last poses do not cross boundaries unless loop wrap is requested', () => {
  const a = action();
  assert.deepEqual(indexes(onionSamples(a, 0, {count: 2}).previous), []);
  assert.deepEqual(indexes(onionSamples(a, 12, {count: 2}).next), []);
  const first = onionSamples(a, 0, {count: 2, wrap: true});
  assert.deepEqual(indexes(first.previous), [3, 2]);
  assert.deepEqual(indexes(first.next), [1, 2]);
  const last = onionSamples(a, 12, {count: 2, wrap: true});
  assert.deepEqual(indexes(last.next), [0, 1]);
});

test('loop wrapping skips current and repeated poses for short clips, count stays bounded', () => {
  const a = {kind: 'frames', loop: true, frames: [frame('a', 3), frame('b', 2)]};
  const samples = onionSamples(a, 0, {count: 3});
  assert.deepEqual(indexes(samples.previous), [1]);
  assert.deepEqual(indexes(samples.next), [1]);
  const still = {kind: 'frames', loop: true, frames: [frame('a', 3)]};
  assert.deepEqual(onionSamples(still, 0, {count: 3}), {previous: [], next: []});
  assert.equal(onionSamples(action(true), 5, {count: 100}).previous.length, 3);
});

test('frame markup retains translation, image source and no editor hit attributes', () => {
  const a = {kind: 'frames', frames: [frame('a', 5, -13, 7)]};
  const images = poseImages(project(), a, 3);
  assert.deepEqual(images, [{id: 'a', assetId: 'a', src: '/assets/a.png', matrix: [1, 0, 0, 1, -13, 7]}]);
  const markup = poseMarkup(images);
  assert.match(markup, /transform="matrix\(1 0 0 1 -13 7\)"/);
  assert.match(markup, /width="128" height="128"/);
  assert.match(markup, /pointer-events="none"/);
  assert.doesNotMatch(markup, /data-preview-part|onclick|onpointer/);
  assert.match(poseMarkup([{src: 'x" onload="unsafe', matrix: [1, 0, 0, 1, 0, 0]}]), /href="x&quot; onload=&quot;unsafe"/);
});

function multipart() {
  const root = {id: 'root', assetId: 'a', parentId: null, x: 64, y: 100, pivotX: 64, pivotY: 128, rotation: 90, scaleX: 1, scaleY: 1, visible: true};
  const child = {id: 'child', assetId: 'b', parentId: 'root', x: 10, y: -20, pivotX: 5, pivotY: 6, rotation: 0, scaleX: 1, scaleY: 1, visible: true};
  const a = {id: 'wave', kind: 'parts', durationTicks: 40, loop: false, frames: [frame('a', 1)], tracks: [{partId: 'child', keys: [
    {tick: 0, ...child}, {tick: 20, ...child, x: 30},
  ]}]};
  return {assets, parts: [child, root], actions: [a]};
}

const closeMatrix = (actual, expected) => actual.forEach((n, index) => assert.ok(Math.abs(n - expected[index]) < 1e-8, `matrix ${index}: ${n} != ${expected[index]}`));

test('multipart samples use short tick intervals and respect first/last boundaries', () => {
  const a = multipart().actions[0];
  assert.deepEqual(ticks(onionSamples(a, 10, {count: 3, wrap: false}).previous), [5, 0]);
  assert.deepEqual(ticks(onionSamples(a, 10, {count: 3, wrap: false}).next), [15, 20, 25]);
  assert.deepEqual(ticks(onionSamples(a, 0, {count: 2, wrap: true}).previous), [35, 30]);
  assert.deepEqual(ticks(onionSamples(a, 39, {count: 2, wrap: false}).next), []);
  const short = {...a, durationTicks: 4};
  assert.deepEqual(ticks(onionSamples(short, 0, {count: 1, wrap: true}).next), [1]);
});

test('multipart pose keeps parent rotation, interpolated child pivot and painter order', () => {
  const p = multipart(), a = p.actions[0];
  const before = JSON.stringify(p);
  const images = poseImages(p, a, 10);
  assert.deepEqual(images.map(image => image.id), ['child', 'root']);
  // Parent rotates child (20,-20) to (20,20), then translates to (84,120).
  // The child's (5,6) pivot under a 90° rotation moves its image to (90,115).
  closeMatrix(images[0].matrix, [0, 1, -1, 0, 90, 115]);
  assert.equal(JSON.stringify(p), before);
});

test('native current pose uses in-progress draft and inherited hidden states', () => {
  const p = multipart(), a = p.actions[0], draft = structuredClone(p);
  draft.parts.find(part => part.id === 'root').x = 74;
  closeMatrix(currentPose({project: p, action: a, tick: 0, partDraft: draft})[0].matrix, [0, 1, -1, 0, 100, 105]);
  draft.parts.find(part => part.id === 'root').visible = false;
  assert.deepEqual(currentPose({project: p, action: a, tick: 0, partDraft: draft}), []);
  assert.equal(p.parts.find(part => part.id === 'root').visible, true);
});

test('part editing endpoint shows the exact endpoint key rather than wrapping to tick zero', () => {
  const p = multipart(), a = p.actions[0];
  a.loop = true;
  a.tracks[0].keys[1].tick = 40;
  a.tracks[0].keys[1].visible = false;
  assert.equal(currentPose({project: p, action: a, tick: 0}).length, 2);
  assert.deepEqual(currentPose({project: p, action: a, tick: 40}).map(image => image.id), ['root']);
});

test('selection loop enters an outside focus at its first tick and wraps single-tick ranges', async () => {
  const {advanceLoopTick} = await import('./preview-model.mjs');
  assert.equal(advanceLoopTick(10,1,{start:5,end:10}),5);
  assert.equal(advanceLoopTick(0,1,{start:5,end:10}),5);
  assert.equal(advanceLoopTick(9,1,{start:5,end:10}),5);
  assert.equal(advanceLoopTick(6,7,{start:5,end:10}),8);
  assert.equal(advanceLoopTick(6,1,{start:5,end:6}),5);
});
