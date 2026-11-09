import test from 'node:test';
import assert from 'node:assert/strict';
import {SEQUENCE_LIMITS, pngSize, validateSources, naturalOrder, gridFrames, normalizationPlan, placeSlices, checkImportCapacity} from './sequence-model.mjs';

const source = (name, width = 128, height = 128, size = 1000) => ({name, width, height, size});
const rect = (width, height) => ({sourceIndex: 0, x: 0, y: 0, width, height});

test('natural ordering is numeric, deterministic and does not alter selected files', () => {
  const files = Object.freeze([source('walk10.png'), source('walk2.png'), source('walk1.png')]);
  assert.deepEqual(naturalOrder(files).map(s => s.name), ['walk1.png','walk2.png','walk10.png']);
  assert.deepEqual(naturalOrder(files, true).map(s => s.name), ['walk10.png','walk2.png','walk1.png']);
  assert.equal(files[0].name, 'walk10.png');
  const ties = [source('walk2.png'), source('Walk2.png')];
  assert.equal(naturalOrder(ties)[0], ties[0]);
});

test('differently sized frames share one fit scale and smaller frames stay smaller', () => {
  const frames = [rect(256,128), rect(64,64)];
  const plan = normalizationPlan(frames);
  assert.equal(plan.scale, .5);
  assert.deepEqual(plan.placements.map(p => [p.width,p.height,p.x,p.y]), [[128,64,0,64],[32,32,48,96]]);
  assert.equal(plan.clippedCount, 0);
  assert.deepEqual(frames, [rect(256,128),rect(64,64)]);
});

test('fit never enlarges a small source and original pixels align at a common bottom anchor', () => {
  const fitted = normalizationPlan([rect(16,32),rect(32,16)]);
  assert.equal(fitted.scale, 1);
  const original = normalizationPlan([rect(3,5),rect(20,10)], {mode:'original'});
  assert.deepEqual(original.placements.map(p => [p.x,p.y,p.width,p.height]), [[63,123,3,5],[54,118,20,10]]);
});

test('custom shared scaling and destination/source anchors are explicit and report clipping', () => {
  const centered = normalizationPlan([rect(16,32)], {mode:'custom', scale:2, anchorX:64, anchorY:64, sourceAnchor:'center'});
  assert.deepEqual([centered.placements[0].x,centered.placements[0].y], [48,32]);
  const original = normalizationPlan([rect(256,128)], {mode:'original'});
  assert.equal(original.clippedCount, 1);
  const topLeft = normalizationPlan([rect(20,20)], {mode:'original',anchorX:0,anchorY:0,sourceAnchor:'top-left'});
  assert.deepEqual([topLeft.placements[0].x,topLeft.placements[0].y], [0,0]);
});

test('sheet cells honor offsets and gaps without including incomplete edge cells', () => {
  const plan = gridFrames({width:40,height:22,cellWidth:10,cellHeight:8,offsetX:2,offsetY:3,gapX:3,gapY:2});
  assert.deepEqual([plan.columns,plan.rows,plan.total],[3,2,6]);
  assert.deepEqual(plan.frames.map(f => [f.x,f.y]), [[2,3],[15,3],[28,3],[2,13],[15,13],[28,13]]);
  assert.ok(plan.frames.every(f => f.x + f.width <= 40 && f.y + f.height <= 22));
  const partial = gridFrames({width:35,height:16,cellWidth:16,cellHeight:16});
  assert.equal(partial.frames.length, 2);
});

test('column-major sheet order and frame count select the intended cells', () => {
  const plan = gridFrames({width:40,height:22,cellWidth:10,cellHeight:8,offsetX:2,offsetY:3,gapX:3,gapY:2,order:'columns',count:4});
  assert.deepEqual(plan.frames.map(f => [f.x,f.y]), [[2,3],[2,13],[15,3],[15,13]]);
  const hugeGrid = gridFrames({width:4096,height:4096,cellWidth:1,cellHeight:1,count:3});
  assert.equal(hugeGrid.total, 16_777_216);
  assert.equal(hugeGrid.frames.length, 3);
});

