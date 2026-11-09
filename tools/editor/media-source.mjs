import {pngSize, naturalOrder} from './sequence-model.mjs';

const LIMITS = {file:8*1024*1024,zip:16*1024*1024,total:32*1024*1024,json:1024*1024,png:16*1024*1024,response:24*1024*1024,pixels:16*1024*1024};
const abort = signal => { if (signal?.aborted) throw new DOMException('导入已取消','AbortError'); };
const pathOf = file => file.webkitRelativePath || file.name;
const stem = name => name.replace(/\.[^.]+$/,'').slice(0,80) || '导入素材';

async function boundedJson(response,signal) {
  if (Number(response.headers.get('Content-Length')) > LIMITS.response) throw new Error('解码结果超过大小上限。');
  const reader=response.body.getReader(), chunks=[]; let length=0;
  try {
    while(true) { abort(signal); const {value,done}=await reader.read(); if(done)break; length+=value.length; if(length>LIMITS.response)throw new Error('解码结果超过大小上限。');chunks.push(value); }
  } catch(error) { await reader.cancel().catch(()=>{}); throw error; }
  abort(signal);
  const bytes=new Uint8Array(length);let offset=0;for(const chunk of chunks){bytes.set(chunk,offset);offset+=chunk.length;}
  let data;try{data=JSON.parse(new TextDecoder().decode(bytes));}catch{throw new Error('素材解码服务返回了无效数据。');}
  if(!response.ok)throw new Error(data.error || '素材解码失败。'); return data;
}

export function validateDecodedMedia(data) {
  if(!data || typeof data.format!=='string' || data.format.length>80 || !Array.isArray(data.frames) || !data.frames.length || data.frames.length>256)throw new Error('素材解码结果格式无效。');
  let pixels=0,bytes=0;
  const frames=data.frames.map((frame,index)=>{
    if(!frame || typeof frame.src!=='string' || !/^data:image\/png;base64,[A-Za-z0-9+/]*={0,2}$/.test(frame.src))throw new Error('解码帧不是内嵌 PNG。');
    if(frame.src.length>Math.ceil(LIMITS.png/3)*4+32)throw new Error('解码图片过大。');
    const raw=atob(frame.src.slice(22)), png=Uint8Array.from(raw,char=>char.charCodeAt(0)), size=pngSize(png);
    if(size.width!==frame.width || size.height!==frame.height || !Number.isFinite(frame.durationMs) || frame.durationMs<0)throw new Error('解码帧尺寸或时长无效。');
    if(frame.durationMs>60000)throw new Error('素材含超过 60 秒的单帧；当前每帧最多 1500 tick，请先缩短源帧时长。');
    pixels+=size.width*size.height;bytes+=png.length;
    if(pixels>LIMITS.pixels || bytes>LIMITS.png)throw new Error('解码帧合计像素或 PNG 大小超过上限。');
    return {...size,name:String(frame.name || `frame-${index+1}.png`).slice(0,256),durationMs:frame.durationMs,src:frame.src,blob:new Blob([png],{type:'image/png'})};
  });
  if(!Array.isArray(data.clips ?? []) || (data.clips?.length ?? 0)>256)throw new Error('素材动作标签数量无效。');
  const clips=(data.clips ?? []).map(clip=>{
    if(!clip || typeof clip.name!=='string' || !clip.name.trim() || clip.name.length>256 || typeof clip.loop!=='boolean' || !Array.isArray(clip.indices) || !clip.indices.length || clip.indices.length>256 || clip.indices.some(index=>!Number.isInteger(index)||index<0||index>=frames.length))throw new Error('素材动作标签引用无效。');
    return {name:clip.name.slice(0,80),indices:[...clip.indices],loop:clip.loop};
  });
  const slices=(Array.isArray(data.slices)?data.slices:[]).slice(0,32).map(slice=>{
    if(!slice || typeof slice.name!=='string' || !slice.name.trim() || slice.name.length>80)throw new Error('切片名称无效。');
    for (const key of ['x','y','width','height']) if(!Number.isInteger(slice[key]) || slice[key]<-4096 || slice[key]>4096)throw new Error('切片矩形无效。');
    const placed={name:slice.name.trim(),x:slice.x,y:slice.y,width:slice.width,height:slice.height};
    if(slice.pivotX!=null || slice.pivotY!=null){
      if(!Number.isInteger(slice.pivotX)||!Number.isInteger(slice.pivotY))throw new Error('切片锚点无效。');
      placed.pivotX=slice.pivotX; placed.pivotY=slice.pivotY;
    }
    return placed;
  });
  return {format:data.format,name:stem(String(data.name || '导入素材')),frames,clips,slices,warnings:(Array.isArray(data.warnings)?data.warnings:[]).slice(0,32).map(w=>String(w).slice(0,512)),pixels,bytes};
}

