import test from 'node:test';
import assert from 'node:assert/strict';
import {applyFramePixels, floodFill, paintStroke} from './pixel-edit.mjs';

const blank = () => new Uint8ClampedArray(16);
const color = [255, 0, 0, 255];

test('pencil, symmetry and locked alpha only change paintable pixels', () => {
  const pixels = blank();
  pixels[4] = 1; pixels[7] = 200;
  const drawn = paintStroke(pixels, 2, 2, [{x: 0, y: 0}], {color, symmetry: true, lockAlpha: true});
  assert.deepEqual([...drawn.slice(0, 4)], [0, 0, 0, 0]);
  assert.equal(drawn[4], 255);
  assert.equal(drawn[7], 200);
  const open = paintStroke(blank(), 2, 2, [{x: 0, y: 0}, {x: 0, y: 1}], {color, symmetry: true});
  assert.equal(open[3], 255);
  assert.equal(open[7], 255);
  assert.equal(open[15], 255);
});

test('fill replaces a connected color and can mirror the seed', () => {
  const pixels = blank();
  pixels.set([0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9, 255, 0, 0, 0, 255]);
  const filled = floodFill(pixels, 2, 2, 0, 0, {color, symmetry: true});
  assert.deepEqual([...filled.slice(0, 4)], color);
  assert.deepEqual([...filled.slice(4, 8)], color);
  assert.deepEqual([...filled.slice(12, 16)], color);
  assert.equal(filled[8], 9);
});

test('a shared frame is detached before its pixels change', () => {
  const png = 'data:image/png;base64,cG5n';
  const project = {assets: [{id: 'a', name: 'eye', src: png}], parts: [], actions: [
    {id: 'idle', kind: 'frames', frames: [{id: 'f1', assetId: 'a'}, {id: 'f2', assetId: 'a'}]},
  ]};
  const result = applyFramePixels(project, 'idle', 'f1', png + 'Yg==', () => 'copy');
  assert.equal(result.detached, true);
  assert.equal(project.actions[0].frames[0].assetId, 'copy');
  assert.equal(project.actions[0].frames[1].assetId, 'a');
  assert.equal(project.assets[0].src, png);
  applyFramePixels(project, 'idle', 'f2', png + 'Yw==', () => 'again');
  assert.equal(project.assets[0].src, png + 'Yw==');
  assert.equal(project.assets.length, 2);
});