test('bad geometry, excessive frame count and invalid scale fail before allocation', () => {
  for (const args of [
    {width:128,height:128,cellWidth:0,cellHeight:16},
    {width:128,height:128,cellWidth:129,cellHeight:16},
    {width:128,height:128,cellWidth:16,cellHeight:16,offsetX:-1},
    {width:128,height:128,cellWidth:16,cellHeight:16,gapX:.5},
    {width:128,height:128,cellWidth:16,cellHeight:16,count:65},
    {width:4096,height:4096,cellWidth:1,cellHeight:1},
  ]) assert.throws(() => gridFrames(args));
  for (const options of [{mode:'custom',scale:0},{mode:'custom',scale:Infinity},{anchorX:-1},{sourceAnchor:'unknown'}]) assert.throws(() => normalizationPlan([rect(16,16)],options));
});

test('PNG header admission validates signature, IHDR and bounded nonzero dimensions', () => {
  const bytes = new Uint8Array(24), d = new DataView(bytes.buffer);
  d.setUint32(0,0x89504e47); d.setUint32(4,0x0d0a1a0a); d.setUint32(8,13); d.setUint32(12,0x49484452); d.setUint32(16,320); d.setUint32(20,128);
  assert.deepEqual(pngSize(bytes), {width:320,height:128});
  d.setUint32(16,0); assert.throws(() => pngSize(bytes));
  d.setUint32(16,5000); assert.throws(() => pngSize(bytes));
  assert.throws(() => pngSize(new Uint8Array(10)));
});

test('source byte and decoded pixel budgets reject large batches before bitmap decoding', () => {
  assert.deepEqual(validateSources([source('a.png',128,128)]), {pixels:16_384,bytes:1000});
  assert.throws(() => validateSources([source('a.png',4096,4096),source('b.png',1,1)]));
  assert.throws(() => validateSources([source('a.png',1,1,SEQUENCE_LIMITS.fileBytes+1)]));
  assert.throws(() => validateSources(Array.from({length:5},(_,i)=>source(`${i}.png`,1,1,8*1024*1024))));
  assert.throws(() => validateSources([source('a.jpg')]));
});

test('capacity checks preserve multipart imports and permit a separate new frame action', () => {
  assert.doesNotThrow(() => checkImportCapacity({kind:undefined,assetCount:250,frameCount:250},6,'append'));
  assert.throws(() => checkImportCapacity({kind:'frames',assetCount:1,frameCount:256},1,'append'));
  assert.doesNotThrow(() => checkImportCapacity({kind:'parts',assetCount:10,frameCount:256,partCount:30},2,'append'));
  assert.throws(() => checkImportCapacity({kind:'parts',assetCount:10,frameCount:1,partCount:32},1,'append'));
  assert.doesNotThrow(() => checkImportCapacity({kind:'parts',assetCount:10,partCount:32},4,'new'));
  assert.throws(() => checkImportCapacity({kind:'frames',assetCount:256,frameCount:1},1,'new'));
});

test('slices land on the normalized canvas and keep the first pivot', () => {
  const placement = {x: 32, y: 64, width: 64, height: 64, source: {width: 64, height: 64}};
  const placed = placeSlices([
    {name: '头', x: 0, y: 0, width: 16, height: 8, pivotX: 8, pivotY: 8},
    {name: '越界', x: 80, y: 50, width: 30, height: 30},
  ], placement);
  assert.deepEqual(placed.regions[0], {name: '头', x: 32, y: 64, width: 16, height: 8});
  assert.deepEqual(placed.pivot, {x: 40, y: 72});
  assert.equal(placed.regions[1].x + placed.regions[1].width, 128);
  assert.equal(placed.regions[1].y + placed.regions[1].height, 128);
});

test('preview preserves paint-time remainder at 60Hz instead of slowing 40ms frames', async () => {
  const {sequenceClock} = await import('./sequence-model.mjs');
  let lastTime = null, steps = 0;
  for (let paint = 0; paint <= 60; paint++) {
    const advance = sequenceClock(lastTime, paint * 1000 / 60, 40);
    lastTime = advance.lastTime; steps += advance.steps;
  }
  assert.equal(steps, 25);
  assert.equal(lastTime, 1000);
  assert.deepEqual(sequenceClock(1000, 1125, 40), {steps:3,lastTime:1120});
});
