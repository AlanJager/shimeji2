// Pixel touch-ups for one 128×128 frame. Shared assets are copied before the new pixels are stored.

const pngPattern = /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/;

function inside(x, y, width, height) {
  return x >= 0 && y >= 0 && x < width && y < height;
}

function stamp(data, width, height, x, y, style) {
  if (!inside(x, y, width, height)) return;
  const index = (y * width + x) * 4;
  if (style.eraser) { data[index + 3] = 0; return; }
  if (style.lockAlpha && data[index + 3] === 0) return;
  data[index] = style.color[0];
  data[index + 1] = style.color[1];
  data[index + 2] = style.color[2];
  if (!style.lockAlpha) data[index + 3] = style.color[3] ?? 255;
}

function plot(data, width, height, x, y, style) {
  stamp(data, width, height, x, y, style);
  if (style.symmetry) stamp(data, width, height, width - 1 - x, y, style);
}

function line(data, width, height, x0, y0, x1, y1, style) {
  let dx = Math.abs(x1 - x0), dy = Math.abs(y1 - y0);
  const sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
  let err = dx - dy;
  for (;;) {
    plot(data, width, height, x0, y0, style);
    if (x0 === x1 && y0 === y1) break;
    const doubled = err * 2;
    if (doubled > -dy) { err -= dy; x0 += sx; }
    if (doubled < dx) { err += dx; y0 += sy; }
  }
}

export function paintStroke(pixels, width, height, points, style) {
  const next = new Uint8ClampedArray(pixels);
  let previous = null;
  for (const point of points) {
    const x = Math.round(point.x), y = Math.round(point.y);
    if (previous) line(next, width, height, previous.x, previous.y, x, y, style);
    else plot(next, width, height, x, y, style);
    previous = {x, y};
  }
  return next;
}

function sameColor(data, index, color) {
  return data[index] === color[0] && data[index + 1] === color[1] && data[index + 2] === color[2] && data[index + 3] === color[3];
}

export function floodFill(pixels, width, height, x, y, style) {
  x = Math.round(x); y = Math.round(y);
  const next = new Uint8ClampedArray(pixels);
  if (!inside(x, y, width, height)) return next;
  const seeds = [[x, y]];
  if (style.symmetry) seeds.push([width - 1 - x, y]);
  const seen = new Uint8Array(width * height);
  for (const seed of seeds) {
    if (!inside(seed[0], seed[1], width, height)) continue;
    const origin = (seed[1] * width + seed[0]) * 4;
    const target = [next[origin], next[origin + 1], next[origin + 2], next[origin + 3]];
    if (style.lockAlpha && target[3] === 0) continue;
    const stack = [seed];
    while (stack.length) {
      const [cx, cy] = stack.pop();
      if (!inside(cx, cy, width, height)) continue;
      const pixel = cy * width + cx;
      if (seen[pixel]) continue;
      const index = pixel * 4;
      if (!sameColor(next, index, target)) continue;
      seen[pixel] = 1;
      if (style.lockAlpha && next[index + 3] === 0) continue;
      stamp(next, width, height, cx, cy, {...style, symmetry: false});
      stack.push([cx + 1, cy], [cx - 1, cy], [cx, cy + 1], [cx, cy - 1]);
    }
  }
  return next;
}

export function assetUseCount(project, assetId) {
  let count = 0;
  for (const action of project.actions ?? []) {
    for (const frame of action.frames ?? []) if (frame.assetId === assetId) count += 1;
    for (const track of action.tracks ?? []) for (const key of track.keys ?? []) if (key.assetId === assetId) count += 1;
  }
  for (const part of project.parts ?? []) if (part.assetId === assetId) count += 1;
  return count;
}

export function applyFramePixels(project, actionId, frameId, src, idFactory) {
  if (!pngPattern.test(src) || src.length > 128 * 1024) throw new Error('补画结果不是有效的 PNG。');
  const action = project.actions.find(item => item.id === actionId);
  const frame = action?.frames.find(item => item.id === frameId);
  if (!action || action.kind === 'parts' || !frame) throw new Error('请在整帧动作里选择要补的那一帧。');
  const source = project.assets.find(asset => asset.id === frame.assetId);
  if (!source) throw new Error('这一帧的素材不存在。');
  if (assetUseCount(project, source.id) > 1) {
    if (project.assets.length >= 256) throw new Error('素材已达到 256 张，请先清理未使用素材。');
    const id = idFactory('png');
    if (typeof id !== 'string' || !/^[a-zA-Z0-9_-]{1,64}$/.test(id) || project.assets.some(asset => asset.id === id)) {
      throw new Error('生成了无效或重复的素材 ID。');
    }
    project.assets.push({id, name: `${source.name} 副本`.slice(0, 80), src});
    frame.assetId = id;
    return {detached: true, assetId: id};
  }
  source.src = src;
  return {detached: false, assetId: source.id};
}

