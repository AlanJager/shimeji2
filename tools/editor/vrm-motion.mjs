// Presets rotate normalized humanoid joints; they never stretch bones.
export const VRM_TICK_SECONDS = .04;

export function presetPose(preset, seconds, height = 1) {
  if (!['idle', 'breathe', 'wave', 'dance'].includes(preset) || !Number.isFinite(seconds) || !Number.isFinite(height) || height <= 0) throw new Error('无效的预设动作参数');
  const phase = seconds * Math.PI;
  const sway = Math.sin(phase), opposite = Math.cos(phase);
  const rotations = {
    leftUpperArm: [0, 0, -1.22], rightUpperArm: [0, 0, 1.22],
    leftLowerArm: [0, -.08, -.08], rightLowerArm: [0, .08, .08],
    head: [0, .018 * sway, .012 * opposite],
  };
  const hips = [0, 0, 0];
  if (preset === 'breathe') {
    rotations.spine = [.025 * sway, 0, 0];
    rotations.chest = [-.018 * sway, 0, 0];
    rotations.leftUpperArm[2] -= .025 * sway;
    rotations.rightUpperArm[2] += .025 * sway;
    hips[1] = height * .003 * sway;
  } else if (preset === 'wave') {
    rotations.rightUpperArm = [0, -.2, -.65];
    rotations.rightLowerArm = [-.1, 0, -1.05];
    rotations.rightHand = [0, .25 * Math.sin(phase * 3), .22 * Math.sin(phase * 3)];
    rotations.head = [0, -.08, -.06];
  } else if (preset === 'dance') {
    rotations.hips = [0, .12 * sway, .06 * sway];
    rotations.spine = [0, -.08 * sway, -.08 * sway];
    rotations.leftUpperArm = [.14 * opposite, 0, -.95 + .18 * sway];
    rotations.rightUpperArm = [-.14 * opposite, 0, .95 + .18 * sway];
    rotations.leftLowerArm = [0, -.18, -.35];
    rotations.rightLowerArm = [0, .18, .35];
    rotations.leftUpperLeg = [.10 * opposite, 0, -.025];
    rotations.rightUpperLeg = [-.10 * opposite, 0, .025];
    rotations.leftLowerLeg = [.10 * (1 - opposite), 0, 0];
    rotations.rightLowerLeg = [.10 * (1 + opposite), 0, 0];
    hips[1] = height * .008 * Math.cos(phase * 2);
  }
  return {rotations, hips};
}

export function frameAlphaStats(rgba, width = 128, height = 128) {
  if (!(rgba instanceof Uint8Array || rgba instanceof Uint8ClampedArray) || rgba.length !== width * height * 4) throw new Error('无效的帧像素');
  let visiblePixels = 0, transparentPixels = 0, edgePixels = 0;
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const alpha = rgba[(y * width + x) * 4 + 3];
    if (alpha < 255) transparentPixels++;
    if (alpha > 8) { visiblePixels++; if (x === 0 || y === 0 || x === width - 1 || y === height - 1) edgePixels++; }
  }
  return {visiblePixels, transparentPixels, edgePixels, clipped: edgePixels > 0};
}
