import {applyFrameOperation, copySelectedFrames, framesForAction} from './timeline-ops.mjs';

export function setupFrameTimeline({getState, onFrames, onSelect, onSelectionChange, onPlayRange, onCreateAction, toast, idFactory}) {
  const $ = id => document.getElementById(id);
  const tools = document.createElement('div');
  tools.id = 'bulk-timeline';
  tools.innerHTML = `<div class="bulk-selection"><strong id="bulk-count"></strong><span class="muted">Shift 连选 · ⌘/Ctrl 多选 · 空白处拖框</span><button id="bulk-all" class="quiet small">全选</button></div>
    <div class="bulk-buttons"><button id="bulk-copy" class="small">拷贝</button><button id="bulk-paste" class="small">粘贴</button><button id="bulk-reverse" class="small">倒序</button><button id="bulk-pingpong" class="small" title="ABC → ABCB，首尾不重复">往返</button><button id="bulk-make-action" class="small">建成动作</button><select id="bulk-action-direction" aria-label="新动作方向"><option value="forward">正序</option><option value="reverse">倒序</option><option value="pingpong">往返</option></select><label>每帧 <input id="bulk-duration" type="number" min="40" max="60000" step="40" value="240" aria-label="所选帧统一时长毫秒"> ms</label><button id="bulk-duration-apply" class="small">应用</button><select id="bulk-speed" aria-label="所选片段调速"><option value="">片段调速…</option><option value="2">加速至 2×</option><option value="0.5">减速至 0.5×</option></select><button id="bulk-speed-apply" class="small">调速</button><label class="range-label"><input id="loop-selection" type="checkbox">循环所选片段</label></div>`;
  document.querySelector('.timeline-toolbar').after(tools);
  let actionId, ids = new Set(), anchorId, clipboard = [], loop = false;
  const frames = () => getState().action.frames;
  const selection = () => frames().filter(f => ids.has(f.id)).map(f => f.id);
  const contiguous = () => {
    const indices = frames().flatMap((f, i) => ids.has(f.id) ? [i] : []);
    return indices.length > 0 && indices.at(-1) - indices[0] + 1 === indices.length;
  };
  function reset(clearClipboard = false) { actionId = undefined; ids.clear(); anchorId = undefined; loop = false; if (clearClipboard) clipboard = []; }
  function sync() {
    const state = getState(), list = state.action.frames;
    tools.hidden = state.action.kind === 'parts';
    if (state.action.id !== actionId) { reset(); actionId = state.action.id; }
    ids = new Set([...ids].filter(id => list.some(f => f.id === id)));
    if (!ids.size) ids.add(list[Math.min(state.selectedFrame, list.length - 1)].id);
    if (!list.some(f => f.id === anchorId)) anchorId = list[state.selectedFrame]?.id;
    const selected = list.filter(f => ids.has(f.id));
    $('bulk-count').textContent = `已选 ${selected.length} / ${list.length} 帧`;
    if (state.assetIds && clipboard.some(frame => !state.assetIds.includes(frame.assetId))) clipboard = [];
    $('bulk-paste').disabled = !clipboard.length;
    $('bulk-pingpong').disabled = !contiguous() || selected.length < 3;
    $('bulk-make-action').disabled = !contiguous();
    $('bulk-copy').disabled = !selected.length;
    if (document.activeElement !== $('bulk-duration')) {
      $('bulk-duration').value = selected.every(f => f.ticks === selected[0].ticks) ? selected[0].ticks * 40 : '';
      $('bulk-duration').placeholder = '多种时长';
    }
    if (!contiguous() || state.mode !== 'edit') loop = false;
    $('loop-selection').disabled = !contiguous() || state.mode !== 'edit';
    $('loop-selection').checked = loop;
  }
  function select(index, event = {}) {
    if (event.preserveSelection) return;
    const list = frames(), id = list[index].id;
    if (event.shiftKey && list.some(f => f.id === anchorId)) {
      const anchor = list.findIndex(f => f.id === anchorId);
      if (!event.metaKey && !event.ctrlKey) ids.clear();
      list.slice(Math.min(anchor, index), Math.max(anchor, index) + 1).forEach(f => ids.add(f.id));
    } else if (event.metaKey || event.ctrlKey) {
      if (ids.has(id) && ids.size > 1) ids.delete(id); else ids.add(id);
      anchorId = id;
    } else { ids = new Set([id]); anchorId = id; }
  }
  function run(operation, options = {}) {
    if (getState().action.kind === 'parts') return;
    try {
      const result = applyFrameOperation(frames(), selection(), operation, {...options, clipboard, idFactory});
      onFrames(result, operation);
    } catch (error) { toast(error.message); }
  }
  function copy() {
    if (getState().action.kind === 'parts') return;
    clipboard = copySelectedFrames(frames(), selection());
    toast(`已拷贝 ${clipboard.length} 帧，可切换动作后粘贴。`); sync();
  }
  $('bulk-copy').onclick = copy;
  $('bulk-paste').onclick = () => run('paste');
  $('bulk-reverse').onclick = () => run('reverse');
  $('bulk-pingpong').onclick = () => run('pingpong');
  $('bulk-make-action').onclick = () => {
    try { onCreateAction?.(framesForAction(frames(), selection(), $('bulk-action-direction').value)); }
    catch (error) { toast(error.message); }
  };
  $('bulk-all').onclick = () => { ids = new Set(frames().map(f => f.id)); onSelectionChange(); };
  $('bulk-duration-apply').onclick = () => {
    const value = Number($('bulk-duration').value);
    if (!Number.isFinite(value) || value < 40 || value > 60000) return toast('每帧时长需要为 40–60000 ms。');
    const ticks = Math.round(value / 40);
    run('duration', {ticks});
    if (ticks * 40 !== value) toast(`已对齐 40 ms 时间网格：每帧 ${ticks * 40} ms。`);
  };
  $('bulk-speed-apply').onclick = () => { const factor = Number($('bulk-speed').value); if (factor) run('speed', {factor}); else toast('请先选择片段速度。'); };
  $('loop-selection').onchange = e => { loop = e.target.checked; if (loop) onPlayRange(); };
  function playbackRange() {
    if (!loop || !contiguous() || getState().mode !== 'edit' || getState().action.kind === 'parts') return null;
    let tick = 0, start, end;
    for (const f of frames()) { if (ids.has(f.id)) { start ??= tick; end = tick + f.ticks; } tick += f.ticks; }
    return {start, end};
  }
  function handleKey(e) {
    if (getState().action.kind === 'parts') return false;
    const modifier = e.metaKey || e.ctrlKey;
    if (modifier && e.key.toLowerCase() === 'a') { ids = new Set(frames().map(f => f.id)); onSelectionChange(); }
    else if (modifier && e.key.toLowerCase() === 'c') copy();
    else if (modifier && e.key.toLowerCase() === 'v') run('paste');
    else if (modifier && e.key.toLowerCase() === 'd') run('duplicate');
    else if (['Backspace', 'Delete'].includes(e.key)) run('delete');
    else return false;
    e.preventDefault(); return true;
  }
  const scroll = $('timeline-scroll');
  let marquee;
  scroll.addEventListener('pointerdown', e => {
    if (getState().action.kind === 'parts' || e.button !== 0 || e.target.closest('button,input,label')) return;
    const overlay = document.createElement('div'); overlay.className = 'timeline-marquee'; document.body.append(overlay);
    marquee = {x:e.clientX, y:e.clientY, endX:e.clientX, endY:e.clientY, overlay, previous:new Set(ids), additive:e.metaKey || e.ctrlKey || e.shiftKey, moved:false};
    scroll.setPointerCapture(e.pointerId); e.preventDefault();
  });
  function updateMarquee(e) {
    if (!marquee) return;
    if (e) { marquee.endX = e.clientX; marquee.endY = e.clientY; }
    const left = Math.min(marquee.x,marquee.endX), top = Math.min(marquee.y,marquee.endY), right = Math.max(marquee.x,marquee.endX), bottom = Math.max(marquee.y,marquee.endY);
    if (right-left + bottom-top < 4) return;
    marquee.moved = true;
    Object.assign(marquee.overlay.style,{left:`${left}px`,top:`${top}px`,width:`${right-left}px`,height:`${bottom-top}px`});
    ids = marquee.additive ? new Set(marquee.previous) : new Set();
    scroll.querySelectorAll('[data-frame]').forEach(cell => {
      const r=cell.getBoundingClientRect(), id=frames()[Number(cell.dataset.frame)].id;
      if (r.right >= left && r.left <= right && r.bottom >= top && r.top <= bottom) ids.add(id);
      cell.classList.toggle('multi-selected',ids.has(id));
    });
  }
  scroll.addEventListener('pointermove', updateMarquee);
  scroll.addEventListener('scroll', () => updateMarquee(), {passive:true});
  function finishMarquee(cancelled = false, event) {
    if (!marquee) return;
    if (!cancelled) updateMarquee(event);
    const {overlay, previous, moved} = marquee; marquee = null; overlay.remove();
    if (cancelled || !moved || !ids.size) ids = previous;
    const index = frames().findIndex(f => ids.has(f.id));
    if (index >= 0) { anchorId = frames()[index].id; onSelect(index,{preserveSelection:true}); }
  }
  scroll.addEventListener('pointerup',e => finishMarquee(false,e));
  scroll.addEventListener('pointercancel',() => finishMarquee(true));
  return {sync,reset,select,isSelected:id=>ids.has(id),selection,run,handleKey,playbackRange,
    setSelection:next=>{ids=new Set(next);anchorId=next[0];}};
}
