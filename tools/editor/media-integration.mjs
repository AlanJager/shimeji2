// Prepare all imported clips before the editor applies its clone/validate/undo
// transaction. PNG pixels remain the desktop runtime's only image format.
export function prepareMediaImport(project, payload, currentActionId, idFactory) {
  const fail = message => { throw new Error(message); };
  const named = text => typeof text === 'string' && text.trim() && [...text].length <= 80;
  const current = project.actions.find(action => action.id === currentActionId);
  if (payload.projectToken !== project || !current) fail('工程已变化，请重新打开素材导入。');
  if (!Array.isArray(payload.assets) || !payload.assets.length || payload.assets.length > 256) fail('一次需要导入 1–256 张素材。');
  for (const asset of payload.assets) if (!asset || typeof asset.name !== 'string' || typeof asset.src !== 'string' || !/^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(asset.src) || asset.src.length > 128 * 1024) fail('归一化素材需要是有效的 128 像素 PNG。');
  if (payload.destination === 'replace') {
    if (payload.assets.length !== 1 || !project.assets.some(asset => asset.id === payload.replaceAssetId)) fail('替换需要选中一张现有素材，并导入单张图片。');
    return {selectedActionId:currentActionId, apply(next) {
      next.assets.find(asset => asset.id === payload.replaceAssetId).src = payload.assets[0].src;
    }};
  }
  if (!['append','new'].includes(payload.destination) || !named(payload.groupName)) fail('导入目标或素材组名称无效。');
  if (!Array.isArray(payload.clips) || !payload.clips.length || payload.clips.length > 64) fail('需要包含 1–64 个动画片段。');
  if (payload.destination === 'append' && payload.clips.length !== 1) fail('多个片段请分别新建动作。');
  if (payload.destination === 'new' && project.actions.length + payload.clips.length > 64) fail('导入后动作会超过 64 个。');
  if ((project.assetGroups?.length ?? 0) >= 256) fail('素材组已达到 256 个，请先整理。');
  const sources = new Map(project.assets.map(asset => [asset.src,asset.id])), assets = [];
  const imported = payload.assets.map(item => {
    let id = sources.get(item.src);
    if (!id) { id = idFactory('png'); sources.set(item.src,id); assets.push({...item,id}); }
    return {...item,id};
  });
  if (project.assets.length + assets.length > 256) fail('导入后素材会超过 256 张，请清理未使用素材或减少帧数。');
  function framesOf(frames) {
    if (!Array.isArray(frames) || !frames.length || frames.length > 256) fail('片段需要包含 1–256 帧。');
    return frames.map(frame => {
      if (!Number.isInteger(frame.assetIndex) || frame.assetIndex < 0 || frame.assetIndex >= imported.length || !Number.isInteger(frame.ticks) || frame.ticks < 1 || frame.ticks > 1500) fail('片段帧引用或时长无效。');
      return {assetId:imported[frame.assetIndex].id,ticks:frame.ticks};
    });
  }
  const clips = payload.clips.map(clip => {
    if (!named(clip.name) || typeof clip.loop !== 'boolean') fail('片段名称或循环设置无效。');
    return {...clip,frames:framesOf(clip.frames)};
  });
  const groupFrames = framesOf(payload.groupFrames ?? payload.clips[0].frames);
  const group = {id:idFactory('group'),name:payload.groupName,frames:groupFrames};
  if (payload.source) group.source = {format:payload.source.format,fileNames:[...payload.source.fileNames]};
  const parts = payload.destination === 'append' && current.kind === 'parts';
  const partAssets = [...new Set(payload.clips[0].frames.map(frame => frame.assetIndex))].map(index => imported[index]);
  if (parts && payload.animated) fail('整组动画请新建整帧动作，不能叠放为静态部件。');
  if (parts && project.parts.length + partAssets.length > 32) fail('当前工程最多 32 个部件。');
  if (!parts && payload.destination === 'append' && current.frames.length + clips[0].frames.length > 256) fail('当前动作最多 256 帧。');
  const actions = clips.map(clip => ({id:idFactory('action'),name:clip.name,kind:'frames',loop:clip.loop,
    frames:clip.frames.map(frame => ({...frame,id:idFactory('frame'),x:0,y:0}))}));
  return {parts,imported,partAssets,selectedActionId:payload.destination === 'new' ? actions[0].id : currentActionId,apply(next) {
    next.assets.push(...assets); (next.assetGroups ??= []).push(group);
    if (payload.destination === 'new') next.actions.push(...actions);
    else if (!parts) next.actions.find(action => action.id === currentActionId).frames.push(...actions[0].frames);
  }};
}
