import test from 'node:test';
import assert from 'node:assert/strict';
import {applyFrameOperation, copySelectedFrames, framesForAction, selectFrameRange} from './timeline-ops.mjs';

const frame = (id, ticks = 5) => ({id, assetId: `asset-${id}`, ticks, x: 0, y: 0, extra: {name: id}});
const frames = (ids = ['a', 'b', 'c', 'd', 'e']) => ids.map(id => frame(id));
const ids = list => list.map(item => item.id);
const factory = () => { let index = 0; return () => `new-${++index}`; };
const total = list => list.reduce((sum, item) => sum + item.ticks, 0);

test('copy and ranges use stable IDs and timeline order, independently of input selection order', () => {
  const input = frames();
  const copied = copySelectedFrames(input, new Set(['e', 'b', 'd']));
  assert.deepEqual(ids(copied), ['b', 'd', 'e']);
  copied[0].extra.name = 'changed';
  assert.equal(input[1].extra.name, 'b');
  assert.deepEqual(selectFrameRange(input, 'd', 'b'), ['b', 'c', 'd']);
  assert.deepEqual(copySelectedFrames(input, []), []);
  assert.throws(() => selectFrameRange(input, 'gone', 'b'));
});

test('duplicate inserts selected frames in order after the last selected frame', () => {
  const input = frames();
  const result = applyFrameOperation(input, ['d', 'b'], 'duplicate', {idFactory: factory()});
  assert.deepEqual(ids(result.frames), ['a', 'b', 'c', 'd', 'new-1', 'new-2', 'e']);
  assert.deepEqual(result.frames.slice(4, 6).map(f => f.assetId), ['asset-b', 'asset-d']);
  assert.deepEqual(result.selectedIds, ['new-1', 'new-2']);
  assert.equal(result.focusId, 'new-1');
});

test('paste gives the clipboard fresh identities and supports append with no selection', () => {
  const input = frames(['a', 'b']);
  const clipboard = copySelectedFrames(frames(['c', 'd']), ['d', 'c']);
  const result = applyFrameOperation(input, ['a'], 'paste', {clipboard, idFactory: factory()});
  assert.deepEqual(ids(result.frames), ['a', 'new-1', 'new-2', 'b']);
  assert.deepEqual(ids(clipboard), ['c', 'd']);
  const appended = applyFrameOperation(input, [], 'paste', {clipboard, idFactory: factory()});
  assert.deepEqual(ids(appended.frames), ['a', 'b', 'new-1', 'new-2']);
  assert.deepEqual(appended.selectedIds, ['new-1', 'new-2']);
});

test('non-contiguous reverse fills selected slots and preserves every unselected frame', () => {
  const input = frames();
  const result = applyFrameOperation(input, ['e', 'a', 'c'], 'reverse');
  assert.deepEqual(ids(result.frames), ['e', 'b', 'c', 'd', 'a']);
  assert.deepEqual(result.selectedIds, ['e', 'c', 'a']);
  assert.equal(result.frames[1].extra.name, 'b');
  assert.equal(result.focusId, 'e');
});

test('ping-pong adds only reversed interior poses and leaves surrounding frames in place', () => {
  const input = frames();
  const result = applyFrameOperation(input, ['b', 'c', 'd'], 'pingpong', {idFactory: factory()});
  assert.deepEqual(result.frames.map(f => f.assetId), ['asset-a', 'asset-b', 'asset-c', 'asset-d', 'asset-c', 'asset-e']);
  assert.deepEqual(result.selectedIds, ['b', 'c', 'd', 'new-1']);
  for (const selected of [['b'], ['b', 'c']]) {
    assert.deepEqual(ids(applyFrameOperation(input, selected, 'pingpong').frames), ids(input));
  }
  assert.throws(() => applyFrameOperation(input, ['a', 'c'], 'pingpong', {idFactory: factory()}), /连续/);
});

test('uniform duration applies only to selected frames and checks tick bounds', () => {
  const input = frames(['a', 'b', 'c']);
  const result = applyFrameOperation(input, ['c', 'a'], 'duration', {ticks: 12});
  assert.deepEqual(result.frames.map(f => f.ticks), [12, 5, 12]);
  for (const ticks of [0, 1501, 1.5, NaN, Infinity, '5']) {
    assert.throws(() => applyFrameOperation(input, ['a'], 'duration', {ticks}));
  }
});

test('speed preserves exact rounded aggregate duration instead of rounding each frame independently', () => {
  const input = frames(['a', 'b', 'c']);
  const result = applyFrameOperation(input, ['a', 'b', 'c'], 'speed', {factor: 2});
  assert.deepEqual(result.frames.map(f => f.ticks), [3, 2, 3]);
  assert.equal(total(result.frames), Math.round(total(input) / 2));
  const mixed = [frame('a', 2), frame('b', 17), frame('c', 3), frame('d', 31)];
  const partial = applyFrameOperation(mixed, ['a', 'c', 'd'], 'speed', {factor: 1.7});
  assert.equal(partial.frames[1].ticks, 17);
  assert.equal(total(copySelectedFrames(partial.frames, partial.selectedIds)), Math.round(36 / 1.7));
});

