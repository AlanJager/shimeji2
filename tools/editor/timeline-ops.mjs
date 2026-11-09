// Pure frame timeline edits. UI selection, clipboard and undo live in editor.mjs.
// Times are integer 40 ms ticks; this module never mutates caller-owned data.
const MAX_FRAMES = 256;
const MAX_TICKS = 1500;
const validId = value => typeof value === 'string' && /^[a-zA-Z0-9_-]{1,64}$/.test(value);
const copy = value => structuredClone(value);

function validateFrames(frames, label = '时间轴') {
  if (!Array.isArray(frames) || !frames.length || frames.length > MAX_FRAMES) {
    throw new Error(`${label}需要包含 1–${MAX_FRAMES} 帧。`);
  }
  const ids = new Set();
  for (const frame of frames) {
    if (!frame || !validId(frame.id) || ids.has(frame.id)) throw new Error(`${label}包含无效或重复的帧 ID。`);
    if (!Number.isInteger(frame.ticks) || frame.ticks < 1 || frame.ticks > MAX_TICKS) {
      throw new Error(`每帧停留时间需要是 1–${MAX_TICKS} 的整数 tick。`);
    }
    ids.add(frame.id);
  }
  return ids;
}

function selection(frames, selectedIds, ids) {
  if (!Array.isArray(selectedIds) && !(selectedIds instanceof Set)) throw new Error('请选择有效的动画帧。');
  const selected = new Set(selectedIds);
  for (const id of selected) if (!ids.has(id)) throw new Error('选中的帧已不存在，请重新选择。');
  return frames.map((frame, index) => selected.has(frame.id) ? index : -1).filter(index => index >= 0);
}

function createCopies(frames, idFactory, used) {
  if (!frames.length) return [];
  if (typeof idFactory !== 'function') throw new Error('复制动画帧需要提供 ID 生成器。');
  return frames.map(frame => {
    const id = idFactory('frame');
    if (!validId(id) || used.has(id)) throw new Error('生成了无效或重复的帧 ID，操作未应用。');
    used.add(id);
    return {...copy(frame), id};
  });
}

function ensureCapacity(count) {
  if (count > MAX_FRAMES) throw new Error(`一个动作最多 ${MAX_FRAMES} 帧。`);
}

/** Copy selected source frames in timeline order; paste always replaces IDs. */
export function copySelectedFrames(frames, selectedIds) {
  const ids = validateFrames(frames);
  return selection(frames, selectedIds, ids).map(index => copy(frames[index]));
}

/** Inclusive range for Shift selection, with stable IDs rather than indexes. */
export function selectFrameRange(frames, anchorId, targetId) {
  const ids = validateFrames(frames);
  if (!ids.has(anchorId) || !ids.has(targetId)) throw new Error('选择范围的端点已不存在。');
  const first = frames.findIndex(frame => frame.id === anchorId);
  const last = frames.findIndex(frame => frame.id === targetId);
  return frames.slice(Math.min(first, last), Math.max(first, last) + 1).map(frame => frame.id);
}

// Find a bounded proportional allocation with the exact aggregate tick target.
// Cumulative rounding keeps temporal boundaries close to the continuous result.
function speedTicks(weights, factor) {
  if (typeof factor !== 'number' || !Number.isFinite(factor) || factor <= 0) {
    throw new Error('速度倍率需要是大于 0 的有限数值。');
  }
  const target = Math.round(weights.reduce((sum, ticks) => sum + ticks, 0) / factor);
  if (!Number.isSafeInteger(target) || target < weights.length || target > weights.length * MAX_TICKS) {
    throw new Error(`这个速度超出了每帧 1–${MAX_TICKS} tick 的可用范围。`);
  }
  let low = 0, high = MAX_TICKS;
  for (let step = 0; step < 64; step++) {
    const scale = (low + high) / 2;
    const sum = weights.reduce((total, ticks) => total + Math.max(1, Math.min(MAX_TICKS, ticks * scale)), 0);
    if (sum < target) low = scale;
    else high = scale;
  }
  const scale = (low + high) / 2;
  let continuous = 0, previous = 0;
  return weights.map((ticks, index) => {
    continuous += Math.max(1, Math.min(MAX_TICKS, ticks * scale));
    const remaining = weights.length - index - 1;
    const minEnd = Math.max(previous + 1, target - remaining * MAX_TICKS);
    const maxEnd = Math.min(previous + MAX_TICKS, target - remaining);
    const end = index === weights.length - 1 ? target : Math.max(minEnd, Math.min(maxEnd, Math.round(continuous)));
    const duration = end - previous;
    previous = end;
    return duration;
  });
}

