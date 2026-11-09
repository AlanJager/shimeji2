import {currentPose, onionSamples, poseImages, poseMarkup} from './preview-model.mjs';
import {frameAt} from './model.mjs';

const SVG = 'http://www.w3.org/2000/svg';
let controller;

/** Mounts local view controls only; root supplies the existing playback clock. */
export function setupAuthoringPreview({getState}) {
  if (controller) return controller;
  const stage = document.getElementById('stage');
  const canvas = document.getElementById('pet-canvas');
  const pet = document.getElementById('pet-wrap');
  const currentImage = document.getElementById('pet-image');
  if (!stage || !canvas || !pet || !currentImage) throw new Error('Authoring preview requires the existing stage');

  const settings = {onion: false, native: true, count: 1, opacity: .25, wrap: true, previous: '#b65f61', next: '#448977'};
  const toolbar = document.createElement('div');
  toolbar.className = 'authoring-preview-toolbar';
  toolbar.setAttribute('aria-label', '动画预览辅助');
  toolbar.innerHTML = `
    <button id="onion-toggle" class="quiet small" aria-pressed="false">洋葱皮</button>
    <details id="onion-options" class="onion-options"><summary>参照设置</summary>
      <div class="onion-settings">
        <label>前后各显示 <select id="onion-count"><option value="1">1 帧</option><option value="2">2 帧</option><option value="3">3 帧</option></select></label>
        <label>不透明度 <input id="onion-opacity" type="range" min="5" max="60" value="25"><output id="onion-opacity-value">25%</output></label>
        <div class="onion-colors"><label>前帧 <input id="onion-previous-color" type="color" value="${settings.previous}"></label><label>后帧 <input id="onion-next-color" type="color" value="${settings.next}"></label></div>
        <label class="onion-wrap"><input id="onion-wrap" type="checkbox" checked>循环动作跨首尾参照</label>
        <p>整帧按相邻帧取样；部件按最多 5 tick 间隔取样。</p>
      </div>
    </details>
    <span id="onion-mode-hint" class="onion-mode-hint">只在编辑模式显示参照</span>
    <button id="native-preview-toggle" class="quiet small" aria-pressed="true">原尺寸预览</button>`;
  stage.before(toolbar);

  // Keep original SVG and its part hit targets intact. The viewport only gives
  // the right companion a separate, non-overlapping rail at narrower widths.
  const artViewport = document.createElement('div');
  artViewport.className = 'authoring-preview-art-viewport';
  stage.insertBefore(artViewport, pet);
  const floor = stage.querySelector('.preview-floor');
  if (floor) artViewport.append(floor);
  artViewport.append(pet);

  const defs = document.createElementNS(SVG, 'defs');
  defs.innerHTML = `
    <clipPath id="authoring-onion-clip"><rect width="128" height="128"/></clipPath>
    <filter id="authoring-onion-previous" x="-100%" y="-100%" width="300%" height="300%" color-interpolation-filters="sRGB"><feFlood id="authoring-previous-flood" flood-color="${settings.previous}" result="tint"/><feComposite in="tint" in2="SourceAlpha" operator="in"/></filter>
    <filter id="authoring-onion-next" x="-100%" y="-100%" width="300%" height="300%" color-interpolation-filters="sRGB"><feFlood id="authoring-next-flood" flood-color="${settings.next}" result="tint"/><feComposite in="tint" in2="SourceAlpha" operator="in"/></filter>`;
  const ghosts = document.createElementNS(SVG, 'g');
  ghosts.id = 'onion-ghosts';
  ghosts.setAttribute('clip-path', 'url(#authoring-onion-clip)');
  ghosts.setAttribute('pointer-events', 'none');
  ghosts.setAttribute('aria-hidden', 'true');
  canvas.insertBefore(defs, currentImage);
  canvas.insertBefore(ghosts, currentImage);

  const companion = document.createElement('aside');
  companion.id = 'native-preview-panel';
  companion.className = 'native-preview-panel';
  companion.setAttribute('aria-label', '原尺寸桌宠预览');
  companion.innerHTML = `
    <div class="native-preview-title"><strong>原尺寸</strong><span>100%</span></div>
    <div class="native-preview-checker"><svg id="native-preview-canvas" width="128" height="128" viewBox="0 0 128 128" overflow="hidden" role="img" aria-label="128 像素原尺寸预览"></svg></div>
    <p id="native-preview-pose" class="native-preview-pose"></p>
    <p id="native-preview-zoom" class="native-preview-zoom">固定 128 × 128 CSS 像素</p>`;
  stage.append(companion);
  stage.classList.add('authoring-native-visible');
  const nativeCanvas = companion.querySelector('svg');
  const toggle = toolbar.querySelector('#onion-toggle');
  const nativeToggle = toolbar.querySelector('#native-preview-toggle');
  let ghostMarkup = '', nativeMarkup = '';

  function update() {
    let state;
    try { state = getState(); } catch { return; } // Root may mount before the first project loads.
    if (!state?.project || !state.action) return;
    const editing = state.mode === 'edit';
    const enabled = settings.onion && editing;
    toggle.disabled = !editing;
    toggle.setAttribute('aria-pressed', settings.onion);
    toggle.classList.toggle('active', settings.onion);
    toolbar.querySelector('#onion-mode-hint').textContent = editing ? '前后姿势参照，不影响导出' : '互动模式隐藏参照';
    let markup = '';
    if (enabled) {
      const source = state.partDraft ?? state.project;
      const action = source.actions.find(action => action.id === state.action.id) ?? state.action;
      const samples = onionSamples(action, state.tick, {count: settings.count, wrap: settings.wrap && Boolean(action.loop)});
      for (const [direction, poses] of Object.entries(samples)) {
        markup += [...poses].reverse().map(sample => `<g data-onion-direction="${direction}" data-onion-tick="${sample.tick}" opacity="${settings.opacity / sample.distance}" filter="url(#authoring-onion-${direction})" pointer-events="none">${poseMarkup(poseImages(source, action, sample.tick))}</g>`).join('');
      }
      if (!markup) toolbar.querySelector('#onion-mode-hint').textContent = '当前没有可显示的前后姿势';
    }
    if (markup !== ghostMarkup) { ghosts.innerHTML = markup; ghostMarkup = markup; }
    ghosts.style.display = enabled ? '' : 'none';
    const native = settings.native ? poseMarkup(currentPose(state)) : '';
    if (native !== nativeMarkup) { nativeCanvas.innerHTML = native; nativeMarkup = native; }
    companion.hidden = !settings.native;
    stage.classList.toggle('authoring-native-visible', settings.native);
    nativeToggle.setAttribute('aria-pressed', settings.native);
    nativeToggle.classList.toggle('active', settings.native);
    companion.querySelector('#native-preview-pose').textContent = state.action.kind === 'parts'
      ? `${state.action.name} · tick ${state.tick}`
      : `${state.action.name} · 第 ${frameAt(state.action, state.tick).index + 1} 帧`;
    const zoom = document.getElementById('zoom');
    companion.querySelector('#native-preview-zoom').textContent = `编辑区 ${Math.round(Number(zoom?.value ?? 2) * 100)}% · 此处始终 100%`;
  }

  toggle.addEventListener('click', () => { settings.onion = !settings.onion; update(); });
  nativeToggle.addEventListener('click', () => { settings.native = !settings.native; update(); });
  toolbar.querySelector('#onion-count').addEventListener('change', event => { settings.count = Number(event.target.value); update(); });
  toolbar.querySelector('#onion-opacity').addEventListener('input', event => {
    settings.opacity = Number(event.target.value) / 100;
    toolbar.querySelector('#onion-opacity-value').textContent = `${event.target.value}%`;
    update();
  });
  toolbar.querySelector('#onion-wrap').addEventListener('change', event => { settings.wrap = event.target.checked; update(); });
  for (const direction of ['previous', 'next']) {
    toolbar.querySelector(`#onion-${direction}-color`).addEventListener('input', event => {
      settings[direction] = event.target.value;
      defs.querySelector(`#authoring-${direction}-flood`).setAttribute('flood-color', event.target.value);
    });
  }
  document.getElementById('zoom')?.addEventListener('change', update);
  toolbar.addEventListener('keydown', event => {
    // Space/arrow keys operate the focused view control, not the global timeline.
    event.stopPropagation();
    if (event.key === 'Escape') toolbar.querySelector('#onion-options').open = false;
  });
  controller = {update};
  return controller;
}
