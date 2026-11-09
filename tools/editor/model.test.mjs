import test from 'node:test';
import assert from 'node:assert/strict';
import {Preview, HoverTracker, frameAt, hitRegion, validateProject, upgradeProject, composeParts, paintParts, partPose, setPartKey, poseOf, removePart, totalTicks} from './model.mjs';

const frame = (id, ticks = 2) => ({id, assetId: 'shime1', ticks, x: 0, y: 0});
function project() {
  return {
    version: 1, name: 'Test', defaultActionId: 'idle',
    assets: [{id: 'shime1', name: 'shime1.png', src: '/assets/shime1.png'}],
    actions: [
      {id: 'idle', name: 'Idle', loop: true, frames: [frame('i', 250)]},
      {id: 'happy', name: 'Happy', loop: true, frames: [frame('h1'), frame('h2', 3)]},
      {id: 'sleep', name: 'Sleep', loop: false, frames: [frame('s', 10)]},
    ],
    regions: [{id: 'head', name: 'Head', x: 20, y: 20, width: 50, height: 50}],
    rules: [{id: 'touch', name: 'Touch', event: 'click', regionId: 'head', actionId: 'happy',
      fromActionId: '*', cooldown: 10, priority: 5, enabled: true, returnToDefault: true}],
  };
}

test('timeline holds a pose for exactly its tick duration, including wrap', () => {
  const action = project().actions[1];
  assert.equal(frameAt(action, 0).index, 0);
  assert.equal(frameAt(action, 1).index, 0);
  assert.equal(frameAt(action, 2).index, 1);
  assert.equal(frameAt(action, 4).index, 1);
  assert.equal(frameAt(action, 5).index, 0);
});

test('touch regions use topmost overlap, with right and bottom edges excluded', () => {
  const regions = [...project().regions, {id: 'front', x: 30, y: 30, width: 10, height: 10}];
  assert.equal(hitRegion(regions, 30, 30), 'front');
  assert.equal(hitRegion(regions, 40, 40), 'head');
  assert.equal(hitRegion(regions, 70, 70), '*');
});

test('whole-canvas entry without named regions fires only on entry and respects rule cooldown', () => {
  const p = project();
  p.regions = [];
  p.rules[0] = {...p.rules[0], event: 'enter', regionId: '*'};
  const player = new Preview(p), hover = new HoverTracker();
  const move = (x, y) => {
    const sample = hover.move(p.regions, x, y);
    return sample.entered ? player.dispatch('enter', sample.region) : null;
  };
  assert.equal(move(-1, 30), null);
  assert.equal(move(0, 30)?.id, 'touch');
  assert.equal(move(40, 30), null);
  assert.equal(move(128, 30), null);
  assert.equal(move(20, 30), null, 're-entry during cooldown');
  player.step(10);
  assert.equal(move(30, 30), null, 'no entry while already inside');
  hover.reset();
  assert.equal(move(30, 30)?.id, 'touch', 're-entry after leave and cooldown');
});

test('a loop can be interrupted for one cycle then return at the precise boundary', () => {
  const player = new Preview(project());
  assert.equal(player.dispatch('click', 'head').id, 'touch');
  player.step(4);
  assert.equal(player.actionId, 'happy');
  player.step();
  assert.equal(player.actionId, 'idle');
  assert.equal(player.elapsed, 0);
});

test('cooldown blocks repeat events until its exact tick boundary', () => {
  const player = new Preview(project());
  player.dispatch('click', 'head');
  player.step(9);
  assert.equal(player.dispatch('click', 'head'), null);
  player.step();
  assert.equal(player.dispatch('click', 'head').id, 'touch');
});

