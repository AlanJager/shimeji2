import {createVrmScene} from './vrm-scene.mjs';
import {inspectVrmGlb, inspectVrmaGlb, validateVrmRecipe, VRM_DEFAULT_RECIPE, VRM_LIMITS, serializeVrmStudioProject, parseVrmStudioProject} from './vrm-model.mjs';

const clone = value => JSON.parse(JSON.stringify(value));
const hash = async buffer => [...new Uint8Array(await crypto.subtle.digest('SHA-256', buffer))].map(byte => byte.toString(16).padStart(2,'0')).join('');
const isAbort = error => error?.name === 'AbortError';

export function setupVrmStudio({getContext, onBake, toast}) {
  const dialog = document.createElement('dialog'); dialog.id = 'vrm-studio-dialog'; dialog.className = 'vrm-studio'; dialog.setAttribute('aria-labelledby','vrm-studio-title');
  const numberFields = [
    ['yaw','镜头水平角度 / °',-180,180,1], ['pitch','镜头俯仰 / °',-60,60,1],
    ['zoom','镜头缩放',.25,4,.05], ['target-y','镜头高度',0,2,.05],
  ];
  dialog.innerHTML = `<header class="vrm-header"><div><span class="eyebrow">3D → SPRITE</span><h2 id="vrm-studio-title">VRM 动作工作室</h2></div><button id="vrm-close" class="icon-button" aria-label="关闭 VRM 工作室">×</button></header>
    <div class="vrm-import-bar"><button id="vrm-open-model">导入 VRM 模型</button><button id="vrm-open-motion">导入 VRMA 动作</button><button id="vrm-open-source" class="quiet">打开 3D 工程</button><button id="vrm-save-source" class="quiet" disabled>保存 3D 工程</button></div>
    <p id="vrm-source-note" class="vrm-source-note">3D 原文件保留在工作室中；保存 3D 工程可在以后继续调整。普通桌宠工程只保存烘焙后的图片与动作设置。</p>
    <div class="vrm-layout"><section class="vrm-preview-section" aria-label="3D 模型预览"><div id="vrm-view"><div id="vrm-empty"><span aria-hidden="true">◇</span><strong>从自己的角色开始</strong><p>导入自包含的 VRM 0.x 或 VRM 1.0 模型。<br>支持 VRMA 动作，也可以先试呼吸、挥手等预设。</p></div></div>
      <div class="vrm-preview-bottom"><div class="vrm-timeline"><button id="vrm-play" class="vrm-play" aria-label="播放 3D 预览" disabled>▶</button><input id="vrm-scrub" type="range" min="0" max="49" step="1" value="0" aria-label="3D 动作预览时间" disabled><output id="vrm-time">0 / 50 tick</output></div><div class="vrm-native"><canvas id="vrm-native-canvas" width="128" height="128" aria-label="128 像素导出尺寸预览"></canvas><span>导出尺寸 · 128 × 128</span><button id="vrm-save-frame" type="button" class="text-button" disabled>保存当前帧 PNG</button></div></div>
      <div id="vrm-metadata" class="vrm-metadata"></div><div id="vrm-warning" class="vrm-warning" hidden></div></section>
    <fieldset id="vrm-settings" class="vrm-settings" disabled><label>动作名称<input id="vrm-action-name" maxlength="80" value="3D 角色动作"></label><label>动作来源<select id="vrm-preset"><option value="idle">自然站立</option><option value="breathe">轻轻呼吸</option><option value="wave">挥手</option><option value="dance">轻轻摇摆</option><option value="vrma" disabled>导入的 VRMA 动作</option></select></label>
      <div class="vrm-two"><label>表情<select id="vrm-expression"><option value="neutral">自然</option></select></label><label>表情强度<output id="vrm-expression-value">0%</output><input id="vrm-expression-weight" type="range" min="0" max="1" step="0.05" value="0"></label></div>
      <div class="vrm-settings-divider"></div><div class="vrm-two">${numberFields.map(([id,label,min,max,step]) => `<label>${label}<input id="vrm-${id}" type="number" min="${min}" max="${max}" step="${step}"></label>`).join('')}</div>
      <div class="vrm-settings-divider"></div><div class="vrm-two"><label>片段起点 / 秒<input id="vrm-start-seconds" type="number" min="0" max="3600" step="0.04" value="0"></label><label>片段长度 / tick<input id="vrm-duration-ticks" type="number" min="1" max="250" step="1" value="50"></label></div>
      <p id="vrm-duration-note" class="vrm-help">每 tick 40 ms，最长 10 秒。</p><label class="vrm-check"><input id="vrm-loop" type="checkbox" checked>循环播放</label><label class="vrm-check"><input id="vrm-in-place" type="checkbox" checked>保持角色位置</label><label class="vrm-check"><input id="vrm-spring" type="checkbox">启用模型弹簧物理</label>
      <button id="vrm-clear-motion" type="button" class="text-button" hidden>移除导入的 VRMA 动作</button></fieldset></div>
    <footer class="vrm-footer"><div><p id="vrm-status" role="status">就绪</p><p id="vrm-capacity" class="vrm-help"></p></div><div><button id="vrm-cancel" type="button">关闭</button><button id="vrm-bake" type="button" class="primary" disabled>烘焙并添加为动作 ↗</button></div></footer>
    <input id="vrm-model-input" type="file" accept=".vrm" hidden><input id="vrm-motion-input" type="file" accept=".vrma" hidden><input id="vrm-source-input" type="file" accept=".json,application/json" hidden>`;
  document.body.append(dialog);
  const $ = id => dialog.querySelector('#' + id);
  let saved = null, scene = null, candidate = null, request = 0, opened = 0, busy = false, baking = false, playing = false;
  let projectToken = null, recipe = {...VRM_DEFAULT_RECIPE}, localTime = 0, ignoredClose = 0;

  function status(message, error = false) { $('vrm-status').textContent = message; $('vrm-status').classList.toggle('error', error); }
  function warning(message) { $('vrm-warning').textContent = message; $('vrm-warning').hidden = !message; }
  function disposeScene(value) { try { value?.dispose(); } catch {} }
  function cleanup() {
    request++; opened++; busy = false; baking = false; playing = false;
    disposeScene(candidate); candidate = null; disposeScene(scene); scene = null;
    $('vrm-view').querySelectorAll('.vrm-scene-stage').forEach(el => el.remove());
    $('vrm-empty').hidden = false;
    $('vrm-native-canvas').getContext('2d').clearRect(0,0,128,128);
  }
  function close() { cleanup(); if (dialog.open) { ignoredClose++; dialog.close(); } }
  dialog.addEventListener('close', () => { if (ignoredClose) ignoredClose--; else cleanup(); });
  dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
  $('vrm-close').onclick = close; $('vrm-cancel').onclick = close;

  function capacity() {
    const context = getContext(), remaining = Math.max(0, 256-context.assetCount);
    $('vrm-capacity').textContent = `可再添加 ${remaining} 张素材 · 此片段生成 ${recipe.durationTicks} 帧 · ${context.actionCount} / 64 个动作`;
    return context.actionCount < 64 && recipe.durationTicks <= remaining;
  }
  function controls() {
    $('vrm-settings').disabled = busy || !scene;
    for (const id of ['vrm-open-model','vrm-open-motion','vrm-open-source']) $(id).disabled = busy;
    $('vrm-open-motion').disabled = busy || !saved?.modelBuffer;
    $('vrm-save-source').disabled = busy || !saved?.modelBuffer;
    $('vrm-play').disabled = busy || !scene;
    $('vrm-scrub').disabled = busy || !scene;
    $('vrm-save-frame').disabled = busy || !scene;
    $('vrm-bake').disabled = busy || !scene || !capacity();
    $('vrm-play').textContent = playing ? 'Ⅱ' : '▶'; $('vrm-play').setAttribute('aria-label', playing ? '暂停 3D 预览' : '播放 3D 预览');
    $('vrm-cancel').textContent = baking ? '取消烘焙并关闭' : busy ? '取消并关闭' : '关闭';
  }
  function syncRecipe() {
    for (const [key,id] of [['yaw','yaw'],['pitch','pitch'],['zoom','zoom'],['targetY','target-y'],['startSeconds','start-seconds'],['durationTicks','duration-ticks']]) $('vrm-'+id).value = recipe[key];
    $('vrm-preset').value = recipe.preset;
    $('vrm-expression').value = recipe.expression; $('vrm-expression-weight').value = recipe.expressionWeight;
    $('vrm-expression-value').textContent = `${Math.round(recipe.expressionWeight*100)}%`;
    for (const [key,id] of [['loop','loop'],['inPlace','in-place'],['spring','spring']]) $('vrm-'+id).checked = recipe[key];
    $('vrm-scrub').max = recipe.durationTicks-1;
    $('vrm-duration-note').textContent = `${recipe.durationTicks} 帧 · ${(recipe.durationTicks*.04).toFixed(2)} 秒 · 每帧 40 ms`;
  }
  function updateCompanion() {
    if (!scene) return;
    scene.renderNativePreview($('vrm-native-canvas'));
  }
  function time(seconds) {
    localTime = seconds;
    const tick = Math.min(recipe.durationTicks-1, Math.max(0,Math.round(seconds/.04)));
    $('vrm-scrub').value = tick; $('vrm-time').textContent = `${tick} / ${recipe.durationTicks} tick`;
    updateCompanion();
  }
  function metadata() {
    const area = $('vrm-metadata'); area.replaceChildren();
    if (!saved) return;
    const info = saved.modelInfo, line = document.createElement('strong');
    line.textContent = `${info.name || saved.modelFileName} · VRM ${info.version}`; area.append(line);
    for (const text of [`作者：${(info.authors ?? []).join('、') || '未提供'}`, `${info.stats.vertices.toLocaleString()} 顶点 · ${info.stats.meshes} 网格 · ${(saved.modelBuffer.byteLength/1024/1024).toFixed(2)} MiB`, saved.motionBuffer ? `VRMA：${saved.motionFileName}${saved.motionInfo?.durationSeconds ? ` · ${saved.motionInfo.durationSeconds.toFixed(2)} 秒` : ''}` : '动作：使用内置姿势预设']) {
      const p = document.createElement('p'); p.textContent = text; area.append(p);
    }
  }
  async function load(next) {
    const id = ++request, session = opened;
    busy = true; playing = false; scene?.play(false); warning(''); status('正在读取模型并准备预览…'); controls();
    const stage = document.createElement('div'); stage.className = 'vrm-scene-stage staging'; $('vrm-view').append(stage);
    let nextScene, callbackScene;
    try {
      next.modelInfo = inspectVrmGlb(next.modelBuffer);
      if (next.motionBuffer) next.motionInfo = inspectVrmaGlb(next.motionBuffer);
      next.recipe = validateVrmRecipe(next.recipe);
      const [modelHash,motionHash] = await Promise.all([hash(next.modelBuffer),next.motionBuffer ? hash(next.motionBuffer) : null]);
      if (next.modelHash && next.modelHash.toLowerCase() !== modelHash) throw new Error('3D 工程中的模型校验和不匹配，未替换当前角色。');
      if (next.motionHash && next.motionHash.toLowerCase() !== motionHash) throw new Error('3D 工程中的动作校验和不匹配，未替换当前角色。');
      next.modelHash = modelHash; next.motionHash = motionHash;
      if (session !== opened || id !== request || !dialog.open) return;
      nextScene = await createVrmScene(stage, {
        onTime: seconds => { if (scene === callbackScene && session === opened) { time(seconds); if (playing && !recipe.loop && seconds >= recipe.durationTicks*.04) { playing=false; controls(); } } },
        onError: error => { if (scene === callbackScene && session === opened) { playing = false; status(error.message,true); controls(); } },
        onWarning: message => { if (scene === callbackScene && session === opened) warning(message); },
      });
      callbackScene = nextScene;
      candidate = nextScene;
      const loaded = await nextScene.loadModel(next.modelBuffer);
      if (next.motionBuffer) next.motionInfo = await nextScene.loadAnimation(next.motionBuffer);
      nextScene.setRecipe(next.recipe); nextScene.renderAt(0);
      if (session !== opened || id !== request || !dialog.open) return;
      disposeScene(scene); $('vrm-view').querySelectorAll('.vrm-scene-stage').forEach(el => { if (el !== stage) el.remove(); });
      scene = nextScene; candidate = null; nextScene = null;
      saved = next; recipe = {...next.recipe}; localTime = 0;
      stage.classList.remove('staging'); $('vrm-empty').hidden = true;
      const expressions = [...new Set(['neutral',...(loaded.expressions ?? [])])];
      $('vrm-expression').replaceChildren(...expressions.map(name => { const option=document.createElement('option'); option.value=name; option.textContent=name==='neutral'?'自然':name;return option; }));
      $('vrm-preset').querySelector('[value="vrma"]').disabled = !saved.motionBuffer;
      $('vrm-clear-motion').hidden = !saved.motionBuffer;
      syncRecipe(); metadata(); time(0);
      status('模型已就绪。可调整动作与镜头，再烘焙为桌宠动画。');
    } catch (error) {
      if (session === opened && id === request && !isAbort(error)) { status(`${error.message}${saved ? ' 当前有效模型已保留。' : ''}`,true); toast(error.message); }
    } finally {
      disposeScene(nextScene); if (candidate === nextScene) candidate = null;
      if (stage.classList.contains('staging')) stage.remove();
      if (session === opened && id === request) { busy = false; controls(); }
    }
  }
  async function readFiles(files) {
    if (!files.length || busy) return;
    const session = opened, id = ++request;
    busy = true; playing = false; scene?.play(false); status('正在读取文件…'); controls();
    try {
      if (files.length > 2) throw new Error('一次请选择一个 VRM 模型和可选的一个 VRMA 动作。');
      const sources = files.filter(file => /\.json$/i.test(file.name));
      if (sources.length) {
        if (files.length !== 1 || sources[0].size > VRM_LIMITS.studioBytes) throw new Error('请选择单个不大于 60 MiB 的 3D 工程 JSON。');
        const next = parseVrmStudioProject(await sources[0].text());
        if (session !== opened || id !== request || !dialog.open) return;
        await load({...next}); return;
      }
      const models=files.filter(file=>/\.vrm$/i.test(file.name)), motions=files.filter(file=>/\.vrma$/i.test(file.name));
      if (models.length>1 || motions.length>1 || models.length+motions.length!==files.length) throw new Error('只支持 .vrm 模型、.vrma 动作，或单独的 3D 工程 JSON。');
      if (!models.length && !saved?.modelBuffer) throw new Error('请先导入 VRM 模型，再添加 VRMA 动作。');
      for (const file of models) if (file.size>VRM_LIMITS.modelBytes || !file.size) throw new Error('VRM 模型需要在 32 MiB 以内。');
      for (const file of motions) if (file.size>VRM_LIMITS.motionBytes || !file.size) throw new Error('VRMA 动作需要在 8 MiB 以内。');
      const next=models.length?{modelBuffer:await models[0].arrayBuffer(),modelFileName:models[0].name,motionBuffer:null,motionFileName:null,recipe:{...recipe,preset:recipe.preset==='vrma'?'idle':recipe.preset}}:{...saved,recipe:{...recipe}};
      if (motions.length) { next.motionBuffer=await motions[0].arrayBuffer();next.motionFileName=motions[0].name;next.motionHash=null;next.recipe.preset='vrma'; }
      if (session !== opened || id !== request || !dialog.open) return;
      await load(next);
    } catch (error) { if (session===opened && id===request && !isAbort(error)) {status(error.message,true);toast(error.message);} }
    finally { if (session===opened && id===request) { busy=false; controls(); } }
  }
  for (const [button,input] of [['vrm-open-model','vrm-model-input'],['vrm-open-motion','vrm-motion-input'],['vrm-open-source','vrm-source-input']]) {
    $(button).onclick=()=>$(input).click();
    $(input).onchange=async event=>{const files=[...event.target.files];event.target.value='';await readFiles(files);};
  }
  $('vrm-clear-motion').onclick=()=>{if(saved&&!busy)load({...saved,motionBuffer:null,motionFileName:null,motionHash:null,motionInfo:null,recipe:{...recipe,preset:recipe.preset==='vrma'?'idle':recipe.preset}});};
  function applyRecipe({report=true}={}) {
    if (!scene || busy) return false;
    try {
      for (const input of $('vrm-settings').querySelectorAll('input[type="number"]')) if (input.value.trim()==='') throw new Error('请填写完整的镜头与片段数值。');
      const next=validateVrmRecipe({yaw:Number($('vrm-yaw').value),pitch:Number($('vrm-pitch').value),zoom:Number($('vrm-zoom').value),targetY:Number($('vrm-target-y').value),startSeconds:Number($('vrm-start-seconds').value),durationTicks:Number($('vrm-duration-ticks').value),preset:$('vrm-preset').value,expression:$('vrm-expression').value,expressionWeight:Number($('vrm-expression-weight').value),loop:$('vrm-loop').checked,inPlace:$('vrm-in-place').checked,spring:$('vrm-spring').checked});
      scene.setRecipe(next); recipe=next; saved.recipe={...next};
      localTime=Math.min(localTime,Math.max(0,(recipe.durationTicks-1)*.04));scene.renderAt(localTime);syncRecipe();time(localTime);controls();
      status('设置已更新。');
      return true;
    } catch(error){if(report){status(error.message,true);toast(error.message);}return false;}
  }
  $('vrm-settings').addEventListener('change',event=>{if(event.target.id!=='vrm-action-name')applyRecipe();});
  $('vrm-settings').addEventListener('input',event=>{if(event.target.type==='number')applyRecipe({report:false});});
  $('vrm-expression-weight').addEventListener('input',applyRecipe);
  $('vrm-expression').addEventListener('change',()=>{if($('vrm-expression').value!=='neutral'&&Number($('vrm-expression-weight').value)===0){$('vrm-expression-weight').value=1;applyRecipe();}});
  $('vrm-play').onclick=()=>{if(scene&&!busy){playing=!playing;if(playing&&localTime>=recipe.durationTicks*.04)localTime=0;scene.renderAt(localTime);scene.play(playing);controls();}};
  $('vrm-scrub').addEventListener('input',()=>{if(scene&&!busy){playing=false;scene.play(false);scene.renderAt(Number($('vrm-scrub').value)*.04);time(scene.getTime());controls();}});
  function download(body,name){const url=URL.createObjectURL(body),a=document.createElement('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),10000);}
  $('vrm-save-frame').onclick=()=>{
    if(!scene||busy||!applyRecipe())return;
    playing=false;scene.play(false);scene.renderAt(localTime);updateCompanion();controls();
    const session=opened,id=request,tick=Math.min(recipe.durationTicks-1,Math.round(localTime/.04));
    $('vrm-native-canvas').toBlob(blob=>{if(session!==opened||id!==request||!dialog.open)return;if(!blob){status('当前帧 PNG 导出失败，请重试。',true);return;}download(blob,`shimeji2-vrm-frame-${tick}.png`);status(`已保存第 ${tick} tick 的 128 × 128 PNG。`);},'image/png');
  };
  $('vrm-save-source').onclick=()=>{if(!saved||busy||!applyRecipe())return;try{const text=serializeVrmStudioProject({...saved,recipe});download(new Blob([text],{type:'application/json'}),'shimeji2-vrm-studio.json');status('3D 工程已下载，包含原始模型、动作和设置。');}catch(error){status(error.message,true);toast(error.message);}};
  $('vrm-bake').onclick=async()=>{
    if(!scene||busy||!saved)return;
    if(!applyRecipe())return;
    const session=opened,id=++request,current=scene;
    try{
      const context=getContext();if(context.projectToken!==projectToken)throw new Error('桌宠工程已经切换，请关闭并重新打开 3D 工作室后再添加。');
      if(!capacity())throw new Error('动作或素材数量不足，请缩短片段，或使用素材更少的工程。');
      const name=$('vrm-action-name').value.trim();if(!name||[...name].length>80)throw new Error('动作名称需要包含 1–80 个字符。');
      busy=true;baking=true;playing=false;current.play(false);controls();warning('');
      const result=await current.bake({recipe,onProgress:({completed,total,clippedFrames})=>{if(session===opened&&id===request){status(`正在烘焙 ${completed} / ${total} 帧…`);if(clippedFrames)warning(`${clippedFrames} 帧接触画布边缘，可减小镜头缩放后重新烘焙。`);}},isCancelled:()=>session!==opened||id!==request||!dialog.open});
      if(session!==opened||id!==request||!dialog.open)return;
      if(getContext().projectToken!==projectToken)throw new Error('桌宠工程已变化，本次烘焙没有添加。请重新打开工作室。');
      if(!capacity())throw new Error('素材数量已变化，本次烘焙没有添加，请缩短片段。');
      const accepted=await onBake({assets:result.assets,ticks:1,name,loop:recipe.loop,projectToken,source3d:{format:'shimeji2-vrm-bake',version:1,recipe:clone(recipe),modelName:saved.modelFileName,modelHash:saved.modelHash,...(saved.motionBuffer?{motionName:saved.motionFileName,motionHash:saved.motionHash}:{})}});
      if(accepted===false)throw new Error('工程没有接受本次烘焙，原有动作未修改。');
      if(session===opened&&id===request){toast(`已添加 ${result.assets.length} 帧的 3D 动作。3D 原文件可从工作室另存。`);status('已添加为普通桌宠动作。');close();}
    }catch(error){if(session===opened&&id===request&&!isAbort(error)){status(error.message,true);toast(error.message);}}
    finally{if(session===opened&&id===request){busy=false;baking=false;controls();time(current.getTime());}}
  };
  return {async open(files=[]){
    if(dialog.open){if(files.length)await readFiles([...files]);return;}
    opened++;request++;projectToken=getContext().projectToken;warning('');status('就绪');dialog.showModal();syncRecipe();controls();
    if(files.length)await readFiles([...files]);else if(saved)await load({...saved,recipe:{...recipe}});
  }};
}
