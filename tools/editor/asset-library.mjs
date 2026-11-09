import {assetReferences,deriveAssetGroups,deleteUnusedAssets,renameAsset,renameAssetGroup,createAssetGroup,removeAssetGroup} from './asset-library-model.mjs';

const el=(tag,className,text)=>{const node=document.createElement(tag);if(className)node.className=className;if(text!==undefined)node.textContent=text;return node;};
const button=(text,handler,className='')=>{const node=el('button',className,text);node.type='button';node.onclick=handler;return node;};
const unique=items=>[...new Set(items)];

export function setupAssetLibrary({getState,onEdit,onInsert,onReplace,toast,idFactory}) {
  const host=document.getElementById('asset-grid');
  host.classList.add('asset-library');host.setAttribute('aria-label','分组素材库');host.replaceChildren();
  const tools=el('div','asset-library-tools'),search=el('input'),filter=el('select');
  search.id='asset-library-search';search.type='search';search.placeholder='搜索素材、分组或来源';search.setAttribute('aria-label','搜索素材库');
  filter.id='asset-library-filter';filter.setAttribute('aria-label','筛选素材');
  for(const [value,label] of [['all','全部素材'],['current','当前动作使用'],['unused','未使用素材']]){const option=el('option','',label);option.value=value;filter.append(option);}
  const clear=button('清理未使用',()=>editDelete([...assetReferences(getState().project)].filter(([,ref])=>!ref.count).map(([id])=>id)));clear.id='asset-library-cleanup';
  const filters=el('div','asset-library-filters');filters.append(filter,clear);tools.append(search,filters);
  const selectedPanel=el('div','asset-library-selection'),selectionText=el('span'),insert=button('添加所选',()=>run(()=>onInsert([...selected]))),rename=button('重命名',renameSelected),replace=button('替换图片',replaceSelected),remove=button('删除',()=>editDelete([...selected]),'danger'),regroup=button('另建分组',regroupSelected);
  insert.id='asset-library-insert';rename.id='asset-library-rename';replace.id='asset-library-replace';remove.id='asset-library-delete';regroup.id='asset-library-regroup';
  const selectionActions=el('div','asset-library-selection-actions');selectionActions.append(insert,rename,replace,regroup,remove);selectedPanel.append(selectionText,selectionActions);
  const groupsHost=el('div','asset-library-groups');host.append(tools,selectedPanel,groupsHost);
  const replacement=el('input');replacement.id='asset-library-replacement-input';replacement.type='file';replacement.accept='.png,.jpg,.jpeg,.gif,.webp,.apng,image/png,image/jpeg,image/gif,image/webp';replacement.hidden=true;document.body.append(replacement);
  const naming=el('dialog','asset-library-name-dialog');naming.id='asset-library-name-dialog';naming.setAttribute('aria-labelledby','asset-library-name-title');
  const title=el('h2');title.id='asset-library-name-title';const nameLabel=el('label','', '名称'),nameInput=el('input');nameInput.id='asset-library-name-input';nameInput.maxLength=80;nameLabel.append(nameInput);
  const nameError=el('p','asset-library-error');nameError.setAttribute('role','alert');const nameActions=el('div','dialog-actions'),nameCancel=button('取消',()=>naming.close()),nameSave=button('保存',saveName,'primary');nameSave.id='asset-library-name-save';nameActions.append(nameCancel,nameSave);naming.append(title,nameLabel,nameError,nameActions);document.body.append(naming);
  const previewDialog=el('dialog','asset-library-preview');previewDialog.id='asset-library-preview-dialog';previewDialog.setAttribute('aria-labelledby','asset-library-preview-title');
  const previewTitle=el('h2');previewTitle.id='asset-library-preview-title';const previewSource=el('p','asset-library-preview-source'),canvas=el('canvas');canvas.width=128;canvas.height=128;canvas.setAttribute('aria-label','素材组 128 像素动画预览');
  const previewStage=el('div','asset-library-preview-stage');previewStage.append(canvas);const previewTransport=el('div','asset-library-preview-transport'),play=button('播放',togglePlay),scrub=el('input'),position=el('output');
  play.id='asset-library-preview-play';scrub.id='asset-library-preview-scrub';scrub.type='range';scrub.min=0;scrub.step=1;scrub.setAttribute('aria-label','素材组动画时间');position.id='asset-library-preview-position';previewTransport.append(play,scrub,position);
  const previewHint=el('p','asset-library-preview-hint'),previewActions=el('div','dialog-actions'),previewInsert=button('添加整组',insertGroup,'primary');previewInsert.id='asset-library-preview-insert';previewActions.append(button('关闭',()=>previewDialog.close()),previewInsert);previewDialog.append(previewTitle,previewSource,previewStage,previewTransport,previewHint,previewActions);document.body.append(previewDialog);

  let selected=new Set(),anchor=null,visibleIds=[],collapsed=new Map(),lastProject=null,refs=new Map(),nameTask=null,replacementTask=null,preview=null,raf=0;
  const images=new Map();
  function run(work){try{return work();}catch(error){toast(error.message);return false;}}
  function edit(mutator,message){return run(()=>onEdit(mutator,message));}
  function stopPreview(){if(raf)cancelAnimationFrame(raf);raf=0;if(preview)preview.playing=false;play.textContent='播放';}
  function closePreview(){stopPreview();preview=null;if(previewDialog.open)previewDialog.close();canvas.getContext('2d').clearRect(0,0,128,128);}
  previewDialog.addEventListener('close',()=>{if(!previewDialog.open){stopPreview();preview=null;}});
  previewDialog.addEventListener('cancel',stopPreview);
  naming.addEventListener('close',()=>{if(!naming.open)nameTask=null;});
  nameInput.addEventListener('keydown',event=>{if(event.key==='Enter'){event.preventDefault();saveName();}});
  function askName(label,value,mutator){nameTask={project:getState().project,mutator};title.textContent=label;nameInput.value=value;nameError.textContent='';naming.showModal();nameInput.focus();nameInput.select();}
  function saveName(){if(!nameTask)return;if(nameTask.project!==getState().project){nameError.textContent='工程已变化，请取消后重试。';return;}const result=edit(project=>nameTask.mutator(project,nameInput.value),'已更新素材名称');if(result!==false)naming.close();else nameError.textContent='名称未保存，请检查名称或当前工程。';}
  function renameSelected(){if(selected.size!==1)return;const id=[...selected][0],asset=getState().project.assets.find(asset=>asset.id===id);if(asset)askName('重命名素材',asset.name,(project,name)=>renameAsset(project,id,name));}
  function regroupSelected(){const ids=[...selected];if(ids.length)askName('将所选素材另建分组','新素材组',(project,name)=>createAssetGroup(project,ids,name,idFactory));}
  function editDelete(ids){if(!ids.length)return;const result=edit(project=>deleteUnusedAssets(project,ids),'已删除未使用素材');if(result!==false){selected=new Set([...selected].filter(id=>!ids.includes(id)));render();}}
  function replaceSelected(){if(selected.size!==1)return;replacementTask={project:getState().project,assetId:[...selected][0]};replacement.value='';replacement.click();}
  replacement.onchange=()=>{const task=replacementTask,files=[...replacement.files];replacementTask=null;replacement.value='';if(!files.length||!task)return;if(task.project!==getState().project){toast('工程已变化，请重新选择要替换的素材。');return;}run(()=>onReplace(task.assetId,files));};
  function selectionUI(){
    refs=assetReferences(getState().project);
    for(const tile of groupsHost.querySelectorAll('[data-library-asset]')){const active=selected.has(tile.dataset.libraryAsset);tile.classList.toggle('selected',active);tile.setAttribute('aria-pressed',String(active));}
    selectionText.textContent=selected.size?`已选 ${selected.size} 张`:'单击选择 · 双击添加';
    insert.disabled=regroup.disabled=!selected.size;rename.disabled=replace.disabled=selected.size!==1;
    const protectedSelection=[...selected].some(id=>refs.get(id)?.count);remove.disabled=!selected.size||protectedSelection;remove.title=protectedSelection?'素材仍被动作或部件使用，先移除引用才能删除。':'删除所选未使用素材';
    clear.disabled=![...refs.values()].some(ref=>!ref.count);
  }
  function selectAsset(id,event){
    if(event.shiftKey&&anchor&&visibleIds.includes(anchor)){const a=visibleIds.indexOf(anchor),b=visibleIds.indexOf(id);if(!event.metaKey&&!event.ctrlKey)selected.clear();for(const item of visibleIds.slice(Math.min(a,b),Math.max(a,b)+1))selected.add(item);}
    else if(event.metaKey||event.ctrlKey){if(selected.has(id))selected.delete(id);else selected.add(id);anchor=id;}
    else{selected=new Set([id]);anchor=id;}
    selectionUI();
  }
  function imageFor(asset){let image=images.get(asset.id);if(!image||image.srcValue!==asset.src){image=new Image();image.srcValue=asset.src;image.onload=()=>{if(preview?.project===getState().project)drawPreview();};image.src=asset.src;images.set(asset.id,image);}return image;}
  function drawPreview(){
    if(!preview)return;const ctx=canvas.getContext('2d');ctx.clearRect(0,0,128,128);let remaining=preview.tick,index=0;
    for(;index<preview.group.frames.length-1;index++){if(remaining<preview.group.frames[index].ticks)break;remaining-=preview.group.frames[index].ticks;}
    const frame=preview.group.frames[index],asset=preview.project.assets.find(asset=>asset.id===frame.assetId),image=asset&&imageFor(asset);if(image?.complete&&image.naturalWidth){ctx.imageSmoothingEnabled=false;ctx.drawImage(image,0,0,128,128);}
    scrub.value=preview.tick;position.textContent=`${index+1} / ${preview.group.frames.length} 帧 · ${preview.tick} / ${preview.total} tick`;
  }
  function animate(now){raf=0;if(!preview?.playing||!previewDialog.open||document.hidden)return;const next=Math.floor((now-preview.started)/40)%preview.total;if(next!==preview.tick){preview.tick=next;drawPreview();}raf=requestAnimationFrame(animate);}
  function togglePlay(){if(!preview)return;if(preview.playing){stopPreview();return;}preview.playing=true;preview.started=performance.now()-preview.tick*40;play.textContent='暂停';raf=requestAnimationFrame(animate);}
  document.addEventListener('visibilitychange',()=>{if(raf)cancelAnimationFrame(raf);raf=0;if(preview?.playing&&!document.hidden){preview.started=performance.now()-preview.tick*40;raf=requestAnimationFrame(animate);}});
  scrub.oninput=()=>{if(!preview)return;stopPreview();preview.tick=Number(scrub.value);drawPreview();};
  function openPreview(group){closePreview();const {project,currentActionId}=getState(),parts=project.actions.find(action=>action.id===currentActionId)?.kind==='parts';preview={project,group,tick:0,total:group.frames.reduce((sum,frame)=>sum+frame.ticks,0),playing:false};previewTitle.textContent=group.name;previewSource.textContent=group.source?`${group.source.format} · ${group.source.fileNames.join('、')||'内置素材'}`:'按现有动作和素材推导的分组';scrub.max=preview.total-1;previewInsert.disabled=parts;previewHint.textContent=parts?'整组动画请追加到整帧动作；单张素材可添加为部件。':`以原顺序和时长追加到当前动作 · ${(preview.total*.04).toFixed(2)} 秒`;previewDialog.showModal();drawPreview();}
  function insertGroup(){if(!preview)return;const group=preview.group;if(preview.project!==getState().project){closePreview();toast('工程已变化，请重新打开分组预览。');return;}const result=run(()=>onInsert(group.frames.map(frame=>frame.assetId),{frames:group.frames.map(frame=>({...frame})),name:group.name,loop:true}));if(result!==false)closePreview();}
  search.oninput=render;filter.onchange=render;
  function render(){
    const {project,currentActionId}=getState();if(!project)return;
    if(preview&&preview.project!==project)closePreview();
    if(lastProject!==project){images.clear();lastProject=project;}
    const assets=new Map(project.assets.map(asset=>[asset.id,asset]));selected=new Set([...selected].filter(id=>assets.has(id)));refs=assetReferences(project);
    const query=search.value.trim().toLocaleLowerCase(),fragment=document.createDocumentFragment(),ordered=[];
    for(const group of deriveAssetGroups(project)){
      const sourceText=[group.name,group.source?.format,...(group.source?.fileNames??[])].join(' ').toLocaleLowerCase(),groupMatch=sourceText.includes(query);
      const ids=unique(group.frames.map(frame=>frame.assetId)).filter(id=>{const ref=refs.get(id);return(filter.value!=='current'||ref.actionIds.includes(currentActionId))&&(filter.value!=='unused'||!ref.count)&&(!query||groupMatch||assets.get(id).name.toLocaleLowerCase().includes(query));});
      if(!ids.length)continue;ordered.push(...ids);
      const card=el('details','asset-library-group');card.dataset.libraryGroup=group.id;card.open=!(collapsed.get(group.id)??group.id==='virtual:builtin');card.ontoggle=()=>collapsed.set(group.id,!card.open);
      const summary=el('summary'),thumb=el('img');thumb.src=assets.get(ids[0]).src;thumb.alt='';thumb.loading='lazy';const copy=el('span','asset-library-group-copy'),label=el('strong','',group.name),counts=el('small','',`${ids.length} 张 · ${group.frames.length} 帧 · ${ids.filter(id=>refs.get(id).count).length} 使用`);copy.append(label,counts);summary.append(thumb,copy);card.append(summary);
      const groupTools=el('div','asset-library-group-tools'),previewButton=button('预览',()=>openPreview(group)),groupRename=button('改名',()=>askName('重命名分组',group.name,(project,name)=>renameAssetGroup(project,group.id,name,idFactory)));
      previewButton.setAttribute('aria-label',`预览分组 ${group.name}`);groupTools.append(previewButton,groupRename);if(!group.virtual)groupTools.append(button('解散',()=>edit(project=>removeAssetGroup(project,group.id),'已解散分组，素材仍保留')));
      card.append(groupTools);if(group.source){const source=el('p','asset-library-group-source',`${group.source.format}${group.source.fileNames.length?' · '+group.source.fileNames.join('、'):''}`);source.title=source.textContent;card.append(source);}
      const grid=el('div','asset-library-tiles');
      for(const id of ids){const asset=assets.get(id),ref=refs.get(id),tile=button('',event=>selectAsset(id,event),'asset-library-tile');tile.dataset.libraryAsset=id;tile.setAttribute('aria-label',`选择素材 ${asset.name}，${ref.count?'已使用 '+ref.count+' 次':'未使用'}`);tile.setAttribute('aria-pressed',String(selected.has(id)));tile.ondblclick=()=>run(()=>onInsert([id]));const image=el('img');image.src=asset.src;image.alt='';image.loading='lazy';tile.append(image,el('span','asset-library-asset-name',asset.name),el('small','',ref.count?`使用 ${ref.count}`:'未使用'));tile.title=`${asset.name}\n单击选择，双击添加\n${ref.frames} 个动画帧 · ${ref.parts} 个部件 · ${ref.keys} 个关键帧`;grid.append(tile);}
      card.append(grid);fragment.append(card);
    }
    visibleIds=unique(ordered);if(!visibleIds.length)fragment.append(el('p','asset-library-empty','没有匹配的素材。'));groupsHost.replaceChildren(fragment);selectionUI();
  }
  function reset(){selected.clear();anchor=null;collapsed.clear();images.clear();lastProject=null;replacementTask=null;closePreview();if(naming.open)naming.close();search.value='';filter.value='all';render();}
  render();return{render,reset};
}