test('bounded speed allocation keeps all ticks feasible at both limits', () => {
  const input = [frame('a', 1), frame('b', 1), frame('c', 100)];
  const faster = applyFrameOperation(input, ids(input), 'speed', {factor: 3});
  assert.deepEqual(faster.frames.map(f => f.ticks), [1, 1, 32]);
  const slower = applyFrameOperation([frame('a', 1500), frame('b', 1)], ['a', 'b'], 'speed', {factor: .6});
  assert.deepEqual(slower.frames.map(f => f.ticks), [1500, 1002]);
  for (const weights of [[1], [1, 1], [1, 1500], [7, 19, 3, 21], [1499, 1500, 1500]]) {
    const input = weights.map((ticks, index) => frame(`f${index}`, ticks));
    for (const factor of [.5, .9, 1, 1.5, 2, 10]) {
      const target = Math.round(total(input) / factor);
      if (target < input.length || target > input.length * 1500) continue;
      const result = applyFrameOperation(input, ids(input), 'speed', {factor});
      assert.equal(total(result.frames), target);
      assert.ok(result.frames.every(f => Number.isInteger(f.ticks) && f.ticks >= 1 && f.ticks <= 1500));
    }
  }
});

test('impossible or invalid speed operations fail without modifying any source frame', () => {
  const input = [frame('a', 1), frame('b', 1)];
  const original = structuredClone(input);
  for (const factor of [0, -1, NaN, Infinity, 2, 1e-300, '2']) {
    assert.throws(() => applyFrameOperation(input, ['a', 'b'], 'speed', {factor}));
    assert.deepEqual(input, original);
  }
});

test('deletion chooses a surviving neighbor and cannot delete the whole action', () => {
  const input = frames(['a', 'b', 'c', 'd']);
  const result = applyFrameOperation(input, ['b', 'd'], 'delete');
  assert.deepEqual(ids(result.frames), ['a', 'c']);
  assert.deepEqual(result.selectedIds, ['c']);
  assert.equal(result.focusId, 'c');
  assert.throws(() => applyFrameOperation(input, ids(input), 'delete'), /至少/);
});

test('frame cap and ID failures are atomic, including clipboard IDs and factory collisions', () => {
  const full = Array.from({length: 256}, (_, index) => frame(`f${index}`));
  let generated = 0;
  assert.throws(() => applyFrameOperation(full, ['f0'], 'duplicate', {idFactory: () => { generated++; return 'new'; }}), /最多/);
  assert.equal(generated, 0);
  const input = frames(['a', 'b', 'c']);
  const original = structuredClone(input);
  const clipboard = [frame('clipboard')];
  for (const idFactory of [() => 'a', () => '../bad', () => '']) {
    assert.throws(() => applyFrameOperation(input, ['a'], 'duplicate', {idFactory}));
  }
  assert.throws(() => applyFrameOperation(input, ['a', 'b'], 'duplicate', {idFactory: () => 'same-new'}));
  assert.throws(() => applyFrameOperation(input, ['a'], 'paste', {clipboard, idFactory: () => 'clipboard'}));
  assert.throws(() => applyFrameOperation(input, ['a'], 'paste', {clipboard: [], idFactory: factory()}));
  assert.deepEqual(input, original);
  assert.deepEqual(clipboard, [frame('clipboard')]);
});

test('frozen inputs work for every operation, and each returned object is independent', () => {
  const input = frames(['a', 'b', 'c']);
  input.forEach(f => { Object.freeze(f.extra); Object.freeze(f); }); Object.freeze(input);
  const selectedIds = Object.freeze(['a', 'b']);
  for (const operation of ['duplicate', 'paste', 'reverse', 'pingpong', 'duration', 'speed', 'delete']) {
    const result = applyFrameOperation(input, selectedIds, operation, {idFactory: factory(), clipboard: [frame('clip')], ticks: 8, factor: 1.2});
    result.frames[0].extra.name = 'changed';
    assert.equal(input[0].extra.name, 'a');
    assert.deepEqual(selectedIds, ['a', 'b']);
  }
});

test('a contiguous selection becomes a new action in forward, reverse or ping-pong order', () => {
  const input = frames(['a', 'b', 'c', 'd']);
  assert.deepEqual(framesForAction(input, ['c', 'a', 'b'], 'forward').map(frame => frame.assetId), ['asset-a', 'asset-b', 'asset-c']);
  assert.equal(framesForAction(input, ['a', 'b', 'c'], 'reverse')[0].assetId, 'asset-c');
  assert.deepEqual(framesForAction(input, ['a', 'b', 'c'], 'pingpong').map(frame => frame.assetId), ['asset-a', 'asset-b', 'asset-c', 'asset-b']);
  assert.equal('id' in framesForAction(input, ['a'], 'forward')[0], false);
  assert.throws(() => framesForAction(input, ['a', 'c'], 'forward'), /连续/);
  assert.deepEqual(ids(input), ['a', 'b', 'c', 'd']);
});

test('invalid source frames, stale selection and unknown operations never partially edit input', () => {
  const input = frames();
  assert.throws(() => applyFrameOperation(input, ['missing'], 'reverse'));
  assert.throws(() => applyFrameOperation(input, [], 'reverse'));
  assert.throws(() => applyFrameOperation(input, ['a'], 'unknown'));
  assert.throws(() => copySelectedFrames([frame('a'), frame('a')], ['a']));
  assert.throws(() => applyFrameOperation([frame('a', 0)], ['a'], 'reverse'));
  assert.deepEqual(ids(input), ['a', 'b', 'c', 'd', 'e']);
});
