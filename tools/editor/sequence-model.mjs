// Geometry and admission rules for atomic local PNG sequence imports.
export const SEQUENCE_LIMITS = Object.freeze({frames: 256, sourceEdge: 4096, sourcePixels: 16_777_216, fileBytes: 8 * 1024 * 1024, totalBytes: 32 * 1024 * 1024, canvas: 128});

// Preserve sub-frame time across browser paints (e.g. 40ms on a 60Hz display).
export function sequenceClock(lastTime, now, duration) {
  if (lastTime == null || now < lastTime) return {steps: 0, lastTime: now};
  const steps = Math.floor((now - lastTime) / duration);
  return {steps, lastTime: lastTime + steps * duration};
}

function integer(value, min, max, label) {
  if (!Number.isInteger(value) || value < min || value > max) throw new Error(`${label}需要是 ${min}–${max} 之间的整数。`);
  return value;
}

export function pngSize(header) {
  const bytes = header instanceof Uint8Array ? header : new Uint8Array(header);
  if (bytes.byteLength < 24) throw new Error('PNG 文件头不完整。');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.getUint32(0) !== 0x89504e47 || view.getUint32(4) !== 0x0d0a1a0a || view.getUint32(8) !== 13 || view.getUint32(12) !== 0x49484452) throw new Error('请选择有效的 PNG 文件。');
  const width = integer(view.getUint32(16), 1, SEQUENCE_LIMITS.sourceEdge, 'PNG 宽度');
  const height = integer(view.getUint32(20), 1, SEQUENCE_LIMITS.sourceEdge, 'PNG 高度');
  return {width, height};
}

export function validateSources(sources) {
  integer(sources.length, 1, SEQUENCE_LIMITS.frames, '源文件数量');
  let pixels = 0, bytes = 0;
  for (const source of sources) {
    integer(source.width, 1, SEQUENCE_LIMITS.sourceEdge, 'PNG 宽度');
    integer(source.height, 1, SEQUENCE_LIMITS.sourceEdge, 'PNG 高度');
    integer(source.size, 1, SEQUENCE_LIMITS.fileBytes, '单个文件字节数');
    if (typeof source.name !== 'string' || !/\.png$/i.test(source.name)) throw new Error('序列只能包含 PNG 文件。');
    pixels += source.width * source.height; bytes += source.size;
  }
  if (pixels > SEQUENCE_LIMITS.sourcePixels) throw new Error('所选 PNG 合计解码像素超过上限，请分批导入。');
  if (bytes > SEQUENCE_LIMITS.totalBytes) throw new Error('所选 PNG 文件合计超过 32 MB，请分批导入。');
  return {pixels, bytes};
}

export function naturalOrder(sources, reverse = false) {
  const collator = new Intl.Collator('zh-CN', {numeric: true, sensitivity: 'base'});
  const ordered = sources.map((source, index) => ({source, index})).sort((a, b) => collator.compare(a.source.name, b.source.name) || a.index - b.index).map(item => item.source);
  return reverse ? ordered.reverse() : ordered;
}

