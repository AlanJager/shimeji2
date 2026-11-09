import test from 'node:test';
import assert from 'node:assert/strict';
import {presetPose, frameAlphaStats, VRM_TICK_SECONDS} from './vrm-motion.mjs';

test('presets are deterministic rotations and translations with natural resting arms', () => {
  assert.equal(VRM_TICK_SECONDS, .04);
  for (const preset of ['idle', 'breathe', 'wave', 'dance']) {
    const pose = presetPose(preset, 1.28, 1.7);
    assert.deepEqual(pose, presetPose(preset, 32 * VRM_TICK_SECONDS, 1.7));
    assert.deepEqual(Object.keys(pose).sort(), ['hips', 'rotations']);
    assert.ok([...Object.values(pose.rotations).flat(), ...pose.hips].every(Number.isFinite));
    assert.ok(Object.values(pose.rotations).every(rotation => rotation.length === 3));
  }
  assert.ok(presetPose('idle', 0).rotations.leftUpperArm[2] < -1);
  assert.ok(presetPose('idle', 0).rotations.rightUpperArm[2] > 1);
  assert.throws(() => presetPose('unknown', 0));
  assert.throws(() => presetPose('idle', Infinity));
});

test('breathing is periodic without changing bone scales or lengths', () => {
  const a = presetPose('breathe', .4, 2), b = presetPose('breathe', 2.4, 2);
  for (const key of Object.keys(a.rotations)) a.rotations[key].forEach((value, index) => assert.ok(Math.abs(value - b.rotations[key][index]) < 1e-12));
  assert.ok(Math.abs(a.hips[1] - b.hips[1]) < 1e-12);
});

test('alpha admission distinguishes blank, opaque and clipped frames', () => {
  const pixels = new Uint8Array(128 * 128 * 4);
  assert.deepEqual(frameAlphaStats(pixels), {visiblePixels:0,transparentPixels:16384,edgePixels:0,clipped:false});
  pixels[(64 * 128 + 64) * 4 + 3] = 255;
  assert.equal(frameAlphaStats(pixels).visiblePixels, 1);
  assert.equal(frameAlphaStats(pixels).clipped, false);
  pixels[3] = 255;
  assert.equal(frameAlphaStats(pixels).edgePixels, 1);
  pixels.fill(255);
  assert.equal(frameAlphaStats(pixels).transparentPixels, 0);
  assert.throws(() => frameAlphaStats(new Uint8Array(2)));
});