export function setupPixelPaint({getFrame, onApply, toast}) {
  const dialog = document.createElement('dialog');
  dialog.id = 'pixel-paint-dialog';
  dialog.innerHTML = `<div class="sequence-header"><div><span class="eyebrow">TOUCH UP</span><h2>补这一帧</h2></div><button id="pixel-close" class="icon-button" type="button" aria-label="关闭补画">×</button></div>
    <p id="pixel-status" class="sequence-help"></p>
    <canvas id="pixel-canvas" width="128" height="128" aria-label="128 像素补画画布"></canvas>
    <div class="bulk-buttons"><button id="pixel-pencil" class="small" type="button">铅笔</button><button id="pixel-eraser" class="small" type="button">橡皮</button><button id="pixel-fill" class="small" type="button">填充</button><button id="pixel-pick" class="small" type="button">吸管</button><label>颜色 <input id="pixel-color" type="color" value="#2b2b2b"></label><label class="sequence-check"><input id="pixel-symmetry" type="checkbox">水平对称</label><label class="sequence-check"><input id="pixel-lock" type="checkbox">锁透明</label></div>
    <div class="dialog-actions"><button id="pixel-cancel" type="button">取消</button><button id="pixel-apply" class="primary" type="button">写回当前帧</button></div>`;
  document.body.append(dialog);
  const $ = id => dialog.querySelector('#' + id);
  const canvas = $('pixel-canvas');
  const context = canvas.getContext('2d', {willReadFrequently: true});
  context.imageSmoothingEnabled = false;
  let tool = 'pencil', drawing = false, last = null, frameId = null;

  function style() {
    const hex = $('pixel-color').value;
    return {eraser: tool === 'eraser', lockAlpha: $('pixel-lock').checked, symmetry: $('pixel-symmetry').checked,
      color: [parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16), 255]};
  }
  function point(event) {
    const rect = canvas.getBoundingClientRect();
    return {x: (event.clientX - rect.left) / rect.width * canvas.width, y: (event.clientY - rect.top) / rect.height * canvas.height};
  }
  function commit(points) {
    const image = context.getImageData(0, 0, canvas.width, canvas.height);
    const next = tool === 'fill' ? floodFill(image.data, canvas.width, canvas.height, points[0].x, points[0].y, style()) : paintStroke(image.data, canvas.width, canvas.height, points, style());
    image.data.set(next);
    context.putImageData(image, 0, 0);
  }
  function select(next) { tool = next; for (const id of ['pencil', 'eraser', 'fill', 'pick']) $('pixel-' + id).disabled = id === next; }
  $('pixel-pencil').onclick = () => select('pencil');
  $('pixel-eraser').onclick = () => select('eraser');
  $('pixel-fill').onclick = () => select('fill');
  $('pixel-pick').onclick = () => select('pick');
  canvas.addEventListener('pointerdown', event => {
    if (tool === 'pick') {
      const at = point(event);
      const x = Math.max(0, Math.min(canvas.width - 1, Math.round(at.x)));
      const y = Math.max(0, Math.min(canvas.height - 1, Math.round(at.y)));
      const pixel = context.getImageData(x, y, 1, 1).data;
      $('pixel-color').value = '#' + [...pixel.slice(0, 3)].map(value => value.toString(16).padStart(2, '0')).join('');
      select('pencil');
      return;
    }
    drawing = tool !== 'fill';
    last = point(event);
    commit([last]);
    if (tool !== 'fill') canvas.setPointerCapture(event.pointerId);
  });
  canvas.addEventListener('pointermove', event => {
    if (!drawing || !last) return;
    const at = point(event);
    commit([last, at]);
    last = at;
  });
  canvas.addEventListener('pointerup', () => { drawing = false; last = null; });
  function close() { if (dialog.open) dialog.close(); }
  $('pixel-close').onclick = close;
  $('pixel-cancel').onclick = close;
  dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
  $('pixel-apply').onclick = () => {
    try {
      onApply(frameId, canvas.toDataURL('image/png'));
      close();
    } catch (error) { toast(error.message); }
  };
  return {open() {
    const frame = getFrame();
    if (!frame) return toast('请先选择整帧动作中的一帧。');
    frameId = frame.frameId;
    const image = new Image();
    image.onload = () => {
      context.clearRect(0, 0, 128, 128);
      context.drawImage(image, 0, 0, 128, 128);
      $('pixel-status').textContent = frame.shared > 1 ? `这张图被 ${frame.shared} 处引用。写回时只断开当前帧，其它帧保持原图。` : '这张图只被当前帧使用，写回会直接替换它。';
      select('pencil');
      dialog.showModal();
    };
    image.onerror = () => toast('当前帧的图片无法打开。');
    image.src = frame.src;
  }};
}
