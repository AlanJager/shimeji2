// Controller state tests with a minimal DOM adapter. These cover event wiring,
// not browser layout/hit testing; the real browser acceptance covers that layer.
import test from 'node:test';
import assert from 'node:assert/strict';
import {setupFrameTimeline} from './authoring-timeline.mjs';

const frame = (id, ticks = 2) => ({id, assetId: `asset-${id}`, ticks, x: 0, y: 0});
const event = values => ({prevented: false, preventDefault() { this.prevented = true; }, ...values});

function withTimeline(check, input = 'abcdef'.split('').map(id => frame(id))) {
  const originalDocument = globalThis.document;
  const elements = new Map(), overlays = [], cells = new Map();
  function element(id = '') {
    const listeners = new Map(), classes = new Set();
    return {_id: id, get id() { return this._id; }, set id(value) { this._id = value; elements.set(value, this); },
      style: {}, disabled: false, checked: false, hidden: false, _value: '',
      get value() { return this._value; }, set value(value) { this._value = String(value); },
      classList: {toggle(name, enabled) { if (enabled) classes.add(name); else classes.delete(name); }, contains: name => classes.has(name)},
      addEventListener(name, callback) { const list = listeners.get(name) ?? []; list.push(callback); listeners.set(name, list); },
      emit(name, input) { for (const callback of listeners.get(name) ?? []) callback(input); },
      remove() { this.removed = true; }, after() {}, setPointerCapture() {}, querySelectorAll() { return []; },
    };
  }
  const get = id => { if (!elements.has(id)) elements.set(id, element(id)); return elements.get(id); };
  globalThis.document = {getElementById: get, createElement: () => element(), querySelector: () => get('timeline-toolbar'),
    body: {append: overlay => overlays.push(overlay)}, activeElement: null};
  const state = {action: {id: 'walk', kind: 'frames', frames: structuredClone(input)}, selectedFrame: 0, mode: 'edit'};
  const notices = [], edits = [];
  let generated = 0, plays = 0, offset = 0, timeline;
  timeline = setupFrameTimeline({getState: () => state, toast: message => notices.push(message), idFactory: () => `generated-${++generated}`,
    onSelect(index, sourceEvent) { timeline.select(index, sourceEvent); state.selectedFrame = index; timeline.sync(); },
    onSelectionChange() { timeline.sync(); }, onPlayRange() { plays++; },
    onFrames(result, operation) {
      timeline.setSelection(result.selectedIds);
      state.selectedFrame = result.frames.findIndex(item => item.id === result.focusId);
      state.action.frames = result.frames;
      edits.push({operation, result}); timeline.sync();
    },
  });
  const scroll = get('timeline-scroll');
  scroll.querySelectorAll = () => state.action.frames.map((item, index) => {
    if (!cells.has(item.id)) cells.set(item.id, element(item.id));
    const cell = cells.get(item.id); cell.dataset = {frame: String(index)};
    cell.getBoundingClientRect = () => ({left: index * 100 - offset, right: index * 100 + 90 - offset, top: 0, bottom: 40});
    return cell;
  });
  function select(index, sourceEvent = {}) { timeline.select(index, sourceEvent); state.selectedFrame = index; timeline.sync(); }
  function marquee(from, to, modifiers = {}, cancelled = false) {
    scroll.emit('pointerdown', event({button: 0, target: {closest: () => null}, pointerId: 1, clientX: from[0], clientY: from[1], ...modifiers}));
    scroll.emit('pointermove', event({clientX: to[0], clientY: to[1]}));
    scroll.emit(cancelled ? 'pointercancel' : 'pointerup', event({clientX: to[0], clientY: to[1]}));
  }
  timeline.sync();
  try { return check({state, timeline, select, marquee, get, notices, edits, overlays, scroll,
    setOffset(value) { offset = value; scroll.emit('scroll', event({})); }, plays: () => plays}); }
  finally { if (originalDocument === undefined) delete globalThis.document; else globalThis.document = originalDocument; }
}

test('marquee establishes its focused anchor before Shift extends the new selection', () => withTimeline(({timeline, state, marquee, select}) => {
  assert.deepEqual(timeline.selection(), ['a']);
  marquee([200, 60], [490, 0]);
  assert.deepEqual(timeline.selection(), ['c', 'd', 'e']);
  assert.equal(state.action.frames[state.selectedFrame].id, 'c');
  select(5, {shiftKey: true});
  assert.deepEqual(timeline.selection(), ['c', 'd', 'e', 'f']);
}));

