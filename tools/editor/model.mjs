// Editor preview semantics. This is an authoring player, not the classic engine.
import {validateAssetGroups} from './asset-library-model.mjs';
export const TICK_MS = 40;
export const EVENTS = {click: '点击', pet: '抚摸', enter: '鼠标进入', drag: '开始拖拽', release: '松开拖拽', idle: '无操作 5 秒'};
export const clone = value => JSON.parse(JSON.stringify(value));
export const totalTicks = action => action.kind === 'parts' ? action.durationTicks : action.frames.reduce((sum, frame) => sum + frame.ticks, 0);
const idPattern = /^[a-zA-Z0-9_-]{1,64}$/;

export function upgradeProject(input) {
  const project = clone(input);
  validateProject(project);
  project.version = 2;
  project.parts ??= [];
  for (const action of project.actions) action.kind ??= 'frames';
  return project;
}

export const poseOf = part => Object.fromEntries(['x', 'y', 'rotation', 'scaleX', 'scaleY', 'visible', 'assetId'].map(key => [key, part[key]]));

export function partPose(part, action, tick) {
  const keys = action.tracks?.find(track => track.partId === part.id)?.keys;
  if (!keys?.length) return poseOf(part);
  if (tick <= keys[0].tick) return poseOf(keys[0]);
  if (tick >= keys.at(-1).tick) return poseOf(keys.at(-1));
  const index = keys.findIndex(key => key.tick > tick);
  const left = keys[index - 1], right = keys[index];
  const ratio = (tick - left.tick) / (right.tick - left.tick);
  const result = poseOf(left);
  for (const field of ['x', 'y', 'rotation', 'scaleX', 'scaleY']) result[field] = left[field] + (right[field] - left[field]) * ratio;
  return result;
}

export function multiply(a, b) {
  return [a[0]*b[0]+a[2]*b[1], a[1]*b[0]+a[3]*b[1], a[0]*b[2]+a[2]*b[3], a[1]*b[2]+a[3]*b[3], a[0]*b[4]+a[2]*b[5]+a[4], a[1]*b[4]+a[3]*b[5]+a[5]];
}

export function composeParts(project, action, tick) {
  const byId = new Map((project.parts ?? []).map(part => [part.id, part]));
  const resolved = new Map();
  function resolve(part) {
    if (resolved.has(part.id)) return resolved.get(part.id);
    const pose = partPose(part, action, tick), angle = pose.rotation * Math.PI / 180;
    const local = [Math.cos(angle)*pose.scaleX, Math.sin(angle)*pose.scaleX, -Math.sin(angle)*pose.scaleY, Math.cos(angle)*pose.scaleY, pose.x, pose.y];
    const parent = part.parentId ? resolve(byId.get(part.parentId)) : null;
    const matrix = parent ? multiply(parent.matrix, local) : local;
    const imageMatrix = multiply(matrix, [1, 0, 0, 1, -part.pivotX, -part.pivotY]);
    const result = {...part, ...pose, matrix, imageMatrix, visible: pose.visible && (parent?.visible ?? true)};
    resolved.set(part.id, result);
    return result;
  }
  return (project.parts ?? []).map(resolve);
}

export function setPartKey(action, partId, tick, pose) {
  action.tracks ??= [];
  let track = action.tracks.find(track => track.partId === partId);
  if (!track) { track = {partId, keys: []}; action.tracks.push(track); }
  track.keys = track.keys.filter(key => key.tick !== tick);
  track.keys.push({tick, ...poseOf(pose)});
  track.keys.sort((a, b) => a.tick - b.tick);
}

export function removePart(project, id) {
  // Detach direct children; preserve their authored local transforms explicitly.
  project.parts = project.parts.filter(part => part.id !== id);
  for (const part of project.parts) if (part.parentId === id) part.parentId = null;
  for (const action of project.actions) if (action.tracks) action.tracks = action.tracks.filter(track => track.partId !== id);
}

