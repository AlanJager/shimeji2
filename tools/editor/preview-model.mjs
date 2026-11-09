// View-only authoring helpers. These do not mutate or add fields to projects.
import {composeParts, frameAt, paintParts, totalTicks} from './model.mjs';

const escape = value => String(value).replace(/[&<>"']/g, char => ({'&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;'}[char]));
const wrapTick = (tick, duration) => ((tick % duration) + duration) % duration;
const boundTick = (tick, duration) => Math.max(0, Math.min(duration - 1, Math.floor(tick)));

export function advanceLoopTick(tick, count, {start, end}) {
  const offset = tick < start || tick >= end ? -1 : tick - start;
  return start + ((offset + count) % (end - start));
}

/** Neighboring distinct frame poses, or short regularly spaced part samples.
 * Arrays are nearest-first; rendering may reverse them to paint the nearest
 * ghost last. Without wrap, samples never cross the first/last pose boundary.
 */
export function onionSamples(action, tick, {count = 1, wrap = Boolean(action.loop)} = {}) {
  const duration = totalTicks(action);
  if (!Number.isFinite(tick) || duration <= 0) return {previous: [], next: []};
  count = Math.max(1, Math.min(3, Math.floor(count) || 1));
  const currentTick = wrap ? wrapTick(Math.floor(tick), duration) : boundTick(tick, duration);
  const multipart = action.kind === 'parts';
  const currentIndex = multipart ? currentTick : frameAt(action, currentTick).index;
  const step = multipart ? Math.min(5, Math.max(1, Math.floor(duration / 4))) : 1;
  const limit = multipart ? duration : action.frames.length;
  const starts = [];
  if (!multipart) {
    let at = 0;
    for (const frame of action.frames) { starts.push(at); at += frame.ticks; }
  }
  const sample = direction => {
    const seen = new Set([currentIndex]);
    const result = [];
    for (let distance = 1; distance <= count; distance++) {
      let index = currentIndex + direction * step * distance;
      if (wrap) index = wrapTick(index, limit);
      else if (index < 0 || index >= limit) continue;
      if (seen.has(index)) continue;
      seen.add(index);
      result.push({tick: multipart ? index : starts[index], index, distance});
    }
    return result;
  };
  return {previous: sample(-1), next: sample(1)};
}

/** Exactly the same part composition / frame translation as the editing pose.
 * Parts keep painter order and inherited visibility, with no recentring.
 */
export function poseImages(project, action, tick) {
  const assets = new Map(project.assets.map(asset => [asset.id, asset]));
  if (action.kind === 'parts') {
    return paintParts(composeParts(project, action, tick)).filter(part => part.visible && (part.opacity ?? 1) > 0 && assets.has(part.assetId)).map(part => ({
      id: part.id, assetId: part.assetId, src: assets.get(part.assetId).src,
      matrix: [...part.imageMatrix], opacity: part.opacity ?? 1,
    }));
  }
  const {frame} = frameAt(action, tick);
  const asset = assets.get(frame.assetId);
  return asset ? [{id: frame.id, assetId: frame.assetId, src: asset.src, matrix: [1, 0, 0, 1, frame.x, frame.y]}] : [];
}

/** Images deliberately have no editor data attributes or event handlers. */
export function poseMarkup(images) {
  return images.map(image => `<image width="128" height="128" href="${escape(image.src)}"${image.opacity != null && image.opacity !== 1 ? ` opacity="${image.opacity}"` : ''} transform="matrix(${image.matrix.join(' ')})" pointer-events="none"/>`).join('');
}

export function currentPose({project, action, tick, partDraft}) {
  const source = partDraft ?? project;
  const sourceAction = source.actions.find(candidate => candidate.id === action.id) ?? action;
  return poseImages(source, sourceAction, tick);
}
