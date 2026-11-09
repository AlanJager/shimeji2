import {naturalOrder, gridFrames, normalizationPlan, sequenceClock} from './sequence-model.mjs';
import {decodeMediaFiles} from './media-source.mjs';
import {quantizeDurations, timedFrameAt} from './media-timing.mjs';

const nextPaint = () => new Promise(resolve => setTimeout(resolve, 0));

export function setupSequenceImport({getContext, onImport, toast}) {
  const dialog = document.createElement('dialog');
  dialog.id = 'sequence-import-dialog';
  dialog.className = 'sequence-dialog';
  dialog.innerHTML = `
    <div class="sequence-header"><div><span class="eyebrow">IMPORT SEQUENCE</span><h2>导入动画序列</h2></div><button id="sequence-close" class="icon-button" aria-label="关闭序列导入">×</button></div>
    <p id="sequence-files-summary" class="sequence-summary"></p>
    <fieldset id="sequence-settings"><div class="sequence-layout"><section class="sequence-settings" aria-label="序列导入设置">
      <label>素材形式<select id="sequence-mode"><option value="batch">多张 PNG · 按文件名排序</option><option value="sheet">一张序列图 · 按网格切分</option></select></label>
      <div id="sequence-batch-settings"><label>文件顺序<select id="sequence-file-order"><option value="forward">自然顺序 · 1、2、10</option><option value="reverse">自然顺序的倒序</option></select></label></div>
      <div id="sequence-sheet-settings" hidden>
        <div class="sequence-two"><label>单格宽度 / px<input id="sequence-cell-width" type="number" min="1" max="4096" step="1" value="128"></label><label>单格高度 / px<input id="sequence-cell-height" type="number" min="1" max="4096" step="1" value="128"></label></div>
        <div class="sequence-two"><label>起始 X / px<input id="sequence-offset-x" type="number" min="0" max="4096" step="1" value="0"></label><label>起始 Y / px<input id="sequence-offset-y" type="number" min="0" max="4096" step="1" value="0"></label></div>
        <div class="sequence-two"><label>横向间隔 / px<input id="sequence-gap-x" type="number" min="0" max="4096" step="1" value="0"></label><label>纵向间隔 / px<input id="sequence-gap-y" type="number" min="0" max="4096" step="1" value="0"></label></div>
        <div class="sequence-two"><label>读取顺序<select id="sequence-grid-order"><option value="rows">逐行 · 从左到右</option><option value="columns">逐列 · 从上到下</option></select></label><label>导入帧数<input id="sequence-count" type="number" min="1" max="256" step="1" placeholder="全部完整格子"></label></div>
        <p id="sequence-grid-summary" class="sequence-help"></p>
      </div>
      <div class="sequence-separator"></div>
      <label>统一缩放<select id="sequence-scale-mode"><option value="fit">适配画布 · 仅缩小，不放大小帧</option><option value="original">原始像素 · 100%</option><option value="custom">指定统一比例</option></select></label>
      <label id="sequence-scale-field" hidden>缩放比例 / %<input id="sequence-scale" type="number" min="1" max="400" step="1" value="100"></label>
      <label>源图片定位点<select id="sequence-source-anchor"><option value="bottom-center">底部居中</option><option value="center">正中心</option><option value="top-left">左上角</option></select></label>
      <div class="sequence-two"><label>目标锚点 X / px<input id="sequence-anchor-x" type="number" min="0" max="128" step="1" value="64"></label><label>目标锚点 Y / px<input id="sequence-anchor-y" type="number" min="0" max="128" step="1" value="128"></label></div>
      <p class="sequence-help">整组共用一个缩放比例与目标锚点；保留帧之间原有的大小差异。画布固定为 128 × 128。</p>
    </section>
    <section class="sequence-preview" aria-label="归一化预览">
      <div class="sequence-preview-title"><h3>导入后的帧</h3><span id="sequence-preview-label"></span></div>
      <div class="sequence-canvas-wrap"><canvas id="sequence-preview-canvas" width="128" height="128" aria-label="导入帧预览"></canvas></div>
      <div class="sequence-scrub"><button id="sequence-play" type="button" class="sequence-play" aria-label="播放导入预览">▶</button><input id="sequence-preview-index" type="range" min="0" max="0" value="0" step="1" aria-label="选择导入预览帧"></div>
      <p id="sequence-scale-summary" class="sequence-help"></p>
      <p id="sequence-clip-warning" class="sequence-warning" hidden></p>
      <div id="sequence-source-sheet" hidden><h3>序列图切分位置</h3><canvas id="sequence-sheet-canvas" width="256" height="128" aria-label="序列图网格预览"></canvas></div>
      <ol id="sequence-order-preview" class="sequence-order-preview" aria-label="导入帧顺序"></ol>
    </section></div>
    <div class="sequence-destination"><label>每帧停留 / tick<input id="sequence-ticks" type="number" min="1" max="1500" step="1" value="6"></label><label>导入到<select id="sequence-destination"><option value="append">追加到当前动作</option><option value="new">新建整帧动作</option></select></label><label id="sequence-name-field" hidden>动作名称<input id="sequence-name" maxlength="80" value="导入动作"></label></div></fieldset>
    <p id="sequence-status" class="sequence-status" role="status"></p>
    <div class="dialog-actions"><button id="sequence-cancel" type="button">取消</button><button id="sequence-confirm" type="button" class="primary" disabled>导入动画帧</button></div>`;
  document.body.append(dialog);
  const $ = id => dialog.querySelector('#' + id);
  const motionSettings=document.createElement('div');motionSettings.id='sequence-motion-settings';
  motionSettings.innerHTML=`<label>动作标签<select id="sequence-clip-select"></select></label><label id="sequence-preview-clip-field" hidden>预览标签<select id="sequence-preview-clip"></select></label><label class="sequence-check"><input id="sequence-preserve-timing" type="checkbox">保留素材原始帧时长</label><label>循环方式<select id="sequence-loop"><option value="source">沿用素材设置</option><option value="true">循环播放</option><option value="false">播放一次</option></select></label><p id="sequence-timing-summary" class="sequence-help"></p><p id="sequence-decoder-warning" class="sequence-warning" hidden></p>`;
  $('sequence-sheet-settings').after(motionSettings);
  $('sequence-mode').querySelector('[value="batch"]').textContent='图片 / 动画帧 · 保留完整画布';
  $('sequence-name-field').firstChild.textContent='素材组 / 动作名称';
  const replacement=document.createElement('option');replacement.value='replace';replacement.textContent='替换原素材 · 保留所有引用';$('sequence-destination').append(replacement);
  let state = null, generation = 0, previewRequest = 0, cachedBitmap = null, cachedIndex = -1;
  let autoPlay = false, raf = 0, lastTime = null;
  let ignoredCloseEvents = 0;
  let previewDecode = Promise.resolve();
  let decoder = null, playbackTick = 0, playbackAnimation = null;

  function stopPlayback() {
    autoPlay = false; cancelAnimationFrame(raf); raf = 0; lastTime = null; playbackAnimation=null;
    $('sequence-play').textContent = '▶'; $('sequence-play').setAttribute('aria-label', '播放导入预览');
  }

  function clearBitmap() {
    previewRequest++;
    cachedBitmap?.close(); cachedBitmap = null; cachedIndex = -1;
  }

  function reset() {
    generation++; decoder?.abort(); decoder=null;stopPlayback(); clearBitmap(); state = null;
    $('sequence-settings').disabled = false;
    $('sequence-confirm').disabled = true; $('sequence-confirm').textContent = '导入动画帧';
  }

  function close() {
    reset();
    if (dialog.open) { ignoredCloseEvents++; dialog.close(); }
  }
  dialog.addEventListener('close', () => { if (ignoredCloseEvents) ignoredCloseEvents--; else reset(); });
  dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
  $('sequence-close').onclick = close; $('sequence-cancel').onclick = close;

  function settings() {
    if (!state) throw new Error('素材尚未准备好。');
    const mode = $('sequence-mode').value;
    let frames, grid;
    if (mode === 'sheet') {
      if (state.sources.length !== 1) throw new Error('序列图模式需要只选择一张 PNG。');
      grid = gridFrames({width: state.sources[0].width, height: state.sources[0].height,
        cellWidth: Number($('sequence-cell-width').value), cellHeight: Number($('sequence-cell-height').value),
        offsetX: Number($('sequence-offset-x').value), offsetY: Number($('sequence-offset-y').value),
        gapX: Number($('sequence-gap-x').value), gapY: Number($('sequence-gap-y').value), order: $('sequence-grid-order').value,
        count: $('sequence-count').value === '' ? undefined : Number($('sequence-count').value)});
      if(!state.gridEligible)throw new Error('完整动画或图集帧不能再次按网格切分。');
      const base = state.sources[0].name.replace(/\.[^.]+$/, '');
      frames = grid.frames.map((frame, index) => ({...frame,durationMs:240,name: `${base}-${String(index + 1).padStart(3, '0')}.png`}));
    } else {
      const sources=state.animated?state.sources:naturalOrder(state.sources,$('sequence-file-order').value==='reverse');
      frames = sources.map(source => ({sourceIndex:source.sourceIndex,x:0,y:0,width:source.width,height:source.height,name:source.name,durationMs:source.durationMs}));
    }
    const plan = normalizationPlan(frames, {mode: $('sequence-scale-mode').value, scale: Number($('sequence-scale').value) / 100,
      anchorX: Number($('sequence-anchor-x').value), anchorY: Number($('sequence-anchor-y').value), sourceAnchor: $('sequence-source-anchor').value});
    const ticks = Number($('sequence-ticks').value), destination = $('sequence-destination').value, name = $('sequence-name').value.trim();
    if (!Number.isInteger(ticks) || ticks < 1 || ticks > 1500) throw new Error('每帧停留时间需要是 1–1500 tick。');
    if (!name || name.length > 80) throw new Error('素材组名称需要包含 1–80 个字符。');
    if(state.replaceAssetId && (destination!=='replace' || frames.length!==1))throw new Error('替换素材只接受一张静态图片。');
    if(!state.replaceAssetId && destination==='replace')throw new Error('请从素材库选择要替换的素材。');
    const preserve=$('sequence-preserve-timing').checked && mode!=='sheet';
    const timing=quantizeDurations(frames.map(frame=>preserve?frame.durationMs:ticks*40));
    const groupFrames=timing.ticks.map((ticks,assetIndex)=>({assetIndex,ticks}));
    const sourceToAsset=new Map(frames.map((frame,index)=>[frame.sourceIndex,index]));
    const selected=$('sequence-clip-select').value;
    let selectedClips=mode==='sheet'?[{name,indices:frames.map((_,index)=>index),loop:true}]:state.clips.filter((_,index)=>selected==='all'||String(index)===selected);
    if(!selectedClips.length)throw new Error('请选择一个动作标签。');
    const loop=$('sequence-loop').value;
    const clips=selectedClips.map(clip=>{
      let indices=mode==='sheet'?clip.indices:clip.indices.map(index=>sourceToAsset.get(index));
      if(!state.animated && mode!=='sheet')indices.sort((a,b)=>a-b);
      const times=quantizeDurations(indices.map(index=>preserve?frames[index].durationMs:ticks*40));
      times.originalMs=indices.reduce((sum,index)=>sum+frames[index].durationMs,0);
      times.changed=indices.some((assetIndex,index)=>frames[assetIndex].durationMs!==times.ticks[index]*40);
      return {name:selectedClips.length===1?name:clip.name,frames:indices.map((assetIndex,index)=>({assetIndex,ticks:times.ticks[index]})),loop:loop==='source'?clip.loop:loop==='true',timing:times};
    });
    const context=getContext();
    if(context.projectToken!==state.projectToken)throw new Error('桌宠工程已变化，请关闭并重新选择素材。');
    if(destination==='append' && (clips.length!==1 || context.kind==='parts'&&state.animated))throw new Error('动画和多个标签请新建整帧动作，不能堆叠为部件。');
    if(destination==='append' && context.kind==='parts' && context.partCount+new Set(clips[0].frames.map(frame=>frame.assetIndex)).size>32)throw new Error('添加后部件会超过 32 个。');
    if(destination==='append' && context.kind!=='parts' && context.frameCount+clips[0].frames.length>256)throw new Error('当前动作最多 256 帧。');
    if(destination==='new' && context.actionCount+clips.length>64)throw new Error('导入全部标签后动作会超过 64 个，请减少标签或整理工程。');
    const previewIndex=selected==='all'?Number($('sequence-preview-clip').value):0;
    return {mode,frames,grid,plan,ticks,destination,name,clips,groupFrames,timing,preserve,previewClip:clips[previewIndex]??clips[0]};
  }

  function drawNormalized(bitmap, placement, canvas) {
    const ctx = canvas.getContext('2d');
    ctx.clearRect(0, 0, 128, 128); ctx.imageSmoothingEnabled = false;
    const source = placement.source;
    ctx.drawImage(bitmap, source.x, source.y, source.width, source.height, placement.x, placement.y, placement.width, placement.height);
  }

  function previewBitmap(index, activeGeneration, request) {
    // Rapid scrubbing queues at most one decode; stale jobs exit before decoding.
    const result = previewDecode.then(async () => {
      if (activeGeneration !== generation || request !== previewRequest || !state) return null;
      if (cachedBitmap && cachedIndex === index) return cachedBitmap;
      cachedBitmap?.close(); cachedBitmap = null; cachedIndex = -1;
      const bitmap = await createImageBitmap(state.files[index]);
      if (activeGeneration !== generation || request !== previewRequest || !state) { bitmap.close(); return null; }
      cachedBitmap = bitmap; cachedIndex = index;
      return bitmap;
    });
    previewDecode = result.then(() => undefined, () => undefined);
    return result;
  }

  async function preview() {
    const request = ++previewRequest, activeGeneration = generation;
    if (!state || !dialog.open || $('sequence-settings').disabled) return;
    $('sequence-sheet-settings').hidden = $('sequence-mode').value !== 'sheet';
    $('sequence-batch-settings').hidden = $('sequence-mode').value === 'sheet' || state.animated;
    $('sequence-motion-settings').hidden = $('sequence-mode').value === 'sheet' || Boolean(state.replaceAssetId);
    $('sequence-scale-field').hidden = $('sequence-scale-mode').value !== 'custom';
    $('sequence-name-field').hidden = Boolean(state.replaceAssetId);
    $('sequence-preview-clip-field').hidden=$('sequence-clip-select').value!=='all';
    $('sequence-ticks').disabled=$('sequence-preserve-timing').checked && $('sequence-mode').value!=='sheet';
    try {
      const config = settings();
      $('sequence-confirm').disabled = false;
      const clip=config.previewClip;
      $('sequence-status').textContent = `${config.frames.length} 张素材 · ${config.clips.length} 个动作 · 预览 ${(clip.timing.adjustedMs/1000).toFixed(2)} s`;
      $('sequence-timing-summary').textContent=`原时长 ${(clip.timing.originalMs/1000).toFixed(3)} s → 导入 ${(clip.timing.adjustedMs/1000).toFixed(3)} s${clip.timing.changed?(config.preserve?'；已对齐 40 ms 网格，每帧至少 1 tick。':'；使用所选的统一帧时长。'):'。'}`;
      $('sequence-status').classList.remove('error');
      $('sequence-preview-index').max = clip.frames.length - 1;
      const index = Math.min(Number($('sequence-preview-index').value), clip.frames.length - 1);
      $('sequence-preview-index').value = index;
      const assetIndex=clip.frames[index].assetIndex, frame = config.frames[assetIndex], placement = config.plan.placements[assetIndex];
      $('sequence-preview-label').textContent = `${index + 1} / ${clip.frames.length} · ${clip.frames[index].ticks*40} ms`;
      $('sequence-scale-summary').textContent = `统一 ${(config.plan.scale * 100).toFixed(2)}% · 源帧 ${frame.width} × ${frame.height} → ${placement.width.toFixed(1)} × ${placement.height.toFixed(1)} px`;
      $('sequence-clip-warning').hidden = config.plan.clippedCount === 0;
      $('sequence-clip-warning').textContent = `${config.plan.clippedCount} 帧会超出画布并被裁切，请检查缩放和目标锚点。`;
      $('sequence-source-sheet').hidden = config.mode !== 'sheet';
      if (config.grid) $('sequence-grid-summary').textContent = `${config.grid.columns} 列 × ${config.grid.rows} 行 · ${config.grid.total} 个完整格子，导入前 ${config.frames.length} 帧。`;
      const listing = $('sequence-order-preview'); listing.replaceChildren();
      clip.frames.slice(0, 12).forEach((entry, i) => { const li = document.createElement('li'); li.textContent = `${String(i + 1).padStart(2,'0')}  ${config.frames[entry.assetIndex].name} · ${entry.ticks*40} ms`; listing.append(li); });
      if (clip.frames.length > 12) { const li = document.createElement('li'); li.textContent = `… 共 ${clip.frames.length} 帧`; listing.append(li); }
      const bitmap = await previewBitmap(frame.sourceIndex, activeGeneration, request);
      if (!bitmap || activeGeneration !== generation || request !== previewRequest) return;
      drawNormalized(bitmap, placement, $('sequence-preview-canvas'));
      if (config.mode === 'sheet') {
        const canvas = $('sequence-sheet-canvas'), source = state.sources[0], ratio = Math.min(1, 300 / source.width, 160 / source.height);
        canvas.width = Math.max(1, Math.round(source.width * ratio)); canvas.height = Math.max(1, Math.round(source.height * ratio));
        const ctx = canvas.getContext('2d'); ctx.imageSmoothingEnabled = false; ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
        ctx.lineWidth = 1; ctx.strokeStyle = '#416b51';
        ctx.strokeRect(frame.x * ratio + .5, frame.y * ratio + .5, Math.max(1, frame.width * ratio - 1), Math.max(1, frame.height * ratio - 1));
      }
    } catch (error) {
      if (activeGeneration !== generation || request !== previewRequest) return;
      stopPlayback(); $('sequence-confirm').disabled = true;
      $('sequence-status').textContent = error.message; $('sequence-status').classList.add('error');
    }
  }

  $('sequence-clip-select').addEventListener('change',()=>{
    const all=$('sequence-clip-select').value==='all';
    $('sequence-destination').querySelector('[value="append"]').disabled=all || state?.animated&&getContext().kind==='parts';
    if(all)$('sequence-destination').value='new';
  });
  for (const element of dialog.querySelectorAll('input, select')) {
    element.addEventListener(element.tagName === 'SELECT' ? 'change' : 'input',()=>{stopPlayback();preview();});
  }
  $('sequence-play').onclick = () => {
    if (autoPlay) { stopPlayback(); return; }
    let config;try { config=settings(); } catch (error) { toast(error.message); return; }
    const clip=config.previewClip, total=clip.frames.reduce((sum,entry)=>sum+entry.ticks,0);
    playbackTick=clip.frames.slice(0,Number($('sequence-preview-index').value)).reduce((sum,entry)=>sum+entry.ticks,0);
    if(!clip.loop && Number($('sequence-preview-index').value)===clip.frames.length-1){playbackTick=0;$('sequence-preview-index').value=0;preview();}
    autoPlay = true; $('sequence-play').textContent = 'Ⅱ'; $('sequence-play').setAttribute('aria-label', '暂停导入预览');
    const animate = now => {
      if (!autoPlay || !dialog.open || document.hidden) return;
      const advance = sequenceClock(lastTime, now, 40); lastTime = advance.lastTime;
      if (advance.steps) {
        playbackTick+=advance.steps;
        const slider=$('sequence-preview-index'),index=timedFrameAt(clip.frames,playbackTick,clip.loop);
        if(Number(slider.value)!==index){slider.value=index;preview();}
        if(!clip.loop && playbackTick>=total){stopPlayback();return;}
      }
      raf = requestAnimationFrame(animate);
    };
    playbackAnimation=animate;raf = requestAnimationFrame(animate);
  };
  document.addEventListener('visibilitychange',()=>{cancelAnimationFrame(raf);raf=0;lastTime=null;if(!document.hidden&&autoPlay&&dialog.open&&playbackAnimation)raf=requestAnimationFrame(playbackAnimation);});

  $('sequence-confirm').onclick = async () => {
    if (!state || $('sequence-settings').disabled) return;
    const activeGeneration = generation;
    let config;
    try { config = settings(); } catch (error) { toast(error.message); return; }
    stopPlayback(); clearBitmap();
    $('sequence-settings').disabled = true; $('sequence-confirm').disabled = true;
    $('sequence-confirm').textContent = '正在整理动画帧…';
    const assets = [], files = state.files;
    const canvas = document.createElement('canvas'); canvas.width = 128; canvas.height = 128;
    let bitmap = null, sourceIndex = -1;
    try {
      for (let index = 0; index < config.frames.length; index++) {
        if (activeGeneration !== generation) return;
        const frame = config.frames[index];
        if (!bitmap || sourceIndex !== frame.sourceIndex) {
          bitmap?.close(); bitmap = null;
          bitmap = await createImageBitmap(files[frame.sourceIndex]); sourceIndex = frame.sourceIndex;
          if (activeGeneration !== generation) return;
        }
        drawNormalized(bitmap, config.plan.placements[index], canvas);
        assets.push({name: frame.name, src: canvas.toDataURL('image/png')});
        $('sequence-status').textContent = `正在整理 ${index + 1} / ${config.frames.length} 帧…`;
        if (index % 4 === 3) await nextPaint();
      }
      if (activeGeneration !== generation) return;
      if(getContext().projectToken!==state.projectToken)throw new Error('桌宠工程已变化，本次导入没有应用。');
      const result = await onImport({assets,clips:config.clips.map(({name,frames,loop})=>({name,frames,loop})),groupFrames:config.groupFrames,
        destination:config.destination,groupName:config.name,source:state.source,projectToken:state.projectToken,animated:state.animated,
        ...(state.replaceAssetId?{replaceAssetId:state.replaceAssetId}:{}),ticks:config.ticks,name:config.name});
      if (result === false) throw new Error('工程未接受这组素材，请调整设置后重试。');
      if (activeGeneration === generation) close();
    } catch (error) {
      if (activeGeneration !== generation) return;
      $('sequence-settings').disabled = false; $('sequence-confirm').disabled = false;
      $('sequence-confirm').textContent = state.replaceAssetId?'替换素材':'导入素材与动作';
      $('sequence-status').textContent = error.message; $('sequence-status').classList.add('error'); toast(error.message);
    } finally { bitmap?.close(); }
  };

  return {async open(input,options={}) {
    const files = [...input];
    if (!files.length) return;
    if (dialog.open) close(); else reset();
    const activeGeneration = generation;
    const context=getContext(),projectToken=context.projectToken;
    decoder=new AbortController();const signal=decoder.signal;
    $('sequence-settings').disabled=true;
    $('sequence-files-summary').textContent = '正在解码素材…'; $('sequence-status').textContent = '';
    $('sequence-order-preview').replaceChildren(); $('sequence-preview-canvas').getContext('2d').clearRect(0,0,128,128);
    $('sequence-preview-label').textContent = '尚未载入';
    $('sequence-scale-summary').textContent = ''; $('sequence-timing-summary').textContent = '';
    $('sequence-decoder-warning').textContent = ''; $('sequence-decoder-warning').hidden = true;
    $('sequence-clip-warning').hidden = true; $('sequence-source-sheet').hidden = true;
    dialog.showModal();
    try {
      const decoded=await decodeMediaFiles(files,{signal,onProgress:({completed,total,name})=>{if(activeGeneration===generation)$('sequence-status').textContent=`正在解码 ${completed} / ${total}${name?' · '+name:''}`;}});
      if (activeGeneration !== generation) return;
      if(options.replaceAssetId && decoded.frames.length!==1)throw new Error('替换素材只支持单张静态图片；动画请作为新动作导入。');
      if(getContext().projectToken!==projectToken)throw new Error('桌宠工程已变化，请重新选择素材。');
      const sources=decoded.frames.map((frame,sourceIndex)=>({...frame,sourceIndex}));
      state = {files:decoded.frames.map(frame=>frame.blob),sources,clips:decoded.clips,animated:decoded.animated,gridEligible:decoded.gridEligible,
        source:{format:decoded.format,fileNames:decoded.fileNames},projectToken,replaceAssetId:options.replaceAssetId};
      $('sequence-settings').disabled=false;
      $('sequence-files-summary').textContent = `${decoded.format.toUpperCase()} · ${sources.length} 帧 · ${decoded.clips.length} 个动作标签${options.replaceAssetId?' · 替换后保留原素材 ID、动画时长和所有引用':''}`;
      $('sequence-mode').value = 'batch'; $('sequence-mode').querySelector('[value="sheet"]').disabled = !decoded.gridEligible || Boolean(options.replaceAssetId);
      $('sequence-mode').disabled=Boolean(options.replaceAssetId) || !decoded.gridEligible;
      $('sequence-file-order').value = 'forward'; $('sequence-scale-mode').value = 'fit'; $('sequence-scale').value = 100;
      $('sequence-source-anchor').value = 'bottom-center'; $('sequence-anchor-x').value = 64; $('sequence-anchor-y').value = 128;
      $('sequence-cell-width').value = Math.min(128, sources[0].width); $('sequence-cell-height').value = Math.min(128, sources[0].height);
      for (const id of ['sequence-offset-x','sequence-offset-y','sequence-gap-x','sequence-gap-y']) $(id).value = 0;
      $('sequence-grid-order').value = 'rows'; $('sequence-count').value = ''; $('sequence-ticks').value = 6; $('sequence-preview-index').value = 0;
      $('sequence-loop').value='source';$('sequence-preserve-timing').checked=decoded.animated;
      const clipOptions=decoded.clips.map((clip,index)=>{const option=document.createElement('option');option.value=index;option.textContent=`${clip.name} · ${clip.indices.length} 帧`;return option;});
      $('sequence-clip-select').replaceChildren(...clipOptions);
      $('sequence-preview-clip').replaceChildren(...clipOptions.map(option=>option.cloneNode(true)));
      if(decoded.clips.length>1){const all=document.createElement('option');all.value='all';all.textContent=`全部 ${decoded.clips.length} 个标签 · 分别创建动作`;$('sequence-clip-select').prepend(all);$('sequence-clip-select').value='all';}
      $('sequence-preview-clip').value='0';
      $('sequence-decoder-warning').textContent=decoded.warnings.join(' ');$('sequence-decoder-warning').hidden=!decoded.warnings.length;
      $('sequence-destination').querySelector('[value="append"]').textContent = context.kind === 'parts' ? '添加到当前部件图层' : '追加到当前动作';
      $('sequence-destination').querySelector('[value="append"]').disabled=decoded.clips.length>1 || decoded.animated&&context.kind==='parts';
      $('sequence-destination').querySelector('[value="replace"]').hidden=!options.replaceAssetId;
      $('sequence-destination').disabled=Boolean(options.replaceAssetId);
      $('sequence-destination').value=options.replaceAssetId?'replace':decoded.animated||decoded.clips.length>1?'new':'append';
      $('sequence-name').value=decoded.name.slice(0,80) || '导入动作';
      $('sequence-confirm').textContent=options.replaceAssetId?'替换素材':'导入素材与动作';
      await preview();
    } catch (error) {
      if (activeGeneration !== generation || error.name==='AbortError') return;
      $('sequence-files-summary').textContent = `${files.length} 个所选文件`;
      $('sequence-status').textContent = error.message; $('sequence-status').classList.add('error'); $('sequence-confirm').disabled = true;
      toast(error.message);
    }
  }};
}