test('priority, condition and enabled gates choose the right rule without consuming skipped cooldowns', () => {
  const p = project();
  p.rules.push({...p.rules[0], id: 'high', priority: 20, event: 'pet'});
  p.rules.push({...p.rules[0], id: 'disabled', priority: 99, enabled: false});
  const player = new Preview(p);
  assert.equal(player.dispatch('pet', 'head').id, 'high');
  assert.equal(player.dispatch('click', 'head'), null);
  player.step(5);
  assert.equal(player.dispatch('click', 'head').id, 'touch');
  const conditioned = new Preview({...p, rules: [{...p.rules[0], fromActionId: 'sleep'}]});
  assert.equal(conditioned.dispatch('click', 'head'), null);
  conditioned.play('sleep');
  assert.equal(conditioned.dispatch('click', 'head').id, 'touch');
});

test('idle fires once after 125 ticks and pointer activity restarts its countdown', () => {
  const p = project();
  p.rules = [{...p.rules[0], event: 'idle', regionId: '*', actionId: 'sleep'}];
  const player = new Preview(p);
  player.step(100);
  player.input();
  player.step(124);
  assert.equal(player.actionId, 'idle');
  player.step();
  assert.equal(player.actionId, 'sleep');
  player.step(150);
  assert.equal(player.actionId, 'idle');
  assert.equal(player.logs.filter(log => log.message.startsWith('Touch →')).length, 1);
});

test('editing a non-looping clip holds its last pose without firing rules or returning to another clip', () => {
  const player = new Preview(project());
  player.play('sleep');
  player.step(200, false);
  assert.equal(player.actionId, 'sleep');
  assert.equal(player.elapsed, 9);
  assert.equal(player.idleSent, false);
});

test('project validation rejects broken references, unsafe image URLs and invalid tick values', () => {
  assert.doesNotThrow(() => validateProject(project()));
  for (const mutate of [
    p => { p.defaultActionId = 'missing'; },
    p => { p.actions[0].frames[0].assetId = 'missing'; },
    p => { p.actions[0].frames[0].ticks = 0; },
    p => { p.actions[0].frames[0].ticks = 2.5; },
    p => { p.actions[0].frames = []; },
    p => { p.rules[0].regionId = 'missing'; },
    p => { p.regions[0].width = 128; },
    p => { p.assets[0].src = 'https://example.com/tracker.png'; },
    p => { p.actions.push({...p.actions[0]}); },
    p => { p.rules[0].name = '   '; },
    p => { p.regions[0].name = 'x'.repeat(81); },
  ]) {
    const p = project(); mutate(p); assert.throws(() => validateProject(p));
  }
});

const part = (id, parentId = null) => ({id, name: id, assetId: 'shime1', parentId, x: 64, y: 128, pivotX: 64, pivotY: 128, rotation: 0, scaleX: 1, scaleY: 1, visible: true});
function multipart() {
  const p = upgradeProject(project());
  p.parts = [part('body'), {...part('hand', 'body'), x: 10, y: -40, pivotX: 8, pivotY: 12}];
  Object.assign(p.actions[1], {kind: 'parts', durationTicks: 20, tracks: []});
  return p;
}

test('v1 projects migrate without changing frame timing or source data', () => {
  const original = project(), next = upgradeProject(original);
  assert.equal(original.version, 1);
  assert.equal(next.version, 2);
  assert.deepEqual(next.parts, []);
  assert.equal(next.actions[0].kind, 'frames');
  assert.equal(totalTicks(next.actions[1]), 5);
});

test('parent rotation transforms child position and fixed pivots without changing painter order', () => {
  const p = multipart();
  p.parts[0].rotation = 90;
  // Parent may occur after child in painter order; graph evaluation stays correct.
  p.parts.reverse();
  const composed = composeParts(p, p.actions[1], 0);
  assert.equal(composed[0].id, 'hand');
  assert.ok(Math.abs(composed[0].matrix[4] - 104) < 1e-9);
  assert.ok(Math.abs(composed[0].matrix[5] - 138) < 1e-9);
  const image = composed[0].imageMatrix;
  assert.ok(Math.abs((image[0]*8 + image[2]*12 + image[4]) - 104) < 1e-9);
  assert.ok(Math.abs((image[1]*8 + image[3]*12 + image[5]) - 138) < 1e-9);
});