test('Ctrl and Meta toggles support noncontiguous selection; Shift can replace or extend it', () => withTimeline(({timeline, select}) => {
  select(1); select(3, {ctrlKey: true});
  assert.deepEqual(timeline.selection(), ['b', 'd']);
  select(5, {shiftKey: true, metaKey: true});
  assert.deepEqual(timeline.selection(), ['b', 'd', 'e', 'f']);
  select(4, {shiftKey: true});
  assert.deepEqual(timeline.selection(), ['d', 'e']);
  select(3, {metaKey: true});
  assert.deepEqual(timeline.selection(), ['e']);
}));

test('cancelled and empty marquee selections preserve selected IDs and remove their overlays', () => withTimeline(({timeline, marquee, select, overlays}) => {
  select(1); select(3, {ctrlKey: true});
  marquee([200, 60], [490, 0], {}, true);
  assert.deepEqual(timeline.selection(), ['b', 'd']);
  assert.equal(overlays.at(-1).removed, true);
  marquee([700, 70], [800, 80]);
  assert.deepEqual(timeline.selection(), ['b', 'd']);
  assert.ok(overlays.every(overlay => overlay.removed));
}));

test('scroll recomputes marquee hits at the stationary pointer and cancellation restores the original selection', () => withTimeline(({timeline, scroll, setOffset, overlays, select}) => {
  const down = () => scroll.emit('pointerdown', event({button: 0, target: {closest: () => null}, pointerId: 1, clientX: 10, clientY: 60}));
  const move = () => scroll.emit('pointermove', event({clientX: 80, clientY: 0}));
  down(); move(); assert.deepEqual(timeline.selection(), ['a']);
  setOffset(100); assert.deepEqual(timeline.selection(), ['b']);
  scroll.emit('pointerup', event({clientX: 80, clientY: 0}));
  assert.deepEqual(timeline.selection(), ['b']); assert.equal(overlays.at(-1).removed, true);
  select(3); down(); move(); assert.deepEqual(timeline.selection(), ['b']);
  scroll.emit('pointercancel', event({}));
  assert.deepEqual(timeline.selection(), ['d']); assert.equal(overlays.at(-1).removed, true);
}));

test('pointerup takes the final rectangle even without a preceding pointermove', () => withTimeline(({timeline, scroll}) => {
  scroll.emit('pointerdown', event({button: 0, target: {closest: () => null}, pointerId: 1, clientX: 200, clientY: 60}));
  scroll.emit('pointerup', event({clientX: 490, clientY: 0}));
  assert.deepEqual(timeline.selection(), ['c', 'd', 'e']);
}));

test('selection follows stable IDs through reordering, and action switches reset selection and loop range', () => withTimeline(({timeline, state, select, get}) => {
  select(1); select(2, {shiftKey: true});
  get('loop-selection').checked = true; get('loop-selection').onchange({target: get('loop-selection')});
  assert.deepEqual(timeline.playbackRange(), {start: 2, end: 6});
  state.action.frames.reverse(); timeline.sync();
  assert.deepEqual(timeline.selection(), ['c', 'b']);
  state.action = {id: 'other', kind: 'frames', frames: [frame('x'), frame('y')]}; state.selectedFrame = 0;
  timeline.sync();
  assert.deepEqual(timeline.selection(), ['x']);
  assert.equal(timeline.playbackRange(), null);
  assert.equal(get('loop-selection').checked, false);
}));

test('copy crosses actions in the same project and paste selects fresh frames after the target', () => withTimeline(({timeline, state, select, get, edits}) => {
  select(1); select(3, {ctrlKey: true}); get('bulk-copy').onclick();
  state.action = {id: 'other', kind: 'frames', frames: [frame('x'), frame('y')]}; state.selectedFrame = 0;
  timeline.sync(); assert.equal(get('bulk-paste').disabled, false);
  get('bulk-paste').onclick();
  assert.deepEqual(state.action.frames.map(item => item.id), ['x', 'generated-1', 'generated-2', 'y']);
  assert.deepEqual(state.action.frames.slice(1, 3).map(item => item.assetId), ['asset-b', 'asset-d']);
  assert.deepEqual(timeline.selection(), ['generated-1', 'generated-2']);
  assert.equal(state.selectedFrame, 1); assert.equal(edits.at(-1).operation, 'paste');
}));

