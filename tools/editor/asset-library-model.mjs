const idPattern = /^[a-zA-Z0-9_-]{1,64}$/;
const named = (value,max) => typeof value==='string' && value.trim().length>0 && [...value].length<=max && !/[\x00-\x1f\x7f]/.test(value);
function nameOf(value) { if(!named(value,80)) throw new Error('名称需要包含 1–80 个字符。'); return value.trim(); }
function assetIdsOf(project,ids) {
  if(!Array.isArray(ids))throw new Error('请选择仍然存在的素材。');
  const result=[...new Set(ids)], assets=new Set(project.assets.map(asset=>asset.id));
  if(!result.length || result.some(id=>!assets.has(id))) throw new Error('请选择仍然存在的素材。');
  return result;
}

/** Membership is authoring metadata; only actual action/part references protect an asset. */
export function assetReferences(project) {
  const refs=new Map(project.assets.map(asset=>[asset.id,{count:0,frames:0,parts:0,keys:0,actionIds:[]}]));
  const add=(id,kind,actionIds=[])=>{const ref=refs.get(id);if(!ref)return;ref.count++;ref[kind]++;for(const actionId of actionIds)if(!ref.actionIds.includes(actionId))ref.actionIds.push(actionId);};
  for(const action of project.actions ?? []) {
    for(const frame of action.frames ?? [])add(frame.assetId,'frames',[action.id]);
    for(const track of action.tracks ?? [])for(const key of track.keys ?? [])add(key.assetId,'keys',[action.id]);
  }
  const rigActions=(project.actions ?? []).filter(action=>action.kind==='parts').map(action=>action.id);
  for(const part of project.parts ?? [])add(part.assetId,'parts',rigActions);
  return refs;
}

export function validateAssetGroups(groups,assets) {
  if(assets===undefined && groups && !Array.isArray(groups)) {assets=groups.assets;groups=groups.assetGroups;}
  if(groups===undefined)return;
  if(!Array.isArray(groups)||groups.length>256)throw new Error('素材分组数量无效。');
  const ids=new Set(),assetIds=new Set(assets.map(asset=>asset.id));
  for(const group of groups) {
    if(!group||typeof group.id!=='string'||!idPattern.test(group.id)||ids.has(group.id)||!named(group.name,80))throw new Error('素材分组 ID 或名称无效。');
    ids.add(group.id);
    if(!Array.isArray(group.frames)||!group.frames.length||group.frames.length>256)throw new Error('素材分组需要包含 1–256 帧。');
    for(const frame of group.frames)if(!frame||!assetIds.has(frame.assetId)||!Number.isInteger(frame.ticks)||frame.ticks<1||frame.ticks>1500)throw new Error('素材分组引用或时长无效。');
    if(group.source!==undefined) {
      if(!group.source||!named(group.source.format,80)||!Array.isArray(group.source.fileNames)||group.source.fileNames.length>256||group.source.fileNames.some(name=>!named(name,256)))throw new Error('素材分组来源无效。');
    }
  }
}

/** Stable virtual groups upgrade old projects without writing during render. */
export function deriveAssetGroups(project) {
  const assets=new Set(project.assets.map(asset=>asset.id)), covered=new Set(), groups=[];
  for(const group of project.assetGroups ?? []) {
    const frames=group.frames.filter(frame=>assets.has(frame.assetId)).map(frame=>({...frame}));
    if(!frames.length)continue;
    groups.push({...group,frames,source:group.source?{...group.source,fileNames:[...group.source.fileNames]}:undefined,virtual:false});
    for(const frame of frames)covered.add(frame.assetId);
  }
  const add=(id,name,frames,extra={})=>{if(!frames.length)return;groups.push({id,name,frames,virtual:true,...extra});for(const frame of frames)covered.add(frame.assetId);};
  add('virtual:builtin','内置素材',project.assets.filter(asset=>!covered.has(asset.id)&&/^\/assets\/shime\d+\.png$/.test(asset.src)).map(asset=>({assetId:asset.id,ticks:6})),{source:{format:'内置',fileNames:[]}});
  for(const action of project.actions ?? []) {
    const available=new Set((action.frames ?? []).filter(frame=>assets.has(frame.assetId)&&!covered.has(frame.assetId)).map(frame=>frame.assetId));
    add(`virtual:action:${action.id}`,action.name,(action.frames ?? []).filter(frame=>available.has(frame.assetId)).map(frame=>({assetId:frame.assetId,ticks:frame.ticks})),{actionId:action.id});
  }
  add('virtual:ungrouped','未分组',project.assets.filter(asset=>!covered.has(asset.id)).map(asset=>({assetId:asset.id,ticks:6})));
  return groups;
}

export function renameAsset(project,id,name) {
  const next=nameOf(name),asset=project.assets.find(asset=>asset.id===id);
  if(!asset)throw new Error('素材已不存在。');asset.name=next;
}

export function renameAssetGroup(project,id,name,idFactory) {
  const next=nameOf(name),group=deriveAssetGroups(project).find(group=>group.id===id);
  if(!group)throw new Error('分组已不存在。');
  if(!group.virtual) {project.assetGroups.find(group=>group.id===id).name=next;return id;}
  const created={id:idFactory('asset-group'),name:next,frames:group.frames.map(frame=>({...frame})),...(group.source?{source:group.source}:{})};
  validateAssetGroups([...(project.assetGroups ?? []),created],project.assets);
  (project.assetGroups ??= []).push(created);return created.id;
}

export function createAssetGroup(project,ids,name,idFactory) {
  const selected=assetIdsOf(project,ids),next=nameOf(name);
  const group={id:idFactory('asset-group'),name:next,frames:selected.map(assetId=>({assetId,ticks:6}))};
  validateAssetGroups([...(project.assetGroups ?? []),group],project.assets);
  (project.assetGroups ??= []).push(group);return group.id;
}

export function removeAssetGroup(project,id) {
  if(!(project.assetGroups ?? []).some(group=>group.id===id))throw new Error('此分组由现有素材推导，可重命名或另建分组。');
  project.assetGroups=project.assetGroups.filter(group=>group.id!==id);
}

/** Preflight the entire batch before touching arrays: a used item never causes partial deletion. */
export function deleteUnusedAssets(project,ids) {
  const selected=assetIdsOf(project,ids),refs=assetReferences(project);
  if(selected.some(id=>refs.get(id).count>0))throw new Error('选中的素材仍被动作或部件使用，请先移除引用。');
  if(selected.length===project.assets.length)throw new Error('工程至少需要保留一张素材。');
  const removed=new Set(selected);
  project.assets=project.assets.filter(asset=>!removed.has(asset.id));
  if(project.assetGroups)project.assetGroups=project.assetGroups.map(group=>({...group,frames:group.frames.filter(frame=>!removed.has(frame.assetId))})).filter(group=>group.frames.length);
  return selected.length;
}