export function gridFrames({width, height, cellWidth, cellHeight, offsetX = 0, offsetY = 0, gapX = 0, gapY = 0, order = 'rows', count}) {
  integer(width, 1, SEQUENCE_LIMITS.sourceEdge, '序列图宽度');
  integer(height, 1, SEQUENCE_LIMITS.sourceEdge, '序列图高度');
  integer(cellWidth, 1, SEQUENCE_LIMITS.sourceEdge, '单格宽度');
  integer(cellHeight, 1, SEQUENCE_LIMITS.sourceEdge, '单格高度');
  for (const [name, value] of Object.entries({offsetX, offsetY, gapX, gapY})) integer(value, 0, SEQUENCE_LIMITS.sourceEdge, name);
  if (!['rows', 'columns'].includes(order)) throw new Error('请选择逐行或逐列读取。');
  const columns = Math.floor((width - offsetX + gapX) / (cellWidth + gapX));
  const rows = Math.floor((height - offsetY + gapY) / (cellHeight + gapY));
  if (columns < 1 || rows < 1) throw new Error('格子或起始偏移超出序列图，至少需要一个完整格子。');
  const total = columns * rows;
  const selectedCount = count == null || count === '' ? total : count;
  integer(selectedCount, 1, Math.min(total, SEQUENCE_LIMITS.frames), '导入帧数');
  const frames = Array.from({length: selectedCount}, (_, index) => {
    const column = order === 'rows' ? index % columns : Math.floor(index / rows);
    const row = order === 'rows' ? Math.floor(index / columns) : index % rows;
    return {sourceIndex: 0, x: offsetX + column * (cellWidth + gapX), y: offsetY + row * (cellHeight + gapY), width: cellWidth, height: cellHeight, column, row};
  });
  return {columns, rows, total, frames};
}

export function normalizationPlan(frames, {mode = 'fit', scale = 1, anchorX = 64, anchorY = 128, sourceAnchor = 'bottom-center'} = {}) {
  integer(frames.length, 1, SEQUENCE_LIMITS.frames, '动画帧数量');
  integer(anchorX, 0, SEQUENCE_LIMITS.canvas, '目标锚点 X');
  integer(anchorY, 0, SEQUENCE_LIMITS.canvas, '目标锚点 Y');
  if (!['fit', 'original', 'custom'].includes(mode)) throw new Error('缩放方式无效。');
  if (!['bottom-center', 'center', 'top-left'].includes(sourceAnchor)) throw new Error('源定位点无效。');
  for (const frame of frames) {
    integer(frame.width, 1, SEQUENCE_LIMITS.sourceEdge, '源帧宽度');
    integer(frame.height, 1, SEQUENCE_LIMITS.sourceEdge, '源帧高度');
  }
  const maxWidth = Math.max(...frames.map(frame => frame.width)), maxHeight = Math.max(...frames.map(frame => frame.height));
  const sharedScale = mode === 'fit' ? Math.min(1, 128 / maxWidth, 128 / maxHeight) : mode === 'original' ? 1 : scale;
  if (!Number.isFinite(sharedScale) || sharedScale < .01 || sharedScale > 4) throw new Error('统一缩放需要在 1%–400% 之间。');
  const [sourceX, sourceY] = sourceAnchor === 'bottom-center' ? [.5, 1] : sourceAnchor === 'center' ? [.5, .5] : [0, 0];
  const placements = frames.map(frame => {
    const width = frame.width * sharedScale, height = frame.height * sharedScale;
    const x = Math.round(anchorX - width * sourceX), y = Math.round(anchorY - height * sourceY);
    return {source: {...frame}, x, y, width, height, clipped: x < 0 || y < 0 || x + width > 128 + 1e-9 || y + height > 128 + 1e-9};
  });
  return {scale: sharedScale, placements, clippedCount: placements.filter(placement => placement.clipped).length};
}

export function checkImportCapacity({assetCount, frameCount, partCount, kind}, count, destination) {
  integer(count, 1, SEQUENCE_LIMITS.frames, '导入帧数');
  if (!['append', 'new'].includes(destination)) throw new Error('导入目标无效。');
  if (!Number.isInteger(assetCount) || assetCount < 0 || assetCount + count > 256) throw new Error('导入后素材会超过 256 张，请减少数量或使用新工程。');
  if (destination === 'append' && kind === 'parts' && (!Number.isInteger(partCount) || partCount < 0 || partCount + count > 32)) throw new Error('当前工程最多 32 个部件，请减少数量或选择新建整帧动作。');
  if (destination === 'append' && kind !== 'parts' && (!Number.isInteger(frameCount) || frameCount < 1 || frameCount + count > 256)) throw new Error('当前动作最多 256 帧，请减少数量或选择新建动作。');
}