/**
 * Apply one atomic edit and return {frames, selectedIds, focusId}.
 *
 * duplicate/paste/pingpong: options.idFactory('frame') supplies fresh IDs.
 * paste: options.clipboard contains frames from copySelectedFrames; with no
 * selection, append at the end. duration: options.ticks is per-frame duration.
 * speed: options.factor > 1 plays faster, with total = round(oldTotal/factor).
 * reverse fills selected slots only. pingpong requires a contiguous selection
 * and appends reversed interior frames, so A B C becomes A B C B.
 */
export function applyFrameOperation(frames, selectedIds, operation, options = {}) {
  const known = new Set(['duplicate', 'paste', 'reverse', 'pingpong', 'duration', 'speed', 'delete']);
  if (!known.has(operation)) throw new Error('不支持的时间轴操作。');
  const ids = validateFrames(frames);
  const indices = selection(frames, selectedIds, ids);
  if (!indices.length && operation !== 'paste') throw new Error('请先选择至少一帧。');
  const out = copy(frames);
  const selected = indices.map(index => out[index]);
  let nextIds = selected.map(frame => frame.id);
  let focusId;

  switch (operation) {
    case 'duplicate': {
      ensureCapacity(frames.length + selected.length);
      const additions = createCopies(selected, options.idFactory, ids);
      out.splice(indices.at(-1) + 1, 0, ...additions);
      nextIds = additions.map(frame => frame.id);
      break;
    }
    case 'paste': {
      const clipboardIds = validateFrames(options.clipboard, '剪贴板');
      ensureCapacity(frames.length + options.clipboard.length);
      const additions = createCopies(options.clipboard, options.idFactory, new Set([...ids, ...clipboardIds]));
      out.splice(indices.length ? indices.at(-1) + 1 : out.length, 0, ...additions);
      nextIds = additions.map(frame => frame.id);
      break;
    }
    case 'reverse': {
      const reversed = [...selected].reverse();
      indices.forEach((index, offset) => { out[index] = reversed[offset]; });
      nextIds = reversed.map(frame => frame.id);
      break;
    }
    case 'pingpong': {
      if (indices.some((index, offset) => index !== indices[0] + offset)) throw new Error('往返循环需要选择连续的动画帧。');
      const interiors = selected.slice(1, -1).reverse();
      ensureCapacity(frames.length + interiors.length);
      const additions = createCopies(interiors, options.idFactory, ids);
      out.splice(indices.at(-1) + 1, 0, ...additions);
      nextIds.push(...additions.map(frame => frame.id));
      break;
    }
    case 'duration': {
      if (!Number.isInteger(options.ticks) || options.ticks < 1 || options.ticks > MAX_TICKS) {
        throw new Error(`统一停留时间需要是 1–${MAX_TICKS} 的整数 tick。`);
      }
      selected.forEach(frame => { frame.ticks = options.ticks; });
      break;
    }
    case 'speed': {
      const durations = speedTicks(selected.map(frame => frame.ticks), options.factor);
      selected.forEach((frame, index) => { frame.ticks = durations[index]; });
      break;
    }
    case 'delete': {
      if (selected.length === out.length) throw new Error('一个动作至少需要保留一帧。');
      const removed = new Set(nextIds);
      const kept = out.filter(frame => !removed.has(frame.id));
      focusId = kept[Math.min(indices[0], kept.length - 1)].id;
      return {frames: kept, selectedIds: [focusId], focusId};
    }
  }
  focusId = nextIds[0];
  return {frames: out, selectedIds: nextIds, focusId};
}
