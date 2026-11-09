import {TICK_MS, EVENTS, Preview, HoverTracker, clone, totalTicks, validateProject, hitRegion, upgradeProject, composeParts, paintParts, partPose, poseOf, setPartKey, removePart} from './model.mjs';
import {setupCatalog} from './catalog.mjs';
import {setupFrameTimeline} from './authoring-timeline.mjs';
import {setupSequenceImport} from './sequence-import.mjs';
import {setupAuthoringPreview} from './authoring-preview.mjs';
import {advanceLoopTick} from './preview-model.mjs';
import {setupVrmStudio} from './vrm-studio.mjs';
import {prepareVrmAction} from './vrm-integration.mjs';
import {prepareMediaImport} from './media-integration.mjs';
import {setupAssetLibrary} from './asset-library.mjs';
import {applyFramePixels, assetUseCount, setupPixelPaint} from './pixel-edit.mjs';

const $ = id => document.getElementById(id);
const escape = value => String(value).replace(/[&<>"']/g, char => ({'&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;'}[char]));
const uid = prefix => `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
const seconds = ticks => `${(ticks * TICK_MS / 1000).toFixed(2)} s`;
const requestedSession = new URLSearchParams(location.search).get('session');
const storageSession = requestedSession && /^[a-zA-Z0-9-]{1,64}$/.test(requestedSession) ? `.${requestedSession}` : '';
const STORAGE = 'shimeji2.editor.v1' + storageSession;
let project, preview, selectedActionId, selectedFrame = 0, selectedRuleId;
let selectedPartId, selectedTick = 0, poseScope = 'key', lastAssetId;
let partDraft = null;
let mode = 'edit', panel = 'animation', playing = true, showRegions = false, drawing = false;
let undo = [], redo = [], toastTimer, saveTimer, accumulator = 0, previousTime = 0;
let pointer = null, lastPet = -Infinity, petDistance = 0, lastHoverPoint = null;
const hover = new HoverTracker();
let layout = [];
let partsMarkup = '';
let bulkTimeline, sequenceImport, authoringPreview, vrmStudio, assetLibrary;
const action = () => project.actions.find(item => item.id === selectedActionId);
const frame = () => action().frames[selectedFrame];
const asset = assetId => project.assets.find(item => item.id === assetId);
const selectedRule = () => project.rules.find(rule => rule.id === selectedRuleId);
const selectedPart = () => project.parts.find(part => part.id === selectedPartId);
const isParts = () => action().kind === 'parts';

function ensurePartSelection() {
  if (!selectedPart()) selectedPartId = project.parts[0]?.id;
  selectedTick = Math.min(selectedTick, isParts() ? totalTicks(action()) : 0);
}

function makePart(assetId, id = uid('part')) {
  return {id, name: (asset(assetId)?.name ?? '新部件').replace(/\.png$/i, ''), assetId, parentId: null,
    x: 64, y: 128, pivotX: 64, pivotY: 128, rotation: 0, scaleX: 1, scaleY: 1, opacity: 1, zOrder: 0, visible: true};
}

function activateParts(p, id) {
  const a = p.actions.find(item => item.id === id);
  if (!p.parts.length) {
    const f = a.frames[0], part = makePart(f.assetId);
    part.x += f.x; part.y += f.y; p.parts.push(part);
  }
  a.kind = 'parts';
  a.durationTicks ??= Math.min(1000, a.frames.reduce((sum, f) => sum + f.ticks, 0));
  a.tracks ??= [];
}

function toast(message) {
  $('toast').textContent = message;
  $('toast').hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { $('toast').hidden = true; }, 3500);
}

function persist() {
  clearTimeout(saveTimer);
  $('save-state').textContent = '正在保存…';
  saveTimer = setTimeout(() => {
    try {
      localStorage.setItem(STORAGE, JSON.stringify(project));
      $('save-state').textContent = '已保存在本机';
    } catch {
      $('save-state').textContent = '请另存工程文件';
      toast('浏览器存储空间不足，请点击“保存工程”下载 JSON。');
    }
  }, 250);
}

function refreshPreview(reason) {
  preview = new Preview(project);
  preview.play(mode === 'edit' ? selectedActionId : project.defaultActionId, reason);
  if (mode === 'edit') preview.elapsed = isParts() ? selectedTick : action().frames.slice(0, selectedFrame).reduce((sum, f) => sum + f.ticks, 0);
  accumulator = 0;
  pointer = null;
  partDraft = null;
  hover.reset();
  lastHoverPoint = null; lastPet = -Infinity;
  petDistance = 0;
  $('pet-wrap').style.transform = '';
}

function edit(mutator, message) {
  try {
    const next = clone(project);
    mutator(next);
    validateProject(next);
    undo.push(JSON.stringify(project));
    undo = undo.slice(-20);
    redo = [];
    project = next;
    if (!project.actions.some(a => a.id === selectedActionId)) selectedActionId = project.defaultActionId;
    selectedFrame = Math.min(selectedFrame, action().frames.length - 1);
    ensurePartSelection();
    if (!selectedRule()) selectedRuleId = project.rules[0]?.id;
    refreshPreview('编辑已更新');
    if (mode === 'test') { selectedActionId = preview.actionId; selectedFrame = 0; selectedTick = 0; }
    persist();
    render();
    if (message) toast(message);
    return true;
  } catch (error) {
    toast(error.message);
    selectedFrame = Math.min(selectedFrame, action().frames.length - 1);
    render();
    return false;
  }
}

function history(from, to) {
  if (!from.length) return;
  to.push(JSON.stringify(project));
  project = JSON.parse(from.pop());
  if (!project.actions.some(a => a.id === selectedActionId)) selectedActionId = project.defaultActionId;
  selectedFrame = Math.min(selectedFrame, action().frames.length - 1);
  ensurePartSelection();
  bulkTimeline?.reset();
  selectedRuleId = project.rules[0]?.id;
  refreshPreview('历史操作'); persist(); render();
}

function selectAction(id) {
  bulkTimeline?.reset();
  selectedActionId = id;
  selectedFrame = 0;
  selectedTick = 0;
  preview.play(id);
  accumulator = 0;
  render();
}

function selectFrame(index, event) {
  bulkTimeline?.select(index, event);
  if (bulkTimeline && !bulkTimeline.isSelected(action().frames[index].id)) {
    index = action().frames.reduce((nearest, f, i) => bulkTimeline.isSelected(f.id) && (nearest < 0 || Math.abs(i - index) < Math.abs(nearest - index)) ? i : nearest, -1);
  }
  selectedFrame = index;
  playing = false;
  if (preview.actionId !== selectedActionId) preview.play(selectedActionId);
  preview.elapsed = action().frames.slice(0, index).reduce((sum, f) => sum + f.ticks, 0);
  preview.priority = -1;
  render();
}

function seek(tick, rebuild = true) {
  selectedTick = Math.max(0, Math.min(totalTicks(action()), Number(tick)));
  playing = false;
  if (preview.actionId !== selectedActionId) preview.play(selectedActionId);
  preview.elapsed = selectedTick;
  preview.priority = -1;
  if (rebuild) render();
  else {
    renderDynamic(); renderPartInspector();
    $('selected-frame-label').textContent = `编辑位置 ${selectedTick} tick · 40 ms / tick`;
    document.querySelectorAll('[data-key-tick]').forEach(button => button.classList.toggle('selected', Number(button.dataset.keyTick) === selectedTick && button.dataset.keyPart === selectedPartId));
  }
}

function selectPart(id) {
  selectedPartId = id; panel = 'parts';
  if (isParts()) selectedTick = Math.min(totalTicks(action()), preview.elapsed);
  playing = false; render();
}

function addPart(assetId) {
  if (project.parts.length >= 32) return toast('一个工程最多 32 个部件。');
  const part = makePart(assetId), id = selectedActionId;
  selectedPartId = part.id; selectedTick = 0; playing = false; mode = 'edit'; panel = 'parts';
  edit(p => { p.parts.push(part); activateParts(p, id); }, '部件已添加；在时间轴上选择时刻即可记录动作。');
}

function renderLibrary() {
  $('project-name').value = project.name;
  $('action-count').textContent = String(project.actions.length).padStart(2, '0');
  $('action-list').innerHTML = project.actions.map(a => `
    <button class="action-item ${a.id === selectedActionId ? 'selected' : ''}" data-action="${escape(a.id)}" aria-pressed="${a.id === selectedActionId}">
      <img src="${escape(asset(a.frames[0].assetId).src)}" alt="">
      <span class="action-copy"><strong>${escape(a.name)}</strong><small>${a.kind === 'parts' ? `${project.parts.length} 部件` : `${a.frames.length} 帧`} · ${seconds(totalTicks(a))} · ${a.loop ? '循环' : '单次'}</small></span>
      ${a.id === project.defaultActionId ? '<span class="default-mark">默认</span>' : ''}
    </button>`).join('');
  assetLibrary?.render();
  $('part-count').textContent = String(project.parts.length).padStart(2, '0');
  $('part-list').innerHTML = project.parts.length ? [...project.parts].reverse().map(part => `<button class="part-item ${part.id === selectedPartId ? 'selected' : ''}" data-part="${escape(part.id)}"><span class="part-eye">${part.visible ? '◉' : '○'}</span><img src="${escape(asset(part.assetId).src)}" alt=""><span>${escape(part.name)}</span><small>${part.parentId ? '↳' : '根'}</small></button>`).join('') : '<p class="part-empty">添加 PNG，自由组装部件</p>';
  document.querySelector('.library-hint').textContent = '单击选中 · 双击添加 · 分组可预览';
}

function renderTimeline() {
  const current = action();
  bulkTimeline?.sync();
  $('duplicate-frame').hidden = isParts(); $('delete-frame').hidden = isParts();
  $('timeline-scroll').classList.toggle('parts-timeline', isParts());
  if (isParts()) {
    const duration = totalTicks(current);
    $('ruler').style.width = '100%'; $('ruler').innerHTML = '';
    $('frame-track').innerHTML = `<div class="parts-scrub"><label for="parts-cursor">时间游标 <strong id="parts-cursor-value">${selectedTick}</strong> / ${duration} tick</label><input id="parts-cursor" type="range" min="0" max="${duration}" step="1" value="${selectedTick}" aria-label="部件动画时间游标"></div>` + project.parts.map(part => {
      const keys = current.tracks.find(t => t.partId === part.id)?.keys ?? [];
      return `<div class="part-track-row"><button class="track-name ${selectedPartId === part.id ? 'selected' : ''}" data-track-part="${escape(part.id)}">${escape(part.name)}</button><div class="part-key-lane" data-track-lane="${escape(part.id)}"><span class="track-cursor"></span>${keys.map(key => `<button class="part-key ${key.tick === selectedTick && selectedPartId === part.id ? 'selected' : ''}" style="left:${key.tick / duration * 100}%" data-key-part="${escape(part.id)}" data-key-tick="${key.tick}" aria-label="${escape(part.name)} 第 ${key.tick} tick 关键帧" title="${key.tick} tick">◆</button>`).join('')}${keys.length ? '' : '<span class="track-empty">默认姿势 · 点击轨道选择时刻</span>'}</div></div>`;
    }).join('');
    $('timeline-summary').textContent = `${project.parts.length} 轨 / ${seconds(duration)}`;
    $('selected-frame-label').textContent = `编辑位置 ${selectedTick} tick · 40 ms / tick`;
    $('undo').disabled = !undo.length; $('redo').disabled = !redo.length;
    return;
  }
  const scale = Math.min(14, Math.max(2, 70 / Math.min(...current.frames.map(f => f.ticks))));
  let left = 0, ticks = 0;
  layout = current.frames.map(f => {
    const item = {left, width: Math.max(14, f.ticks * scale), tick: ticks};
    left += item.width + 5; ticks += f.ticks;
    return item;
  });
  $('ruler').style.width = `${left}px`;
  $('ruler').innerHTML = layout.map((item, index) => `<span style="left:${20 + item.left}px">${index === 0 ? '0.00 s' : seconds(item.tick)}</span>`).join('');
  $('frame-track').innerHTML = current.frames.map((f, index) => `<button class="frame-cell ${index === selectedFrame ? 'selected' : ''} ${bulkTimeline?.isSelected(f.id) ? 'multi-selected' : ''}" style="width:${layout[index].width}px;max-width:none" data-frame="${index}" draggable="true" aria-label="第 ${index + 1} 帧，${f.ticks} tick" aria-pressed="${bulkTimeline?.isSelected(f.id) ?? index === selectedFrame}"><img src="${escape(asset(f.assetId).src)}" alt="" style="max-width:100%"><span class="frame-meta"><span>${String(index + 1).padStart(2, '0')}</span><span>${f.ticks}t</span></span></button>`).join('');
  $('timeline-summary').textContent = `${current.frames.length} 帧 / ${seconds(totalTicks(current))}`;
  $('selected-frame-label').textContent = `选中帧 ${String(selectedFrame + 1).padStart(2, '0')}`;
  $('undo').disabled = !undo.length;
  $('redo').disabled = !redo.length;
  $('delete-frame').disabled = current.frames.length <= 1 || bulkTimeline?.selection().length === current.frames.length;
}

function options(items, value, allLabel) {
  return (allLabel ? `<option value="*" ${value === '*' ? 'selected' : ''}>${allLabel}</option>` : '') +
    items.map(item => `<option value="${escape(item.id)}" ${value === item.id ? 'selected' : ''}>${escape(item.name)}</option>`).join('');
}

function renderInspector() {
  const current = action(), f = frame();
  $('action-name').value = current.name;
  $('action-loop').checked = current.loop;
  $('action-default').checked = current.id === project.defaultActionId;
  $('delete-action').disabled = project.actions.length <= 1;
  $('action-kind').value = current.kind;
  $('parts-duration-field').hidden = !isParts();
  $('parts-duration').value = current.durationTicks ?? 50;
  $('frame-properties').hidden = isParts();
  $('frame-number').textContent = `${String(selectedFrame + 1).padStart(2, '0')} / ${String(current.frames.length).padStart(2, '0')}`;
  $('frame-preview').innerHTML = `<img src="${escape(asset(f.assetId).src)}" alt="选中动画帧">`;
  $('frame-ticks').value = f.ticks;
  $('frame-duration').textContent = `${f.ticks * TICK_MS} ms · 1 tick = 40 ms`;
  $('frame-x').value = f.x; $('frame-y').value = f.y;
  $('frame-left').disabled = selectedFrame === 0;
  $('frame-right').disabled = selectedFrame === current.frames.length - 1;
  renderPartInspector();
  $('region-list').innerHTML = project.regions.length ? project.regions.map(r => `<div class="region-row"><input data-region-name="${escape(r.id)}" value="${escape(r.name)}" aria-label="区域名称" maxlength="40"><span>${r.width} × ${r.height}</span><button class="icon-button" data-remove-region="${escape(r.id)}" aria-label="删除区域 ${escape(r.name)}">×</button></div>`).join('') : '<p class="muted">在画布上绘制第一个触摸区域。</p>';
  $('rule-count').textContent = project.rules.length;
  $('rule-list').innerHTML = project.rules.map(rule => `<button class="rule-item ${rule.id === selectedRuleId ? 'selected' : ''} ${rule.enabled ? '' : 'disabled'}" data-rule="${escape(rule.id)}"><span class="rule-indicator"></span><span><strong>${escape(rule.name)}</strong><small>${escape(EVENTS[rule.event])} → ${escape(project.actions.find(a => a.id === rule.actionId).name)}</small></span></button>`).join('');
  const rule = selectedRule();
  $('rule-editor').innerHTML = !rule ? '' : `
    <label>规则名称<input data-rule-field="name" value="${escape(rule.name)}" maxlength="80"></label>
    <div class="two-fields"><label>发生什么<select data-rule-field="event">${Object.entries(EVENTS).map(([key, name]) => `<option value="${key}" ${rule.event === key ? 'selected' : ''}>${name}</option>`).join('')}</select></label><label>在哪个区域<select data-rule-field="regionId">${options(project.regions, rule.regionId, '整个画布')}</select></label></div>
    <label>当前动作满足<select data-rule-field="fromActionId">${options(project.actions, rule.fromActionId, '任意动作')}</select></label>
    <label>就播放这个动作<select data-rule-field="actionId">${options(project.actions, rule.actionId)}</select></label>
    <div class="two-fields"><label>冷却时间 / tick<input data-rule-field="cooldown" type="number" min="0" max="1500" value="${rule.cooldown}"></label><label>优先级 / 0–100<input data-rule-field="priority" type="number" min="0" max="100" value="${rule.priority}"></label></div>
    <div class="switch-row"><label for="rule-return">播完一轮回到默认</label><input id="rule-return" data-rule-field="returnToDefault" type="checkbox" role="switch" ${rule.returnToDefault ? 'checked' : ''}></div>
    <div class="switch-row"><label for="rule-enabled">启用这条规则</label><input id="rule-enabled" data-rule-field="enabled" type="checkbox" role="switch" ${rule.enabled ? 'checked' : ''}></div>
    <div class="two-fields"><button id="test-rule" class="primary small">测试触发事件</button><button id="remove-rule" class="quiet small danger">删除规则</button></div>`;
}

function renderPartInspector() {
  const part = selectedPart();
  $('parts-empty').hidden = Boolean(part);
  $('part-properties').hidden = !part;
  if (!part) return;
  const pose = isParts() && poseScope === 'key' ? partPose(part, action(), selectedTick) : poseOf(part);
  $('part-name').value = part.name;
  const validParents = project.parts.filter(candidate => {
    let current = candidate;
    while (current) {
      if (current.id === part.id) return false;
      current = project.parts.find(p => p.id === current.parentId);
    }
    return true;
  });
  $('part-parent').innerHTML = `<option value="">无父级 · 画布坐标</option>` + options(validParents, part.parentId);
  $('part-parent').value = part.parentId ?? '';
  $('part-asset').innerHTML = options(project.assets, pose.assetId);
  $('part-scope').value = isParts() ? poseScope : 'base';
  $('part-scope').disabled = !isParts();
  for (const field of ['x', 'y', 'rotation', 'scaleX', 'scaleY', 'opacity']) $('part-' + field).value = Math.round(pose[field] * 1000) / 1000;
  $('part-z').value = pose.zOrder ?? 0;
  for (const field of ['pivotX', 'pivotY']) $('part-' + field).value = part[field];
  $('part-visible').checked = pose.visible;
  const track = currentTrack(part.id);
  $('part-edit-tick').textContent = isParts() && poseScope === 'key' ? `TICK ${selectedTick}` : 'DEFAULT POSE';
  $('save-part-key').disabled = !isParts();
  $('delete-part-key').disabled = !isParts() || !track?.keys.some(key => key.tick === selectedTick);
  const index = project.parts.findIndex(p => p.id === part.id);
  $('part-back').disabled = index === 0;
  $('part-front').disabled = index === project.parts.length - 1;
}

function currentTrack(partId) { return action().tracks?.find(track => track.partId === partId); }

function writePose(p, id, pose, tick = selectedTick) {
  if (action().kind === 'parts' && poseScope === 'key') {
    setPartKey(p.actions.find(a => a.id === selectedActionId), id, tick, pose);
  } else Object.assign(p.parts.find(part => part.id === id), poseOf(pose));
}

function changePartPose(field, value) {
  const part = selectedPart(); if (!part) return;
  playing = false; mode = 'edit';
  const pose = isParts() && poseScope === 'key' ? partPose(part, action(), selectedTick) : poseOf(part);
  pose[field] = value;
  edit(p => writePose(p, part.id, pose));
}

function renderRegions() {
  $('regions-overlay').innerHTML = showRegions || drawing ? project.regions.map(r => `<rect class="region-rect" x="${r.x}" y="${r.y}" width="${r.width}" height="${r.height}" rx="2"/><text class="region-label" x="${r.x + 2}" y="${Math.max(6, r.y - 2)}">${escape(r.name)}</text>`).join('') : '';
}

function render() {
  renderLibrary(); renderTimeline(); renderInspector(); renderRegions();
  $('mode-edit').classList.toggle('active', mode === 'edit');
  $('mode-test').classList.toggle('active', mode === 'test');
  $('mode-edit').setAttribute('aria-pressed', mode === 'edit');
  $('mode-test').setAttribute('aria-pressed', mode === 'test');
  for (const name of ['animation', 'parts', 'interaction']) {
    $('tab-' + name).classList.toggle('active', panel === name);
    $('tab-' + name).setAttribute('aria-selected', panel === name);
    $(name === 'parts' ? 'parts-panel' : name + '-panel').hidden = panel !== name;
  }
  $('animation-panel').hidden = panel !== 'animation';
  $('interaction-panel').hidden = panel !== 'interaction';
  $('draw-region').hidden = panel !== 'interaction';
  $('toggle-regions').setAttribute('aria-pressed', showRegions);
  $('toggle-regions').textContent = showRegions ? '隐藏触摸区域' : '显示触摸区域';
  $('pet-wrap').classList.toggle('testing', mode === 'test');
  $('pet-wrap').classList.toggle('drawing', drawing);
  $('stage-description').textContent = mode === 'test' ? '点击、抚摸或拖拽，观察角色的反应' : '编辑中的每一帧，即时可见';
  $('stage-hint').textContent = drawing ? '按住鼠标拖出一个矩形区域 · Esc 取消' : mode === 'test' ? '摸摸头、点点身体，或安静地陪它待一会儿' : '选择一帧，调整它的停留时间';
  renderDynamic();
}

function renderDynamic() {
  const current = preview.current, f = current.frame;
  const multipart = preview.action.kind === 'parts';
  $('pet-image').style.display = multipart ? 'none' : '';
  const image = $('pet-image'), imageSource = asset(f.assetId).src;
  if (image.getAttribute('href') !== imageSource) image.setAttribute('href', imageSource);
  if (image.getAttribute('x') !== String(f.x)) image.setAttribute('x', f.x);
  if (image.getAttribute('y') !== String(f.y)) image.setAttribute('y', f.y);
  if (multipart) {
    const source = partDraft ?? project;
    const composed = paintParts(composeParts(source, source.actions.find(a => a.id === preview.actionId), preview.elapsed));
    let markup = composed.filter(part => part.visible && (part.opacity ?? 1) > 0).map(part => `<image data-preview-part="${escape(part.id)}" width="128" height="128" href="${escape(asset(part.assetId).src)}" opacity="${part.opacity ?? 1}" transform="matrix(${part.imageMatrix.join(' ')})"/>`).join('');
    const selected = composed.find(part => part.id === selectedPartId);
    if (selected && panel === 'parts' && mode === 'edit') markup += `<circle class="part-pivot" cx="${selected.matrix[4]}" cy="${selected.matrix[5]}" r="1.5"/>`;
    if (markup !== partsMarkup) { $('parts-canvas').innerHTML = markup; partsMarkup = markup; }
  } else if (partsMarkup) { $('parts-canvas').innerHTML = ''; partsMarkup = ''; }
  $('live-action').textContent = preview.action.name;
  $('live-detail').textContent = multipart ? `${preview.elapsed} / ${totalTicks(preview.action)} tick · ${project.parts.length} 部件` : `${String(current.index + 1).padStart(2, '0')} / ${String(preview.action.frames.length).padStart(2, '0')} 帧`;
  $('play').textContent = playing ? 'Ⅱ' : '▶';
  $('play').setAttribute('aria-label', playing ? '暂停播放' : '开始播放');
  $('timecode').textContent = `${String(Math.floor(preview.elapsed * .04 / 60)).padStart(2, '0')}:${(preview.elapsed * .04 % 60).toFixed(2).padStart(5, '0')}`;
  $('playhead').hidden = preview.actionId !== selectedActionId || multipart;
  if (multipart && preview.actionId === selectedActionId) {
    const ratio = preview.elapsed / totalTicks(preview.action);
    document.querySelectorAll('.track-cursor').forEach(el => { el.style.left = `${ratio * 100}%`; });
    if ($('parts-cursor')) { $('parts-cursor').value = preview.elapsed; $('parts-cursor-value').textContent = preview.elapsed; }
  } else if (preview.actionId === selectedActionId) {
    const cell = layout[current.index];
    $('playhead').style.left = `${20 + cell.left + (preview.elapsed - cell.tick) / f.ticks * cell.width}px`;
  }
  const latest = preview.logs[0];
  $('status-text').textContent = latest?.message ?? '就绪';
  $('log-count').textContent = preview.logs.length;
  if (!$('log-panel').hidden) $('event-log').innerHTML = preview.logs.map(log => `<li><time>${seconds(log.tick)}</time><span>${escape(log.message)}</span></li>`).join('');
  authoringPreview?.update();
}

function setMode(next) {
  mode = next;
  drawing = false;
  $('drawing-rect').hidden = true;
  playing = true;
  if (mode === 'test') { showRegions = true; panel = 'interaction'; selectedActionId = project.defaultActionId; selectedFrame = 0; selectedTick = 0; }
  refreshPreview('切换预览模式'); render();
}

function event(eventName, region = '*') {
  if (mode !== 'test') setMode('test');
  playing = true;
  const matched = preview.dispatch(eventName, region);
  if (matched) {
    selectedActionId = preview.actionId; selectedFrame = 0; selectedTick = 0;
    render();
  } else renderDynamic();
}

function moveFrame(from, to) {
  if (to < 0 || to >= action().frames.length || from === to) return;
  const id = selectedActionId;
  selectedFrame = to;
  edit(p => {
    const frames = p.actions.find(a => a.id === id).frames;
    frames.splice(to, 0, frames.splice(from, 1)[0]);
  });
}

function download(body, name) {
  const url = URL.createObjectURL(body);
  const a = document.createElement('a'); a.href = url; a.download = name;
  a.click(); setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

async function confirmChange(title, text) {
  $('confirm-title').textContent = title; $('confirm-text').textContent = text;
  const dialog = $('confirm-dialog');
  dialog.showModal();
  return new Promise(resolve => dialog.addEventListener('close', () => resolve(dialog.returnValue === 'ok'), {once: true}));
}

async function openProject(next) {
  next = upgradeProject(next);
  await Promise.all(next.assets.filter(a => a.src.startsWith('data:')).map(a => new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => image.width === 128 && image.height === 128 ? resolve() : reject(new Error('工程中的自定义 PNG 必须为 128 × 128'));
    image.onerror = () => reject(new Error('工程中有无法读取的 PNG'));
    image.src = a.src;
  })));
  project = next; selectedActionId = next.defaultActionId; selectedFrame = 0; selectedRuleId = next.rules[0]?.id;
  bulkTimeline?.reset(true);
  assetLibrary?.reset();
  selectedPartId = next.parts[0]?.id; selectedTick = 0; partDraft = null;
  undo = []; redo = []; drawing = false; playing = true;
  $('drawing-rect').hidden = true;
  refreshPreview('打开工程'); persist(); render();
}

async function importImages(files) {
  if (files.length) await sequenceImport.open(files);
}

function beginDrawing() {
  if (project.regions.length >= 32) return toast('一个工程最多 32 个触摸区域。');
  drawing = true; showRegions = true; playing = false; pointer = null;
  render();
}

function point(e) {
  const rect = $('pet-canvas').getBoundingClientRect();
  return {x: Math.max(0, Math.min(128, (e.clientX - rect.left) / rect.width * 128)), y: Math.max(0, Math.min(128, (e.clientY - rect.top) / rect.height * 128))};
}

$('action-list').addEventListener('click', e => { const button = e.target.closest('[data-action]'); if (button) selectAction(button.dataset.action); });
$('frame-track').addEventListener('click', e => {
  const key = e.target.closest('[data-key-part]');
  if (key) { selectedPartId = key.dataset.keyPart; panel = 'parts'; return seek(Number(key.dataset.keyTick)); }
  const track = e.target.closest('[data-track-part]');
  if (track) return selectPart(track.dataset.trackPart);
  const lane = e.target.closest('[data-track-lane]');
  if (lane) {
    selectedPartId = lane.dataset.trackLane; panel = 'parts';
    const rect = lane.getBoundingClientRect();
    return seek(Math.round((e.clientX - rect.left) / rect.width * totalTicks(action())));
  }
  const button = e.target.closest('[data-frame]'); if (button) selectFrame(Number(button.dataset.frame), e);
});
$('frame-track').addEventListener('input', e => { if (e.target.id === 'parts-cursor') seek(e.target.value, false); });
$('frame-track').addEventListener('change', e => { if (e.target.id === 'parts-cursor') seek(e.target.value); });
let draggedFrame;
$('frame-track').addEventListener('dragstart', e => {
  const cell = e.target.closest('[data-frame]'); if (!cell) return;
  draggedFrame = Number(cell.dataset.frame);
  e.dataTransfer.effectAllowed = 'move'; e.dataTransfer.setData('text/plain', String(draggedFrame));
});
$('frame-track').addEventListener('dragover', e => { e.preventDefault(); const cell = e.target.closest('[data-frame]'); if (cell) cell.classList.add('drag-over'); });
$('frame-track').addEventListener('dragleave', e => e.target.closest('[data-frame]')?.classList.remove('drag-over'));
$('frame-track').addEventListener('drop', e => {
  e.preventDefault();
  const files = [...e.dataTransfer.files];
  const cell = e.target.closest('[data-frame]');
  if (files.length) {
    e.stopPropagation();
    document.querySelectorAll('.drag-over').forEach(el => el.classList.remove('drag-over'));
    if (cell && !isParts()) sequenceImport.open(files, {insertAfterFrameId: action().frames[Number(cell.dataset.frame)].id}).catch(error => toast(error.message));
    else importImages(files);
    return;
  }
  if (cell && Number.isInteger(draggedFrame)) moveFrame(draggedFrame, Number(cell.dataset.frame));
  draggedFrame = undefined;
});
$('frame-track').addEventListener('dragend', () => { draggedFrame = undefined; document.querySelectorAll('.drag-over').forEach(el => el.classList.remove('drag-over')); });
$('mode-edit').onclick = () => setMode('edit'); $('mode-test').onclick = () => setMode('test');
$('tab-animation').onclick = () => { panel = 'animation'; render(); };
$('tab-parts').onclick = () => { panel = 'parts'; render(); };
$('tab-interaction').onclick = () => { panel = 'interaction'; showRegions = true; render(); };
$('toggle-regions').onclick = () => { showRegions = !showRegions; render(); };
$('zoom').onchange = () => { const size = 128 * Number($('zoom').value); $('pet-wrap').style.width = `${size}px`; $('pet-wrap').style.height = `${size}px`; };
$('play').onclick = () => {
  playing = !playing; accumulator = 0;
  const range = mode === 'edit' ? bulkTimeline?.playbackRange() : null;
  if (playing && range && (preview.elapsed < range.start || preview.elapsed >= range.end)) preview.elapsed = range.start;
  if (!playing && isParts()) { selectedTick = preview.elapsed; render(); }
  else { if (isParts() && preview.elapsed >= totalTicks(action())) preview.elapsed = 0; renderDynamic(); }
};
$('restart').onclick = () => {
  refreshPreview('从头播放');
  if (mode === 'test') { selectedActionId = preview.actionId; selectedFrame = 0; selectedTick = 0; render(); }
  else { const range = bulkTimeline?.playbackRange(); if (range) preview.elapsed = range.start; renderDynamic(); }
};
$('step').onclick = () => {
  playing = false;
  const range = mode === 'edit' ? bulkTimeline?.playbackRange() : null;
  if (range) { preview.tick++; preview.elapsed = advanceLoopTick(preview.elapsed, 1, range); }
  else preview.step(1, mode === 'test');
  if (mode === 'test' && selectedActionId !== preview.actionId) { selectedActionId = preview.actionId; selectedFrame = 0; selectedTick = 0; render(); }
  else renderDynamic();
};
$('speed').onchange = () => { accumulator = 0; };
$('undo').onclick = () => history(undo, redo); $('redo').onclick = () => history(redo, undo);
$('project-name').onchange = e => edit(p => { p.name = e.target.value.trim(); });
$('action-name').onchange = e => { const id = selectedActionId; edit(p => { p.actions.find(a => a.id === id).name = e.target.value.trim(); }); };
$('action-loop').onchange = e => { const id = selectedActionId; edit(p => { p.actions.find(a => a.id === id).loop = e.target.checked; }); };
$('action-kind').onchange = e => {
  const kind = e.target.value, id = selectedActionId;
  selectedTick = 0; playing = false; mode = 'edit';
  edit(p => { if (kind === 'parts') activateParts(p, id); else p.actions.find(a => a.id === id).kind = 'frames'; });
};
$('parts-duration').onchange = e => {
  const id = selectedActionId, duration = Number(e.target.value);
  edit(p => {
    const a = p.actions.find(a => a.id === id);
    if (a.tracks.some(t => t.keys.some(key => key.tick > duration))) throw new Error('新时长会超出已有关键帧，请先删除或移动末尾关键帧。');
    a.durationTicks = duration;
  });
};
$('add-part').onclick = () => addPart(lastAssetId ?? selectedPart()?.assetId ?? frame().assetId);
$('part-list').onclick = e => { const button = e.target.closest('[data-part]'); if (button) selectPart(button.dataset.part); };
$('part-scope').onchange = e => { poseScope = e.target.value; renderPartInspector(); };
$('part-name').onchange = e => { const id = selectedPartId; edit(p => { p.parts.find(part => part.id === id).name = e.target.value.trim(); }); };
$('part-parent').onchange = e => { const id = selectedPartId, value = e.target.value || null; edit(p => { p.parts.find(part => part.id === id).parentId = value; }); };
for (const field of ['pivotX', 'pivotY']) $('part-' + field).onchange = e => { const id = selectedPartId, value = Number(e.target.value); edit(p => { p.parts.find(part => part.id === id)[field] = value; }); };
for (const field of ['x', 'y', 'rotation', 'scaleX', 'scaleY', 'opacity']) $('part-' + field).onchange = e => changePartPose(field, Number(e.target.value));
$('part-z').onchange = e => changePartPose('zOrder', Number(e.target.value));
$('part-asset').onchange = e => changePartPose('assetId', e.target.value);
$('part-visible').onchange = e => changePartPose('visible', e.target.checked);
$('save-part-key').onclick = () => {
  if (!isParts() || !selectedPart()) return;
  const id = selectedPartId, tick = selectedTick, pose = partPose(selectedPart(), action(), tick);
  edit(p => setPartKey(p.actions.find(a => a.id === selectedActionId), id, tick, pose), `已记录第 ${tick} tick 的关键帧。`);
};
$('delete-part-key').onclick = () => {
  const id = selectedPartId, tick = selectedTick;
  edit(p => {
    const a = p.actions.find(a => a.id === selectedActionId);
    a.tracks = a.tracks.map(track => track.partId === id ? {...track, keys: track.keys.filter(key => key.tick !== tick)} : track).filter(track => track.keys.length);
  });
};
function reorderPart(direction) {
  const id = selectedPartId, index = project.parts.findIndex(part => part.id === id), to = index + direction;
  if (to < 0 || to >= project.parts.length) return;
  edit(p => { p.parts.splice(to, 0, p.parts.splice(index, 1)[0]); });
}
$('part-back').onclick = () => reorderPart(-1); $('part-front').onclick = () => reorderPart(1);
$('delete-part').onclick = async () => {
  const id = selectedPartId;
  if (!id) return;
  if (project.parts.length === 1 && project.actions.some(a => a.kind === 'parts')) return toast('部件动作至少需要一个部件。请先把相关动作切换为整帧动画。');
  if (!await confirmChange('删除这个部件？', '它的动画轨道会一并删除；子部件解除父级关系。操作可撤销。')) return;
  edit(p => removePart(p, id));
};
$('action-default').onchange = e => {
  if (!e.target.checked && selectedActionId === project.defaultActionId) { e.target.checked = true; return toast('工程需要保留一个默认动作。'); }
  edit(p => { p.defaultActionId = selectedActionId; });
};
for (const [input, field] of [['frame-ticks', 'ticks'], ['frame-x', 'x'], ['frame-y', 'y']]) {
  $(input).onchange = e => { const id = selectedActionId, index = selectedFrame; edit(p => { p.actions.find(a => a.id === id).frames[index][field] = Number(e.target.value); }); };
}
$('frame-left').onclick = () => moveFrame(selectedFrame, selectedFrame - 1);
$('frame-right').onclick = () => moveFrame(selectedFrame, selectedFrame + 1);
$('duplicate-frame').onclick = () => bulkTimeline.run('duplicate');
$('delete-frame').onclick = () => bulkTimeline.run('delete');
$('add-action').onclick = () => {
  if (project.actions.length >= 64) return toast('一个工程最多 64 个动作。');
  const id = uid('action'), first = clone(frame());
  first.id = uid('frame'); first.ticks = 6;
  edit(p => p.actions.push({id, name: `新动作 ${p.actions.length + 1}`, loop: true, frames: [first]}));
  selectAction(id); panel = 'animation'; render();
};
$('add-standing-dance').onclick = async () => {
  const button = $('add-standing-dance'); button.disabled = true;
  const sourceProject = project;
  try {
    if (project.actions.length >= 64) throw new Error('一个工程最多 64 个动作。');
    const response = await fetch('/api/presets/standing-dance'), preset = await response.json();
    if (!response.ok) throw new Error(preset.error || '无法读取舞蹈动作');
    if (project !== sourceProject) throw new Error('工程已变化，请重新添加舞蹈。');
    const sources = new Map(project.assets.map(asset => [asset.src, asset.id]));
    const assets = [], mapping = new Map();
    for (const asset of preset.assets) {
      let id = sources.get(asset.src);
      if (!id) { id = uid('dance-png'); assets.push({...asset, id}); sources.set(asset.src, id); }
      mapping.set(asset.id, id);
    }
    const id = uid('dance');
    const next = {...preset.action, id, frames: preset.action.frames.map(f => ({...f, id: uid('frame'), assetId: mapping.get(f.assetId)}))};
    mode = 'edit';
    if (edit(p => { p.assets.push(...assets); p.actions.push(next); }, '已添加站立舞蹈 · 1.60 秒循环，可继续编辑。')) {
      playing = true; selectAction(id); panel = 'animation'; render();
    }
  } catch (error) { toast(error.message); }
  finally { button.disabled = false; }
};
$('delete-action').onclick = async () => {
  if (project.actions.length <= 1) return;
  const id = selectedActionId;
  const linked = project.rules.filter(r => r.actionId === id || r.fromActionId === id).length;
  if (!await confirmChange(`删除“${action().name}”？`, `相关的 ${linked} 条互动规则也会删除。操作后可撤销。`)) return;
  edit(p => { p.actions = p.actions.filter(a => a.id !== id); p.rules = p.rules.filter(r => r.actionId !== id && r.fromActionId !== id); if (p.defaultActionId === id) p.defaultActionId = p.actions[0].id; });
};
$('add-rule').onclick = () => {
  if (project.rules.length >= 128) return toast('一个工程最多 128 条规则。');
  const id = uid('rule'); selectedRuleId = id;
  edit(p => p.rules.push({id, name: '新的互动', event: 'click', regionId: '*', fromActionId: '*', actionId: selectedActionId, cooldown: 25, priority: 5, enabled: true, returnToDefault: true}));
  $('rule-editor').scrollIntoView({block: 'nearest', behavior: 'smooth'});
};
$('rule-list').onclick = e => { const button = e.target.closest('[data-rule]'); if (button) { selectedRuleId = button.dataset.rule; renderInspector(); } };
$('rule-editor').onchange = e => {
  const field = e.target.dataset.ruleField; if (!field) return;
  const value = e.target.type === 'checkbox' ? e.target.checked : e.target.type === 'number' ? Number(e.target.value) : e.target.value;
  const id = selectedRuleId;
  edit(p => { p.rules.find(r => r.id === id)[field] = value; });
};
$('rule-editor').onclick = e => {
  if (e.target.closest('#test-rule')) {
    const rule = clone(selectedRule());
    if (mode !== 'test') setMode('test');
    if (rule.fromActionId !== '*') preview.play(rule.fromActionId, '设置测试前置动作');
    event(rule.event, rule.regionId);
    toast('已触发所选事件；匹配规则仍按条件、冷却和优先级选择。');
  } else if (e.target.closest('#remove-rule')) {
    const id = selectedRuleId;
    edit(p => { p.rules = p.rules.filter(r => r.id !== id); });
  }
};
$('region-list').onchange = e => {
  const id = e.target.dataset.regionName; if (id) edit(p => { p.regions.find(r => r.id === id).name = e.target.value.trim() || '触摸区域'; });
};
$('region-list').onclick = async e => {
  const button = e.target.closest('[data-remove-region]'); if (!button) return;
  const id = button.dataset.removeRegion;
  const linked = project.rules.filter(r => r.regionId === id).length;
  if (linked && !await confirmChange('删除触摸区域？', `同时删除关联的 ${linked} 条互动规则。操作后可撤销。`)) return;
  edit(p => { p.regions = p.regions.filter(r => r.id !== id); p.rules = p.rules.filter(r => r.regionId !== id); });
};
$('add-region').onclick = beginDrawing; $('draw-region').onclick = beginDrawing;

$('pet-canvas').addEventListener('pointerdown', e => {
  if (e.button === 0 && mode === 'edit' && isParts() && !drawing && e.target.closest('[data-preview-part]')) {
    const id = selectedPartId ?? e.target.closest('[data-preview-part]').dataset.previewPart;
    selectedPartId = id; selectedTick = Math.min(totalTicks(action()), preview.elapsed);
    playing = false; panel = 'parts';
    const part = selectedPart(), pose = partPose(part, action(), selectedTick);
    const parent = composeParts(project, action(), selectedTick).find(p => p.id === part.parentId);
    pointer = {partId: id, start: point(e), pose, parentMatrix: parent?.matrix ?? [1,0,0,1,0,0], moved: false};
    partDraft = clone(project);
    e.preventDefault(); $('pet-canvas').setPointerCapture(e.pointerId); render(); return;
  }
  if (e.button !== 0 || (!drawing && mode !== 'test')) return;
  e.preventDefault(); $('pet-canvas').setPointerCapture(e.pointerId);
  pointer = {start: point(e), clientX: e.clientX, clientY: e.clientY, region: hitRegion(project.regions, point(e).x, point(e).y), dragging: false};
  if (drawing) { $('drawing-rect').hidden = false; $('drawing-rect').setAttribute('width', 0); $('drawing-rect').setAttribute('height', 0); }
  else preview.input();
});
$('pet-canvas').addEventListener('pointermove', e => {
  const pt = point(e);
  if (pointer?.partId) {
    const [a,b,c,d] = pointer.parentMatrix, det = a*d-b*c;
    const dx = pt.x-pointer.start.x, dy = pt.y-pointer.start.y;
    const pose = {...pointer.pose, x: pointer.pose.x + (d*dx-c*dy)/det, y: pointer.pose.y + (-b*dx+a*dy)/det};
    pose.x = Math.max(-256, Math.min(256, Math.round(pose.x * 1000) / 1000));
    pose.y = Math.max(-256, Math.min(256, Math.round(pose.y * 1000) / 1000));
    pointer.moved ||= Math.hypot(dx,dy) > .5;
    pointer.nextPose = pose;
    writePose(partDraft, pointer.partId, pose);
    renderDynamic(); return;
  }
  if (drawing && pointer) {
    const rect = $('drawing-rect');
    rect.setAttribute('x', Math.min(pointer.start.x, pt.x)); rect.setAttribute('y', Math.min(pointer.start.y, pt.y));
    rect.setAttribute('width', Math.abs(pointer.start.x - pt.x)); rect.setAttribute('height', Math.abs(pointer.start.y - pt.y));
    return;
  }
  if (mode !== 'test' || drawing) return;
  preview.input();
  if (pointer) {
    const dx = e.clientX - pointer.clientX, dy = e.clientY - pointer.clientY;
    if (!pointer.dragging && Math.hypot(dx, dy) > 8) { pointer.dragging = true; event('drag', pointer.region); }
    if (pointer?.dragging) $('pet-wrap').style.transform = `translate(${Math.max(-100, Math.min(100, dx))}px,${Math.max(-100, Math.min(100, dy)) - 20}px)`;
    return;
  }
  const {region, entered} = hover.move(project.regions, pt.x, pt.y);
  if (region === null) { petDistance = 0; lastHoverPoint = null; return; }
  if (entered) { petDistance = 0; event('enter', region); }
  if (lastHoverPoint) petDistance += Math.hypot(pt.x - lastHoverPoint.x, pt.y - lastHoverPoint.y);
  lastHoverPoint = pt;
  if (petDistance >= 15 && preview.tick - lastPet >= 9) {
    petDistance = 0; lastPet = preview.tick; event('pet', region);
  }
});
$('pet-canvas').addEventListener('pointerup', e => {
  if (!pointer) return;
  const saved = pointer, end = point(e); pointer = null;
  $('pet-wrap').style.transform = '';
  if (saved.partId) {
    partDraft = null;
    if (saved.moved) edit(p => writePose(p, saved.partId, saved.nextPose));
    else render();
    return;
  }
  if (drawing) {
    drawing = false; $('drawing-rect').hidden = true;
    const x = Math.min(127, Math.round(Math.min(saved.start.x, end.x))), y = Math.min(127, Math.round(Math.min(saved.start.y, end.y)));
    const width = Math.min(128 - x, Math.round(Math.abs(end.x - saved.start.x))), height = Math.min(128 - y, Math.round(Math.abs(end.y - saved.start.y)));
    if (width < 3 || height < 3) { render(); return toast('触摸区域至少需要 3 × 3 像素，请重新绘制。'); }
    edit(p => p.regions.push({id: uid('region'), name: `区域 ${p.regions.length + 1}`, x, y, width, height}), '触摸区域已添加，可在规则里选择它。');
  } else if (mode === 'test') event(saved.dragging ? 'release' : 'click', saved.region);
});
$('pet-canvas').addEventListener('pointercancel', () => { pointer = null; partDraft = null; $('pet-wrap').style.transform = ''; $('drawing-rect').hidden = true; renderDynamic(); });
$('pet-canvas').addEventListener('pointerleave', () => { hover.reset(); petDistance = 0; lastHoverPoint = null; });
$('toggle-log').onclick = () => { $('log-panel').hidden = !$('log-panel').hidden; renderDynamic(); };
$('close-log').onclick = () => { $('log-panel').hidden = true; };
$('import-images').onclick = () => $('image-files').click();
$('image-files').onchange = async e => { await importImages([...e.target.files]); e.target.value = ''; };
$('import-folder').onclick = () => $('folder-files').click();
$('folder-files').onchange = async e => { await importImages([...e.target.files]); e.target.value = ''; };
$('open-project').onclick = () => $('project-file').click();
$('project-file').onchange = async e => {
  const file = e.target.files[0]; e.target.value = ''; if (!file) return;
  try {
    if (file.size > 24 * 1024 * 1024) throw new Error('工程大小需在 24 MB 以内。');
    const next = JSON.parse(await file.text()); validateProject(next);
    if (!await confirmChange('打开另一个工程？', '当前工程会被替换。需要保留时，请先取消并点击“保存工程”。')) return;
    await openProject(next); toast('工程已打开。');
  } catch (error) { toast(`无法打开工程：${error.message}`); }
};
$('save-project').onclick = () => { download(new Blob([JSON.stringify(project, null, 2)], {type: 'application/json'}), 'shimeji2-project.json'); toast('工程已下载，包含动作、规则和自定义素材。'); };
$('reset-demo').onclick = async () => {
  if (!await confirmChange('重新打开白色 shime 示例？', '当前工程会被替换。需要保留时，请先保存工程。')) return;
  try { await openProject(await (await fetch('/api/demo')).json()); } catch (error) { toast(error.message); }
};
$('export-pack').onclick = () => { $('export-dialog').showModal(); };
$('confirm-export').onclick = async () => {
  const button = $('confirm-export'); button.disabled = true; button.textContent = '正在打包…';
  try {
    const response = await fetch('/api/export', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify(project)});
    if (!response.ok) throw new Error((await response.json()).error || '导出失败');
    download(await response.blob(), 'shimeji2-editor-pack.zip');
    $('export-dialog').close(); toast('动作包已下载，可在 shimeji2 的宠物设置中导入。');
  } catch (error) { toast(`导出失败：${error.message}`); }
  finally { button.disabled = false; button.textContent = '下载动作包 ZIP ↗'; }
};
document.addEventListener('keydown', e => {
  if (e.key === 'Escape' && drawing) { drawing = false; pointer = null; $('drawing-rect').hidden = true; render(); return; }
  if (['INPUT', 'TEXTAREA', 'SELECT'].includes(e.target.tagName) || document.querySelector('dialog[open]')) return;
  if (bulkTimeline?.handleKey(e)) return;
  if (e.code === 'Space') { e.preventDefault(); $('play').click(); }
  else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') { e.preventDefault(); history(e.shiftKey ? redo : undo, e.shiftKey ? undo : redo); }
  else if (e.key === 'ArrowLeft') { e.preventDefault(); if (isParts()) seek(selectedTick - 1); else selectFrame(Math.max(0, selectedFrame - 1), e); }
  else if (e.key === 'ArrowRight') { e.preventDefault(); if (isParts()) seek(selectedTick + 1); else selectFrame(Math.min(action().frames.length - 1, selectedFrame + 1), e); }
});
document.addEventListener('dragover', e => { if (e.dataTransfer.types.includes('Files')) e.preventDefault(); });
document.addEventListener('drop', e => {
  if (!e.dataTransfer.files.length) return;
  e.preventDefault();
  const files = [...e.dataTransfer.files];
  if (files.some(file => /\.(vrm|vrma)$/i.test(file.name))) vrmStudio?.open(files).catch(error => toast(error.message));
  else importImages(files);
});
document.addEventListener('visibilitychange', () => { previousTime = 0; accumulator = 0; });

function animate(now) {
  if (previousTime && playing && !document.hidden && !document.querySelector('dialog[open]')) {
    accumulator += Math.min(now - previousTime, 200) * Number($('speed').value);
    const count = Math.floor(accumulator / TICK_MS);
    if (count) {
      accumulator -= count * TICK_MS;
      const before = preview.actionId;
      const range = mode === 'edit' ? bulkTimeline?.playbackRange() : null;
      if (range) { preview.tick += count; preview.elapsed = advanceLoopTick(preview.elapsed, count, range); }
      else preview.step(count, mode === 'test');
      if (!range && mode === 'edit' && !preview.action.loop && preview.elapsed === totalTicks(preview.action) - 1) playing = false;
      if (mode === 'test' && before !== preview.actionId) { selectedActionId = preview.actionId; selectedFrame = 0; selectedTick = 0; render(); }
      else renderDynamic();
    }
  }
  previousTime = now;
  requestAnimationFrame(animate);
}

try {
  bulkTimeline = setupFrameTimeline({
    getState: () => ({action: action(), selectedFrame, mode, assetIds:project.assets.map(a => a.id)}), toast, idFactory: uid,
    onSelect: selectFrame, onSelectionChange: () => render(),
    onPlayRange: () => { const range = bulkTimeline.playbackRange(); if (range) { preview.elapsed = range.start; playing = true; renderDynamic(); } },
    onCreateAction: copies => {
      if (project.actions.length >= 64) return toast('一个工程最多 64 个动作。');
      const id = uid('action');
      const created = copies.map(frame => ({id: uid('frame'), assetId: frame.assetId, ticks: frame.ticks, x: frame.x ?? 0, y: frame.y ?? 0}));
      if (!edit(p => { p.actions.push({id, name: `${action().name} 片段`.slice(0, 80), kind: 'frames', loop: true, frames: created}); }, '已把连续选区建成动作 · 可撤销。')) return;
      selectAction(id);
    },
    onFrames: result => {
      const id = selectedActionId, before = selectedFrame, previous = bulkTimeline.selection();
      selectedFrame = result.frames.findIndex(f => f.id === result.focusId);
      bulkTimeline.setSelection(result.selectedIds); playing = false; mode = 'edit';
      if (!edit(p => { p.actions.find(a => a.id === id).frames = result.frames; }, `已更新 ${result.selectedIds.length} 个选中帧 · 可撤销。`)) {
        selectedFrame = before; bulkTimeline.setSelection(previous); render();
      }
    },
  });
  const pixelPaint = setupPixelPaint({toast,
    getFrame: () => {
      if (isParts()) return null;
      const current = frame();
      return {frameId: current.id, src: asset(current.assetId).src, shared: assetUseCount(project, current.assetId)};
    },
    onApply: (frameId, src) => {
      const actionId = selectedActionId;
      edit(p => { applyFramePixels(p, actionId, frameId, src, uid); }, '已写回当前帧 · 可撤销。');
    },
  });
  $('paint-frame').onclick = () => pixelPaint.open();
  sequenceImport = setupSequenceImport({toast,
    getContext: () => ({projectToken:project,kind:action().kind,assetCount:project.assets.length,frameCount:action().frames.length,partCount:project.parts.length,actionCount:project.actions.length}),
    onImport: payload => {
      const currentId = selectedActionId;
      const prepared = prepareMediaImport(project,payload,currentId,uid);
      mode = 'edit'; playing = false;
      const accepted = edit(p => {
        prepared.apply(p);
        if (prepared.parts) {
          for (const item of prepared.partAssets) {
            const part = {...makePart(item.id), name: item.name.replace(/\.png$/i, '')};
            if (prepared.pivot && prepared.partAssets.length === 1) {
              part.pivotX = Math.max(-256, Math.min(256, prepared.pivot.x));
              part.pivotY = Math.max(-256, Math.min(256, prepared.pivot.y));
            }
            p.parts.push(part);
          }
        }
      },payload.destination === 'replace' ? '已替换图片，动作时长与引用保持不变。' : `已导入素材组“${payload.groupName}” · 可撤销。`);
      if (!accepted) throw new Error('工程没有应用此次导入，请检查素材和帧数上限。');
      selectAction(prepared.selectedActionId);
    },
  });
  authoringPreview = setupAuthoringPreview({getState: () => ({project,action:preview.action,tick:preview.elapsed,mode,partDraft})});
  let saved;
  try { saved = JSON.parse(localStorage.getItem(STORAGE) || 'null'); if (saved) validateProject(saved); }
  catch { toast('本地工程无法读取，已打开内置示例。'); saved = null; }
  await openProject(saved || await (await fetch('/api/demo')).json());
  assetLibrary = setupAssetLibrary({toast,idFactory:uid,
    getState: () => ({project,currentActionId:selectedActionId}),
    onEdit: edit,
    onReplace: (assetId,files) => sequenceImport.open(files,{replaceAssetId:assetId}),
    onInsert: (assetIds,options = {}) => {
      const id = selectedActionId;
      if (!Array.isArray(assetIds) || !assetIds.length || assetIds.some(id => !asset(id))) throw new Error('请选择有效素材。');
      if (isParts() && options.frames) throw new Error('整组动画请追加到整帧动作。');
      const groupFrames = options.frames ?? assetIds.map(assetId => ({assetId,ticks:6}));
      if (groupFrames.some(f => !asset(f.assetId) || !Number.isInteger(f.ticks) || f.ticks < 1 || f.ticks > 1500)) throw new Error('素材组帧或时长无效。');
      if (isParts() ? project.parts.length + assetIds.length > 32 : action().frames.length + groupFrames.length > 256) throw new Error('添加后会超过部件或动作帧数上限。');
      mode = 'edit'; playing = false;
      const accepted = edit(p => {
        if (isParts()) for (const assetId of assetIds) p.parts.push(makePart(assetId));
        else p.actions.find(a => a.id === id).frames.push(...groupFrames.map(f => ({...f,id:uid('frame'),x:0,y:0})));
      },`已添加 ${isParts() ? assetIds.length : groupFrames.length} ${isParts() ? '个部件' : '帧'} · 可撤销。`);
      if (!accepted) throw new Error('未添加素材。');
      lastAssetId = assetIds.at(-1);
    },
  });
  vrmStudio = setupVrmStudio({toast,
    getContext: () => ({projectToken:project,assetCount:project.assets.length,actionCount:project.actions.length}),
    onBake: payload => {
      const prepared = prepareVrmAction(project, payload, uid);
      const accepted = edit(p => { p.assets.push(...prepared.assets); p.actions.push(prepared.action); });
      if (!accepted) throw new Error('烘焙未加入工程，请检查素材和动作上限。');
      mode = 'edit'; playing = true; panel = 'animation'; selectAction(prepared.action.id); render();
      toast(`已添加 ${prepared.action.frames.length} 帧 VRM 动作 · 可撤销。`);
    },
  });
  $('open-vrm').onclick = () => vrmStudio.open().catch(error => toast(error.message));
  setupCatalog({openProject, confirmChange, toast});
  requestAnimationFrame(animate);
} catch (error) {
  $('status-text').textContent = `无法启动编辑器：${error.message}`;
  toast('启动失败，请检查本地服务并刷新页面。');
}