test('project reset and missing source assets invalidate the clipboard before paste', () => withTimeline(({timeline, state, get}) => {
  get('bulk-copy').onclick(); assert.equal(get('bulk-paste').disabled, false);
  timeline.reset(true); timeline.sync(); assert.equal(get('bulk-paste').disabled, true);
  get('bulk-copy').onclick(); state.assetIds = ['unrelated-asset']; timeline.sync();
  assert.equal(get('bulk-paste').disabled, true);
}));

test('keyboard shortcuts consume frame editing commands and reject deleting all without mutation', () => withTimeline(({timeline, state, select, notices, edits}) => {
  select(1); const duplicate = event({key: 'd', metaKey: true});
  assert.equal(timeline.handleKey(duplicate), true); assert.equal(duplicate.prevented, true);
  assert.equal(edits.at(-1).operation, 'duplicate'); assert.deepEqual(timeline.selection(), ['generated-1']);
  const all = event({key: 'a', ctrlKey: true}); assert.equal(timeline.handleKey(all), true);
  assert.equal(timeline.selection().length, state.action.frames.length);
  const before = structuredClone(state.action.frames), count = edits.length, remove = event({key: 'Delete'});
  assert.equal(timeline.handleKey(remove), true); assert.equal(remove.prevented, true);
  assert.equal(edits.length, count); assert.deepEqual(state.action.frames, before);
  assert.match(notices.at(-1), /至少/);
  assert.equal(timeline.handleKey(event({key: 'q'})), false);
}));

test('frame loop ranges honor real tick lengths and are unavailable in interaction or part modes', () => withTimeline(({timeline, state, select, get, plays, edits}) => {
  state.action.frames[0].ticks = 7; state.action.frames[1].ticks = 3; state.action.frames[2].ticks = 11;
  select(1); select(2, {shiftKey: true});
  get('loop-selection').checked = true; get('loop-selection').onchange({target: get('loop-selection')});
  assert.equal(plays(), 1); assert.deepEqual(timeline.playbackRange(), {start: 7, end: 21});
  state.mode = 'test'; timeline.sync(); assert.equal(timeline.playbackRange(), null);
  assert.equal(get('loop-selection').disabled, true); assert.equal(get('loop-selection').checked, false);
  state.mode = 'edit'; timeline.sync(); assert.equal(timeline.playbackRange(), null);
  state.mode = 'edit'; state.action.kind = 'parts'; timeline.sync();
  assert.equal(get('bulk-timeline').hidden, true);
  assert.equal(timeline.playbackRange(), null);
  const before = edits.length;
  for (const key of ['a', 'c', 'v', 'd', 'Delete']) assert.equal(timeline.handleKey(event({key, ctrlKey: true})), false);
  timeline.run('duplicate'); assert.equal(edits.length, before);
}));

test('speed selection remains stable until Apply and its exact aggregate duration is committed once', () => withTimeline(({timeline, state, get, edits}) => {
  state.action.frames.forEach(item => { item.ticks = 5; });
  timeline.setSelection(['a', 'b', 'c']); timeline.sync();
  const before = structuredClone(state.action.frames);
  get('bulk-speed').value = '2';
  assert.deepEqual(state.action.frames, before); assert.equal(edits.length, 0);
  get('bulk-speed-apply').onclick();
  assert.equal(get('bulk-speed').value, '2'); assert.equal(edits.length, 1);
  assert.deepEqual(state.action.frames.slice(0, 3).map(item => item.ticks), [3, 2, 3]);
  assert.deepEqual(state.action.frames.slice(3).map(item => item.ticks), [5, 5, 5]);
  assert.deepEqual(timeline.selection(), ['a', 'b', 'c']);
}));

test('uniform duration aligns milliseconds to ticks only for the current selection', () => withTimeline(({state, select, get, notices, edits}) => {
  select(1); select(3, {ctrlKey: true});
  get('bulk-duration').value = '100'; get('bulk-duration-apply').onclick();
  assert.deepEqual(state.action.frames.map(item => item.ticks), [2, 3, 2, 3, 2, 2]);
  assert.match(notices.at(-1), /120 ms/); assert.equal(edits.length, 1);
  get('bulk-duration').value = '0'; get('bulk-duration-apply').onclick(); assert.equal(edits.length, 1);
}));