export function validateProject(project) {
  const fail = message => { throw new Error(message); };
  const integer = (n, lo, hi) => Number.isInteger(n) && n >= lo && n <= hi;
  const named = (value, max) => typeof value === 'string' && value.trim().length > 0 && [...value].length <= max;
  const list = (items, max, allowEmpty = false) => {
    if (!Array.isArray(items) || items.length > max || (!allowEmpty && !items.length)) fail('工程列表大小无效');
    const ids = new Set();
    for (const item of items) {
      if (!item || !idPattern.test(item.id) || ids.has(item.id)) fail('工程包含无效或重复 ID');
      ids.add(item.id);
    }
    return ids;
  };
  if (!project || ![1, 2].includes(project.version)) fail('不支持的工程版本');
  if (!named(project.name, 100)) fail('工程名称需要包含 1–100 个字符');
  const assets = list(project.assets, 256);
  for (const asset of project.assets) {
    if (typeof asset.name !== 'string' || typeof asset.src !== 'string' ||
        !(/^\/assets\/shime([1-9]|[1-3][0-9]|4[0-6])\.png$/.test(asset.src) ||
          /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(asset.src)) || asset.src.length > 2_800_000) fail('工程图片来源无效，仅支持内置素材或 PNG');
  }
  validateAssetGroups(project);
  const actions = list(project.actions, 64);
  const parts = list(project.parts ?? [], 32, true);
  const finite = (n, lo, hi) => typeof n === 'number' && Number.isFinite(n) && n >= lo && n <= hi;
  const poseValid = pose => finite(pose.x, -256, 256) && finite(pose.y, -256, 256) &&
    finite(pose.rotation, -3600, 3600) && finite(pose.scaleX, .05, 4) && finite(pose.scaleY, .05, 4) &&
    typeof pose.visible === 'boolean' && assets.has(pose.assetId);
  for (const part of project.parts ?? []) {
    if (!named(part.name, 80) || !poseValid(part) ||
        !finite(part.pivotX, -256, 256) || !finite(part.pivotY, -256, 256) ||
        (part.parentId !== null && !parts.has(part.parentId))) fail('部件参数或父级引用无效');
    const seen = new Set([part.id]);
    let parent = part.parentId;
    while (parent) {
      if (seen.has(parent)) fail('部件父子关系不能形成循环');
      seen.add(parent); parent = project.parts.find(p => p.id === parent).parentId;
    }
  }
  let bakedTicks = 0;
  if (!actions.has(project.defaultActionId)) fail('默认动作不存在');
  for (const action of project.actions) {
    if (!named(action.name, 80) || typeof action.loop !== 'boolean') fail('动作名称或循环选项无效');
    list(action.frames, 256);
    for (const frame of action.frames) {
      if (!assets.has(frame.assetId) || !integer(frame.ticks, 1, 1500) || !integer(frame.x, -128, 128) || !integer(frame.y, -128, 128)) fail('动画帧、时长或偏移无效');
    }
    if (action.kind && !['frames', 'parts'].includes(action.kind)) fail('动作类型无效');
    if (action.kind === 'parts') {
      if (!parts.size || !integer(action.durationTicks, 1, 1000) || !Array.isArray(action.tracks) || action.tracks.length > 32) fail('部件动作需要部件与 1–1000 tick 的时长');
      bakedTicks += action.durationTicks;
      const seen = new Set();
      for (const track of action.tracks) {
        if (!parts.has(track.partId) || seen.has(track.partId) || !Array.isArray(track.keys) || !track.keys.length || track.keys.length > 256) fail('部件轨道引用或关键帧数量无效');
        seen.add(track.partId);
        let previous = -1;
        for (const key of track.keys) {
          if (!integer(key.tick, 0, action.durationTicks) || key.tick <= previous || !poseValid(key)) fail('关键帧需要按时间排序，且不能重复或超出动作时长');
          previous = key.tick;
        }
      }
    }
  }
  if (bakedTicks > 2000) fail('部件动作合计不能超过 2000 tick，请缩短时长或减少动作');
  const regions = list(project.regions, 32, true);
  for (const region of project.regions) {
    if (!named(region.name, 80) || !integer(region.x, 0, 127) || !integer(region.y, 0, 127) ||
        !integer(region.width, 1, 128) || !integer(region.height, 1, 128) || region.x + region.width > 128 || region.y + region.height > 128) fail('触摸区域需要在画布范围内');
  }
  list(project.rules, 128, true);
  for (const rule of project.rules) {
    if (!named(rule.name, 80) || !Object.hasOwn(EVENTS, rule.event) || !actions.has(rule.actionId) ||
        (rule.regionId !== '*' && !regions.has(rule.regionId)) || (rule.fromActionId !== '*' && !actions.has(rule.fromActionId)) ||
        !integer(rule.cooldown, 0, 1500) || !integer(rule.priority, 0, 100) ||
        typeof rule.enabled !== 'boolean' || typeof rule.returnToDefault !== 'boolean') fail('互动规则引用或参数无效');
  }
  return project;
}

