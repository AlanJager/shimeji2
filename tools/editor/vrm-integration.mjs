import {validateVrmRecipe} from './vrm-model.mjs';

// Prepare a whole action before the editor's clone/validate/undo transaction.
// Source model bytes belong in the separate 3D studio file, never in this data.
export function prepareVrmAction(project, payload, idFactory) {
  if (payload.projectToken !== project) throw new Error('工程已变化，请重新打开 3D 工作室后烘焙。');
  if (project.actions.length >= 64) throw new Error('一个工程最多 64 个动作。');
  const recipe = validateVrmRecipe(payload.source3d?.recipe);
  if (!Array.isArray(payload.assets) || payload.assets.length !== recipe.durationTicks) throw new Error('烘焙帧数与动作时长不一致。');
  const name = String(payload.name || 'VRM 动作').trim();
  if (!name || name.length > 80 || typeof payload.loop !== 'boolean') throw new Error('动作名称或循环设置无效。');
  const sources = new Map(project.assets.map(item => [item.src, item.id])), assets = [], frames = [];
  for (const item of payload.assets) {
    if (!item || typeof item.src !== 'string' || !item.src.startsWith('data:image/png;base64,') || item.src.length > 128 * 1024) throw new Error('烘焙图片格式或大小无效。');
    let id = sources.get(item.src);
    if (!id) { id = idFactory('vrm-png'); assets.push({id, name:String(item.name || 'VRM.png').slice(0, 80),src:item.src}); sources.set(item.src, id); }
    frames.push({id:idFactory('frame'),assetId:id,ticks:1,x:0,y:0});
  }
  if (project.assets.length + assets.length > 256) throw new Error('素材超过 256 张，请缩短动画或移除未使用素材。');
  const source3d = {format:'vrm-bake',version:1,recipe};
  for (const key of ['modelName','modelHash','motionName','motionHash']) {
    if (payload.source3d[key] != null) source3d[key] = String(payload.source3d[key]).slice(0, 256);
  }
  return {assets,action:{id:idFactory('vrm-action'),name,kind:'frames',loop:payload.loop,frames,source3d}};
}