test('opacity fades across keys while draw order stays on the earlier key', () => {
  const p = multipart(), a = p.actions[1], hand = p.parts[1];
  setPartKey(a, hand.id, 0, {...poseOf(hand), opacity: 1, zOrder: 0});
  setPartKey(a, hand.id, 20, {...poseOf(hand), opacity: 0, zOrder: 4});
  assert.equal(partPose(hand, a, 10).opacity, 0.5);
  assert.equal(partPose(hand, a, 10).zOrder, 0);
  assert.equal(partPose(hand, a, 20).zOrder, 4);
  hand.zOrder = -1;
  assert.equal(paintParts(composeParts(p, a, 0))[0].id, 'body');
  assert.equal(paintParts(composeParts(p, a, 0)).at(-1).id, 'hand');
  p.parts[1].zOrder = -2;
  assert.equal(paintParts(composeParts(p, p.actions[0], 0))[0].id, 'hand');
});

test('numeric keys interpolate while image and visibility switch at their key tick', () => {
  const p = multipart(), a = p.actions[1], hand = p.parts[1];
  setPartKey(a, hand.id, 0, poseOf(hand));
  setPartKey(a, hand.id, 20, {...poseOf(hand), x: 30, rotation: 90, visible: false});
  assert.equal(partPose(hand, a, 10).x, 20);
  assert.equal(partPose(hand, a, 10).rotation, 45);
  assert.equal(partPose(hand, a, 19).visible, true);
  assert.equal(partPose(hand, a, 20).visible, false);
  assert.equal(composeParts(p, a, 20)[1].visible, false);
});

test('hidden parent hides descendants and deleting a part repairs all graph and track references', () => {
  const p = multipart(), a = p.actions[1];
  p.parts[0].visible = false;
  setPartKey(a, 'body', 0, p.parts[0]);
  assert.equal(composeParts(p, a, 0)[1].visible, false);
  removePart(p, 'body');
  assert.equal(p.parts[0].parentId, null);
  assert.deepEqual(a.tracks, []);
  assert.doesNotThrow(() => validateProject(p));
});

test('keyframe upsert replaces one tick and sorts the remaining timeline', () => {
  const p = multipart(), a = p.actions[1], pose = p.parts[1];
  setPartKey(a, 'hand', 20, pose);
  setPartKey(a, 'hand', 0, pose);
  setPartKey(a, 'hand', 20, {...pose, rotation: -30});
  assert.deepEqual(a.tracks[0].keys.map(k => k.tick), [0,20]);
  assert.equal(a.tracks[0].keys[1].rotation, -30);
  assert.doesNotThrow(() => validateProject(p));
});

test('invalid part graphs, unsorted keys and export budgets are rejected', () => {
  for (const mutate of [
    p => { p.parts[0].parentId = 'hand'; },
    p => { p.parts[0].parentId = 'missing'; },
    p => { p.parts[0].scaleX = 0; },
    p => { p.actions[1].tracks = [{partId:'missing', keys:[{tick:0,...poseOf(p.parts[0])}]}]; },
    p => { p.actions[1].tracks = [{partId:'body', keys:[{tick:10,...poseOf(p.parts[0])},{tick:0,...poseOf(p.parts[0])}]}]; },
    p => { p.actions[1].durationTicks = 1001; },
    p => { for (const a of p.actions) Object.assign(a, {kind:'parts',durationTicks:1000,tracks:[]}); },
  ]) {
    const p = multipart(); mutate(p); assert.throws(() => validateProject(p));
  }
});

test('multipart duration determines preview return boundary rather than fallback frame length', () => {
  const p = multipart(), player = new Preview(p);
  player.dispatch('click', 'head');
  player.step(19);
  assert.equal(player.actionId, 'happy');
  player.step();
  assert.equal(player.actionId, 'idle');
});