export function frameAt(action, tick) {
  let remaining = Math.max(0, tick) % totalTicks(action);
  for (let index = 0; index < action.frames.length; index++) {
    if (remaining < action.frames[index].ticks) return {frame: action.frames[index], index};
    remaining -= action.frames[index].ticks;
  }
  return {frame: action.frames[0], index: 0};
}

export function hitRegion(regions, x, y) {
  // The last drawn rectangle is on top, matching the preview overlay.
  return [...regions].reverse().find(r => x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height)?.id ?? '*';
}

// Shared canvas-entry state for the live preview. Named regions and the
// unlabelled canvas both use the same rule dispatcher.
export class HoverTracker {
  constructor() { this.reset(); }
  reset() { this.region = null; }
  move(regions, x, y) {
    const region = x >= 0 && x < 128 && y >= 0 && y < 128 ? hitRegion(regions, x, y) : null;
    const entered = region !== null && region !== this.region;
    this.region = region;
    return {region, entered};
  }
}

export class Preview {
  constructor(project) {
    this.project = project;
    this.tick = 0;
    this.elapsed = 0;
    this.actionId = project.defaultActionId;
    this.priority = -1;
    this.returnAfterCycle = false;
    this.lastInput = 0;
    this.idleSent = false;
    this.fired = new Map();
    this.logs = [];
    this.note('预览已就绪，可以摸摸头或点击身体');
  }
  get action() { return this.project.actions.find(a => a.id === this.actionId); }
  get current() { return this.action.kind === 'parts' ? {frame: this.action.frames[0], index: 0} : frameAt(this.action, this.elapsed); }
  note(message) {
    this.logs.unshift({tick: this.tick, message});
    this.logs = this.logs.slice(0, 30);
  }
  play(actionId, reason = '手动播放') {
    if (!this.project.actions.some(a => a.id === actionId)) return;
    this.actionId = actionId;
    this.elapsed = 0;
    this.priority = -1;
    this.returnAfterCycle = false;
    this.note(`${reason} → ${this.action.name}`);
  }
  input() {
    this.lastInput = this.tick;
    this.idleSent = false;
  }
  dispatch(event, regionId = '*') {
    if (event !== 'idle') this.input();
    const matching = this.project.rules.filter(rule => rule.enabled && rule.event === event &&
      (rule.regionId === '*' || rule.regionId === regionId) &&
      (rule.fromActionId === '*' || rule.fromActionId === this.actionId));
    const ready = matching.filter(rule => this.tick - (this.fired.get(rule.id) ?? -Infinity) >= rule.cooldown && rule.priority >= this.priority);
    // Stable sort preserves list order for equal priority.
    const rule = ready.sort((a, b) => b.priority - a.priority)[0];
    if (!rule) {
      this.note(`${EVENTS[event]} → ${matching.length ? '冷却中或当前动作优先级更高' : '没有匹配的规则'}`);
      return null;
    }
    this.fired.set(rule.id, this.tick);
    this.play(rule.actionId, rule.name);
    this.priority = rule.priority;
    this.returnAfterCycle = rule.returnToDefault;
    return rule;
  }
  step(count = 1, interactions = true) {
    for (let i = 0; i < count; i++) {
      this.tick++;
      this.elapsed++;
      if (this.elapsed >= totalTicks(this.action)) {
        if (!interactions && !this.action.loop) this.elapsed = totalTicks(this.action) - 1;
        else if (this.returnAfterCycle || !this.action.loop) this.play(this.project.defaultActionId, '动作结束，回到默认');
        else { this.elapsed = 0; this.priority = -1; }
      }
      if (interactions && !this.idleSent && this.tick - this.lastInput >= 125) {
        this.idleSent = true;
        this.dispatch('idle');
      }
    }
  }
}