async function atlas(files,signal) {
  const metadataFiles=files.filter(file=>/\.json$/i.test(file.name)), sheets=files.filter(file=>/\.png$/i.test(file.name));
  if(files.length!==2 || metadataFiles.length!==1 || sheets.length!==1)throw new Error('Aseprite 图集请选择一个 JSON 与它引用的一张 PNG。');
  const file=metadataFiles[0], sheet=sheets[0];if(file.size>LIMITS.json)throw new Error('图集 JSON 不能超过 1 MiB。');
  const text=await file.text();abort(signal);let metadata;try{metadata=JSON.parse(text);}catch{throw new Error('图集 JSON 无法读取。');}
  const bytes=new Uint8Array(await sheet.arrayBuffer());abort(signal);let binary='';for(let at=0;at<bytes.length;at+=32768)binary+=String.fromCharCode(...bytes.subarray(at,at+32768));
  return boundedJson(await fetch('/api/media/atlas',{method:'POST',headers:{'Content-Type':'application/json'},signal,body:JSON.stringify({metadata,sheetBase64:btoa(binary),sheetName:sheet.name,atlasName:file.name})}),signal);
}

/** Decode local rasters only through the bounded server (including .png APNG). */
export async function decodeMediaFiles(input,{signal,onProgress=()=>{}}={}) {
  const selected=[...input], accepted=selected.filter(file=>{
    if(!file.webkitRelativePath)return true;
    const components=file.webkitRelativePath.split('/'),base=components.at(-1);
    return base!=='.DS_Store' && !base.startsWith('._') && !components.includes('__MACOSX');
  });
  const ignored=selected.length-accepted.length;
  if(ignored && !accepted.length)throw new Error('所选文件夹只有系统元数据，没有可导入的图片。');
  const files=naturalOrder(accepted.map(file=>({file,name:pathOf(file)}))).map(item=>item.file);
  if(!files.length || files.length>256)throw new Error('一次请选择 1–256 个文件。');
  let total=0;
  for(const file of files) {
    if(!/\.(png|apng|gif|webp|jpe?g|json|zip|ase|aseprite|piskel|pxo)$/i.test(file.name))throw new Error('支持 PNG、GIF、APNG、WebP、JPEG、ZIP、Aseprite、Piskel、Pixelorama，或 Aseprite PNG + JSON。');
    const cap=/\.(zip|pxo)$/i.test(file.name)?LIMITS.zip:/\.json$/i.test(file.name)?LIMITS.json:LIMITS.file;
    if(!file.size || file.size>cap)throw new Error(`${file.name} 超过文件大小上限。`);
    total+=file.size;if(total>LIMITS.total)throw new Error('所选文件合计不能超过 32 MiB。');
    if(pathOf(file).length>256)throw new Error('素材文件名或文件夹路径不能超过 256 个字符。');
  }
  abort(signal);
  const results=[];let admittedFrames=0,admittedPixels=0,admittedBytes=0,admittedClips=0;
  function accept(data,path) {
    admittedFrames+=data.frames.length;admittedPixels+=data.pixels;admittedBytes+=data.bytes;admittedClips+=data.clips.length;
    if(admittedFrames>256 || admittedPixels>LIMITS.pixels || admittedBytes>LIMITS.png || admittedClips>256)throw new Error('整批解码结果超过帧数、像素、PNG 或动作标签上限。');
    results.push({data,path});
  }
  if(files.some(file=>/\.json$/i.test(file.name))) accept(validateDecodedMedia(await atlas(files,signal)),pathOf(files[0]));
  else for(let index=0;index<files.length;index++) {
    abort(signal);onProgress({completed:index,total:files.length,name:pathOf(files[index])});
    const response=await fetch(`/api/media/decode?name=${encodeURIComponent(files[index].name)}`,{method:'POST',body:files[index],signal});
    accept(validateDecodedMedia(await boundedJson(response,signal)),pathOf(files[index]));
  }
  abort(signal);
  const frames=[],clips=[],warnings=ignored?[`已忽略文件夹中的 ${ignored} 个 macOS 系统元数据文件。`]:[],directories=new Map();let pixels=0,bytes=0;
  let animated=false;
  for(const {data,path} of results) {
    const offset=frames.length;pixels+=data.pixels;bytes+=data.bytes;
    if(offset+data.frames.length>256 || pixels>LIMITS.pixels || bytes>LIMITS.png)throw new Error('整批解码结果超过 256 帧、16M 像素或 16 MiB PNG 上限。');
    frames.push(...data.frames);warnings.push(...data.warnings);
    const hasMotion=data.frames.length>1 && ['gif','apng','webp','aseprite','piskel','pixelorama'].includes(data.format) || ['aseprite','zip','piskel','pixelorama'].includes(data.format);
    animated ||= hasMotion;
    if(data.clips.length)clips.push(...data.clips.map(clip=>({...clip,indices:clip.indices.map(index=>index+offset)})));
    else if(hasMotion)clips.push({name:data.name,indices:data.frames.map((_,index)=>offset+index),loop:true});
    else { const directory=path.includes('/')?path.slice(0,path.lastIndexOf('/')):'';if(!directories.has(directory))directories.set(directory,[]);directories.get(directory).push(...data.frames.map((_,index)=>offset+index)); }
  }
  for(const [directory,indices] of directories)clips.push({name:stem(directory.split('/').at(-1)||results[0].data.name),indices,loop:true});
  if(!clips.length)clips.push({name:results[0].data.name,indices:frames.map((_,index)=>index),loop:true});
  onProgress({completed:files.length,total:files.length});
  return {format:results.length===1?results[0].data.format:'batch',name:results.length===1?results[0].data.name:stem(files[0].name),frames,clips,slices:results.length===1?results[0].data.slices:[],warnings:[...new Set(warnings)],fileNames:files.map(pathOf),animated,gridEligible:!animated && files.length===1 && frames.length===1};
}
