// Allocation/resource admission before handing untrusted, local GLB bytes to a
// renderer. Pure JS: no DOM, networking, image decoder or third-party imports.
export const VRM_LIMITS = Object.freeze({modelBytes: 32 * 1024 * 1024, motionBytes: 8 * 1024 * 1024,
  studioBytes: 60 * 1024 * 1024, jsonBytes: 4 * 1024 * 1024, nodes: 512, vertices: 200000,
  drawElements: 1000000, morphBytes: 64 * 1024 * 1024,
  materials: 128, texturePixels: 32 * 1024 * 1024, accessorComponents: 16 * 1024 * 1024, channels: 4096});
export const VRM_DEFAULT_RECIPE = Object.freeze({yaw: 0, pitch: 0, zoom: 1, targetY: 1, startSeconds: 0,
  durationTicks: 50, preset: 'idle', expression: 'neutral', expressionWeight: 0, loop: true, inPlace: true, spring: false});
export const VRM_STUDIO_VERSION = 1;
const FORMAT = 'shimeji2-vrm-studio';
const fail = message => { throw new Error(message); };
const own = (object, key) => Object.hasOwn(object, key);
const object = (value, name) => { if (!value || typeof value !== 'object' || Array.isArray(value)) fail(`${name} 需要是对象。`); return value; };
function integer(value, min, max, name) {
  if (!Number.isSafeInteger(value) || value < min || value > max) fail(`${name} 超出范围 ${min}–${max}。`);
  return value;
}
function finite(value, min, max, name) {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < min || value > max) fail(`${name} 需要是范围 ${min}–${max} 内的有限数值。`);
  return value;
}
function list(value, max, name) {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > max) fail(`${name} 数量超过上限 ${max} 或格式无效。`);
  return value;
}
function ref(value, values, name) { integer(value, 0, values.length - 1, `${name} 引用`); return values[value]; }
function vector(value, count, name) {
  if (!Array.isArray(value) || value.length !== count) fail(`${name} 长度无效。`);
  value.forEach(number => finite(number, -1e9, 1e9, name));
}
function bytesOf(input, cap, name) {
  const bytes = input instanceof ArrayBuffer ? new Uint8Array(input) : input instanceof Uint8Array ? input : null;
  if (!bytes || bytes.byteLength < 12 || bytes.byteLength > cap) fail(`${name} 大小需要在 12 字节与 ${cap / 1024 / 1024} MiB 上限之间。`);
  return bytes;
}

const CODECS = new Set(['KHR_draco_mesh_compression', 'EXT_meshopt_compression', 'KHR_texture_basisu', 'EXT_texture_webp', 'EXT_texture_avif', 'EXT_mesh_gpu_instancing']);
// Extensions understood by the bundled Three/VRM loaders without extra decoders.
const REQUIRED = new Set(['VRM', 'VRMC_vrm', 'VRMC_vrm_animation', 'VRMC_springBone', 'VRMC_node_constraint',
  'VRMC_materials_mtoon', 'VRMC_materials_hdr_emissiveMultiplier', 'KHR_materials_unlit', 'KHR_texture_transform',
  'KHR_mesh_quantization', 'KHR_lights_punctual', 'KHR_materials_clearcoat', 'KHR_materials_sheen', 'KHR_materials_transmission',
  'KHR_materials_ior', 'KHR_materials_volume', 'KHR_materials_specular', 'KHR_materials_iridescence',
  'KHR_materials_anisotropy', 'KHR_materials_emissive_strength', 'KHR_materials_dispersion']);

function inspectTree(json) {
  const stack = [[json, 0]]; let items = 0;
  while (stack.length) {
    const [value, depth] = stack.pop();
    if (++items > 250000 || depth > 64) fail('GLB JSON 结构超过深度或对象数量上限。');
    if (typeof value === 'number' && !Number.isFinite(value)) fail('GLB JSON 包含非有限数值。');
    if (!value || typeof value !== 'object') continue;
    if (!Array.isArray(value) && value.extensions) {
      object(value.extensions, 'extensions');
      for (const name of Object.keys(value.extensions)) if (CODECS.has(name)) fail(`不支持 ${name} 压缩或实例化扩展，请先导出普通 GLB。`);
    }
    for (const child of Object.values(value)) stack.push([child, depth + 1]);
  }
  for (const name of list(json.extensionsRequired, 64, 'extensionsRequired')) {
    if (!REQUIRED.has(name)) fail(`不支持必需扩展 ${name}。`);
  }
  list(json.extensionsUsed, 64, 'extensionsUsed').forEach(name => { if (typeof name !== 'string' || CODECS.has(name)) fail(`不支持扩展 ${name}。`); });
}

function parseGlb(input, cap) {
  const bytes = bytesOf(input, cap, 'GLB'), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.getUint32(0, true) !== 0x46546c67 || view.getUint32(4, true) !== 2 || view.getUint32(8, true) !== bytes.length) fail('GLB 文件头、版本或总长度无效。');
  let at = 12, json, bin;
  while (at < bytes.length) {
    if (at + 8 > bytes.length) fail('GLB chunk 文件头被截断。');
    const length = view.getUint32(at, true), type = view.getUint32(at + 4, true), start = at + 8;
    if (length % 4 || start + length > bytes.length) fail('GLB chunk 长度或边界无效。');
    if (type === 0x4e4f534a) {
      if (at !== 12 || json || length > VRM_LIMITS.jsonBytes) fail('GLB JSON chunk 顺序、数量或大小无效。');
      try { json = JSON.parse(new TextDecoder('utf-8', {fatal: true}).decode(bytes.subarray(start, start + length))); }
      catch { fail('GLB JSON 或 UTF-8 数据损坏。'); }
      object(json, 'GLB JSON');
    } else if (type === 0x004e4942) {
      if (!json || bin) fail('GLB BIN chunk 顺序或数量无效。');
      bin = bytes.subarray(start, start + length);
    } else fail('GLB 包含不支持的 chunk。');
    at = start + length;
  }
  if (!json || !bin) fail('只支持包含 JSON 与嵌入 BIN 的 GLB。');
  object(json.asset, 'asset');
  if (json.asset.version !== '2.0' || (json.asset.minVersion && json.asset.minVersion !== '2.0')) fail('只支持 glTF 2.0。');
  inspectTree(json);
  return {json, bin, bytes};
}

const COMPONENTS = {SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4, MAT2: 4, MAT3: 9, MAT4: 16};
const WIDTHS = {5120: 1, 5121: 1, 5122: 2, 5123: 2, 5125: 4, 5126: 4};
const READERS = {5120: 'getInt8', 5121: 'getUint8', 5122: 'getInt16', 5123: 'getUint16', 5125: 'getUint32', 5126: 'getFloat32'};

function validateResources(json, bin) {
  const buffers = list(json.buffers, 1, 'buffers');
  if (buffers.length !== 1) fail('只支持一个自包含 BIN buffer。');
  const buffer = object(buffers[0], 'buffer');
  if (own(buffer, 'uri')) fail('不允许 buffer URI，包括 data URI。');
  integer(buffer.byteLength, 1, bin.length, 'buffer byteLength');
  if (bin.length - buffer.byteLength > 3) fail('BIN 长度与 buffer 声明不匹配。');
  const views = list(json.bufferViews, 4096, 'bufferViews').map(raw => {
    const view = object(raw, 'bufferView'); ref(view.buffer, buffers, 'bufferView.buffer');
    const offset = integer(view.byteOffset ?? 0, 0, buffer.byteLength, 'bufferView byteOffset');
    const length = integer(view.byteLength, 1, buffer.byteLength, 'bufferView byteLength');
    if (offset + length > buffer.byteLength) fail('bufferView 超出 BIN 范围。');
    if (own(view, 'byteStride')) integer(view.byteStride, 4, 252, 'bufferView byteStride');
    if (view.byteStride % 4) fail('bufferView stride 需要 4 字节对齐。');
    return {raw: view, offset, length, stride: view.byteStride};
  });
  let decodedComponents = 0;
  const binary = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  const accessors = list(json.accessors, 4096, 'accessors').map(raw => {
    object(raw, 'accessor');
    if (!Number.isInteger(raw.componentType) || !own(WIDTHS, raw.componentType) || !own(COMPONENTS, raw.type)) fail('accessor componentType/type 不支持。');
    const size = WIDTHS[raw.componentType], components = COMPONENTS[raw.type];
    const count = integer(raw.count, 1, 2000000, 'accessor count');
    decodedComponents += count * components;
    if (decodedComponents > VRM_LIMITS.accessorComponents) fail('accessor 解码预算超过上限。');
    if (raw.normalized !== undefined && (typeof raw.normalized !== 'boolean' || (raw.normalized && [5125, 5126].includes(raw.componentType)))) fail('accessor normalized 无效。');
    const columns = raw.type.startsWith('MAT') ? Number(raw.type.at(-1)) : 1;
    const rows = components / columns;
    const columnBytes = columns > 1 ? Math.ceil(rows * size / 4) * 4 : rows * size;
    const elementBytes = columns * columnBytes;
    const offset = integer(raw.byteOffset ?? 0, 0, buffer.byteLength, 'accessor byteOffset');
    if (offset % size) fail('accessor byteOffset 未按 component 对齐。');
    let view;
    if (raw.bufferView !== undefined) {
      view = ref(raw.bufferView, views, 'accessor.bufferView');
      if ((view.offset + offset) % size) fail('accessor 绝对偏移未对齐。');
      if ((view.stride ?? elementBytes) < elementBytes || (view.stride ?? elementBytes) % size || offset + (count - 1) * (view.stride ?? elementBytes) + elementBytes > view.length) fail('accessor 超出 bufferView 范围。');
    } else if (own(raw, 'byteOffset')) fail('没有 bufferView 的 accessor 不能有 byteOffset。');
    for (const key of ['min', 'max']) if (raw[key] !== undefined) vector(raw[key], components, `accessor ${key}`);
    const componentOffset = component => Math.floor(component / rows) * columnBytes + component % rows * size;
    const read = (at, type = raw.componentType) => binary[READERS[type]](at, true);
    let sparse;
    if (raw.sparse !== undefined) {
      const s = object(raw.sparse, 'sparse'), length = integer(s.count, 1, count, 'sparse count');
      const indices = object(s.indices, 'sparse.indices'), values = object(s.values, 'sparse.values');
      if (![5121, 5123, 5125].includes(indices.componentType)) fail('sparse indices componentType 无效。');
      const iv = ref(indices.bufferView, views, 'sparse indices'), vv = ref(values.bufferView, views, 'sparse values');
      const io = integer(indices.byteOffset ?? 0, 0, iv.length, 'sparse indices offset'), vo = integer(values.byteOffset ?? 0, 0, vv.length, 'sparse values offset');
      const width = WIDTHS[indices.componentType];
      if (iv.stride || vv.stride || (iv.offset + io) % width || (vv.offset + vo) % size || io + length * width > iv.length || vo + length * elementBytes > vv.length) fail('sparse 数据范围或对齐无效。');
      let previous = -1;
      for (let i = 0; i < length; i++) {
        const index = read(iv.offset + io + i * width, indices.componentType);
        if (index <= previous || index >= count) fail('sparse indices 必须递增并位于 accessor 范围内。');
        previous = index;
      }
      sparse = {length, index: i => read(iv.offset + io + i * width, indices.componentType), start: vv.offset + vo};
    }
    function get(index, component = 0) {
      if (sparse) {
        let low = 0, high = sparse.length - 1;
        while (low <= high) { const middle = (low + high) >>> 1, target = sparse.index(middle); if (target === index) return read(sparse.start + middle * elementBytes + componentOffset(component)); if (target < index) low = middle + 1; else high = middle - 1; }
      }
      return view ? read(view.offset + offset + index * (view.stride ?? elementBytes) + componentOffset(component)) : 0;
    }
    // Float NaN/Infinity can poison skeleton and camera bounds. Scan every used
    // float with a strict global component budget, without constructing arrays.
    if (raw.componentType === 5126) for (let i = 0; i < count; i++) for (let c = 0; c < components; c++) if (!Number.isFinite(get(i, c))) fail('accessor 包含非有限数值。');
    return {raw, count, components, get};
  });
  return {views, accessors};
}

function imageSize(bytes, mime) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (mime === 'image/png') {
    if (bytes.length < 24 || view.getUint32(0) !== 0x89504e47 || view.getUint32(4) !== 0x0d0a1a0a || view.getUint32(8) !== 13 || view.getUint32(12) !== 0x49484452) fail('嵌入 PNG 文件头无效。');
    return [integer(view.getUint32(16), 1, 16384, 'PNG 宽度'), integer(view.getUint32(20), 1, 16384, 'PNG 高度')];
  }
  if (mime !== 'image/jpeg' || bytes.length < 4 || view.getUint16(0) !== 0xffd8) fail('嵌入 JPEG 文件头或图片格式无效，只支持 PNG/JPEG。');
  let at = 2;
  while (at < bytes.length) {
    if (bytes[at++] !== 0xff) fail('JPEG marker 无效。');
    while (at < bytes.length && bytes[at] === 0xff) at++;
    if (at >= bytes.length) break;
    const marker = bytes[at++];
    if (marker === 0xd9 || marker === 0xda) break;
    if (marker === 0x01 || marker >= 0xd0 && marker <= 0xd7) continue;
    if (at + 2 > bytes.length) break;
    const length = view.getUint16(at);
    if (length < 2 || at + length > bytes.length) fail('JPEG segment 超出范围。');
    if ([0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf].includes(marker)) {
      if (length < 8) fail('JPEG SOF 长度无效。');
      return [integer(view.getUint16(at + 5), 1, 16384, 'JPEG 宽度'), integer(view.getUint16(at + 3), 1, 16384, 'JPEG 高度')];
    }
    at += length;
  }
  fail('JPEG 未包含有效尺寸。');
}

function validateImages(json, bin, views) {
  const images = list(json.images, 128, 'images'); let pixels = 0;
  const imagePixels = [];
  for (const image of images) {
    object(image, 'image'); if (own(image, 'uri')) fail('不允许 image URI，包括 data URI。');
    const view = ref(image.bufferView, views, 'image.bufferView');
    if (view.stride) fail('image bufferView 不允许 stride。');
    const [width, height] = imageSize(bin.subarray(view.offset, view.offset + view.length), image.mimeType);
    imagePixels.push(width * height); pixels += width * height;
    if (pixels > VRM_LIMITS.texturePixels) fail('图片合计解码像素超过 32M 像素上限。');
  }
  const samplers = list(json.samplers, 128, 'samplers');
  const textures = list(json.textures, 256, 'textures');
  let texturePixels = 0;
  for (const texture of textures) {
    object(texture, 'texture'); ref(texture.source, images, 'texture.source'); if (texture.sampler !== undefined) ref(texture.sampler, samplers, 'texture.sampler');
    texturePixels += imagePixels[texture.source];
    if (texturePixels > VRM_LIMITS.texturePixels) fail('重复纹理实例合计像素超过 32M 像素上限。');
  }
  const materials = list(json.materials, VRM_LIMITS.materials, 'materials');
  function textureRefs(value) {
    if (!value || typeof value !== 'object') return;
    for (const [key, child] of Object.entries(value)) {
      if (key.endsWith('Texture') && child && typeof child === 'object') ref(child.index, textures, `${key}.index`);
      else textureRefs(child);
    }
  }
  for (const material of materials) textureRefs(object(material, 'material'));
  return {images, textures, materials, texturePixels: pixels};
}

function validateGraph(json, accessors, materials) {
  const nodes = list(json.nodes, VRM_LIMITS.nodes, 'nodes'), meshes = list(json.meshes, 512, 'meshes'), skins = list(json.skins, 128, 'skins');
  let vertices = 0, primitives = 0, drawElements = 0, morphBytes = 0;
  const indexMaxima = new Map(), meshVertices = [], meshDrawElements = [], meshMorphBytes = [];
  for (const mesh of meshes) {
    let meshVertexCount = 0, meshElements = 0, meshMorph = 0;
    object(mesh, 'mesh'); const entries = list(mesh.primitives, 512, 'mesh.primitives');
    if (!entries.length) fail('mesh 需要 primitives。');
    for (const primitive of entries) {
      object(primitive, 'primitive'); if (++primitives > 1024) fail('primitive 数量超过上限。');
      const attributes = object(primitive.attributes, 'attributes'), position = ref(attributes.POSITION, accessors, 'POSITION');
      if (position.raw.type !== 'VEC3') fail('POSITION 需要 VEC3。');
      meshVertexCount += position.count; vertices += position.count; if (vertices > VRM_LIMITS.vertices) fail('顶点总数超过 200000 上限。');
      for (const index of Object.values(attributes)) if (ref(index, accessors, 'attribute').count !== position.count) fail('mesh attribute 顶点数量不一致。');
      if (primitive.mode !== undefined) integer(primitive.mode, 0, 6, 'primitive.mode');
      if (primitive.material !== undefined) ref(primitive.material, materials, 'primitive.material');
      const elements = primitive.indices === undefined ? position.count : ref(primitive.indices, accessors, 'primitive.indices').count;
      drawElements += elements; meshElements += elements;
      // Check before scanning index values: repeated/shared index accessors still
      // issue their full count in each primitive's draw call.
      if (drawElements > VRM_LIMITS.drawElements) fail('mesh 绘制元素数量超过 1000000 上限。');
      if (primitive.indices !== undefined) {
        const indices = ref(primitive.indices, accessors, 'primitive.indices');
        if (indices.raw.type !== 'SCALAR' || ![5121, 5123, 5125].includes(indices.raw.componentType)) fail('mesh indices 格式无效。');
        if (!indexMaxima.has(primitive.indices)) {
          let maximum = 0; for (let i = 0; i < indices.count; i++) maximum = Math.max(maximum, indices.get(i));
          indexMaxima.set(primitive.indices, maximum);
        }
        if (indexMaxima.get(primitive.indices) >= position.count) fail('mesh index 超出顶点范围。');
      }
      const targets = list(primitive.targets, 128, 'morph targets');
      for (const target of targets) for (const [semantic, index] of Object.entries(object(target, 'morph target'))) {
        if (!['POSITION', 'NORMAL', 'TANGENT', 'COLOR_0'].includes(semantic)) fail('morph target 属性不支持。');
        const accessor = ref(index, accessors, 'morph target');
        if (accessor.count !== position.count || !(semantic === 'COLOR_0' ? ['VEC3', 'VEC4'].includes(accessor.raw.type) : accessor.raw.type === 'VEC3')) fail('morph target 顶点格式无效。');
      }
      // Pinned Three WebGLMorphtargets expands each target to an RGBA FLOAT
      // DataArrayTexture: position slot=1, normal=2, color=3. Shared accessors do
      // not reduce it. Row padding is strictly <2x for any maxTextureSize, so
      // budget that conservative CPU allocation; GPU mirrors it separately.
      const slots = targets.some(target => own(target, 'COLOR_0')) ? 3 : targets.some(target => own(target, 'NORMAL')) ? 2 : targets.some(target => own(target, 'POSITION')) ? 1 : 0;
      const expanded = position.count * slots * 4 * 4 * targets.length * 2;
      morphBytes += expanded; meshMorph += expanded;
      if (morphBytes > VRM_LIMITS.morphBytes) fail('展开 morph 纹理预算超过 64 MiB 上限。');
    }
    if (mesh.weights !== undefined) list(mesh.weights, 128, 'mesh.weights').forEach(weight => finite(weight, -100, 100, 'weight'));
    meshVertices.push(meshVertexCount);
    meshDrawElements.push(meshElements); meshMorphBytes.push(meshMorph);
  }
  const parents = new Set(); let renderedVertices = 0, renderedElements = 0, renderedMorph = 0;
  for (const node of nodes) {
    object(node, 'node');
    if (node.mesh !== undefined) {
      ref(node.mesh, meshes, 'node.mesh'); renderedVertices += meshVertices[node.mesh];
      if (renderedVertices > VRM_LIMITS.vertices) fail('重复 mesh 节点的总顶点数超过 200000 上限。');
      renderedElements += meshDrawElements[node.mesh]; renderedMorph += meshMorphBytes[node.mesh];
      if (renderedElements > VRM_LIMITS.drawElements) fail('重复 mesh 节点的绘制元素数量超过 1000000 上限。');
      if (renderedMorph > VRM_LIMITS.morphBytes) fail('重复 mesh 节点的展开 morph 纹理预算超过 64 MiB 上限。');
    }
    if (node.skin !== undefined) ref(node.skin, skins, 'node.skin');
    for (const [key, count] of [['translation', 3], ['rotation', 4], ['scale', 3], ['matrix', 16]]) if (node[key] !== undefined) vector(node[key], count, `node.${key}`);
    if (node.matrix && (node.translation || node.rotation || node.scale)) fail('node 不能同时定义 matrix 与 TRS。');
    for (const child of list(node.children, VRM_LIMITS.nodes, 'children')) {
      ref(child, nodes, 'node.children'); if (parents.has(child)) fail('node 包含重复父节点。'); parents.add(child);
    }
  }
  const visited = new Set(), active = new Set();
  function visit(index) {
    if (active.has(index)) fail('node 图包含循环。'); if (visited.has(index)) return;
    active.add(index); for (const child of nodes[index].children ?? []) visit(child);
    active.delete(index); visited.add(index);
  }
  nodes.forEach((_, i) => visit(i));
  const scenes = list(json.scenes, 64, 'scenes');
  for (const scene of scenes) for (const index of list(object(scene, 'scene').nodes, VRM_LIMITS.nodes, 'scene.nodes')) ref(index, nodes, 'scene.nodes');
  if (json.scene !== undefined) ref(json.scene, scenes, 'scene');
  for (const skin of skins) {
    object(skin, 'skin'); const joints = list(skin.joints, VRM_LIMITS.nodes, 'skin.joints');
    if (!joints.length || new Set(joints).size !== joints.length) fail('skin joints 为空或重复。');
    joints.forEach(index => ref(index, nodes, 'skin.joints'));
    if (skin.skeleton !== undefined) ref(skin.skeleton, nodes, 'skin.skeleton');
    if (skin.inverseBindMatrices !== undefined) {
      const accessor = ref(skin.inverseBindMatrices, accessors, 'inverseBindMatrices');
      if (accessor.raw.type !== 'MAT4' || accessor.raw.componentType !== 5126 || accessor.count !== joints.length) fail('inverseBindMatrices 格式或数量无效。');
    }
  }
  return {nodes, meshes, skins, vertices, drawElements: Math.max(drawElements, renderedElements), morphBytes: Math.max(morphBytes, renderedMorph)};
}

function validateAnimations(json, nodes, meshes, accessors, motion) {
  const animations = list(json.animations, 32, 'animations'); let channels = 0, durationSeconds = 0;
  const checkedTimes = new Map();
  for (const animation of animations) {
    object(animation, 'animation'); const samplers = list(animation.samplers, VRM_LIMITS.channels, 'animation.samplers');
    for (const sampler of samplers) {
      object(sampler, 'animation.sampler'); const input = ref(sampler.input, accessors, 'animation.input'); ref(sampler.output, accessors, 'animation.output');
      if (input.raw.type !== 'SCALAR' || input.raw.componentType !== 5126) fail('动画时间需要 FLOAT SCALAR。');
      if (!['LINEAR', 'STEP', 'CUBICSPLINE'].includes(sampler.interpolation ?? 'LINEAR')) fail('动画插值方式不支持。');
      if (!checkedTimes.has(sampler.input)) {
        let previous = -1;
        for (let i = 0; i < input.count; i++) {
          const value = input.get(i); if (!Number.isFinite(value) || value < 0 || value <= previous || value > 3600) fail('动画时间必须为有限、递增的 0–3600 秒。');
          previous = value;
        }
        checkedTimes.set(sampler.input, previous);
      }
      durationSeconds = Math.max(durationSeconds, checkedTimes.get(sampler.input));
    }
    const targets = new Set();
    for (const channel of list(animation.channels, VRM_LIMITS.channels, 'animation.channels')) {
      if (++channels > VRM_LIMITS.channels) fail('动画 channel 总数超过上限。');
      object(channel, 'channel'); const sampler = ref(channel.sampler, samplers, 'channel.sampler');
      const target = object(channel.target, 'channel.target'), node = ref(target.node, nodes, 'channel.target.node');
      const types = {translation: 'VEC3', rotation: 'VEC4', scale: 'VEC3', weights: 'SCALAR'};
      if (!own(types, target.path)) fail('动画 channel 路径不支持。');
      const key = `${target.node}:${target.path}`; if (targets.has(key)) fail('动画 channel 重复目标。'); targets.add(key);
      const input = accessors[sampler.input], output = accessors[sampler.output];
      let multiplier = sampler.interpolation === 'CUBICSPLINE' ? 3 : 1;
      if (target.path === 'weights') {
        const mesh = ref(node.mesh, meshes, 'weights node.mesh');
        const weights = mesh.primitives[0].targets?.length ?? 0; if (!weights) fail('weights 动画需要 morph targets。'); multiplier *= weights;
      }
      if (output.raw.type !== types[target.path] || output.raw.componentType !== 5126 || output.count !== input.count * multiplier) fail('动画 output 格式或采样数量不匹配。');
    }
  }
  if (motion && (!animations.length || !channels)) fail('VRMA 需要有效动画 channel。');
  return {animations: animations.length, channels, durationSeconds};
}

function extensionRefs(json, graph, resources, images) {
  const {nodes, meshes} = graph, {materials, textures} = images;
  const root = json.extensions ?? {};
  const vrm = root.VRMC_vrm ?? root.VRMC_vrm_animation;
  if (vrm?.humanoid) {
    const bones = object(vrm.humanoid.humanBones, 'humanoid.humanBones');
    if (Object.keys(bones).length > 128) fail('humanBones 数量超过上限。');
    for (const bone of Object.values(bones)) ref(object(bone, 'humanBone').node, nodes, 'humanBone.node');
  }
  if (vrm?.meta?.thumbnailImage !== undefined) ref(vrm.meta.thumbnailImage, images.images, 'meta.thumbnailImage');
  for (const group of Object.values(vrm?.expressions ?? {})) {
    if (!group || typeof group !== 'object') continue;
    for (const expression of Object.values(group)) {
      object(expression, 'expression');
      if (expression.node !== undefined) ref(expression.node, nodes, 'expression.node');
      for (const bind of list(expression.morphTargetBinds, 4096, 'morphTargetBinds')) {
        const node = ref(bind.node, nodes, 'morph bind.node'), mesh = ref(node.mesh, meshes, 'morph bind.mesh');
        integer(bind.index, 0, Math.min(...mesh.primitives.map(p => p.targets?.length ?? 0)) - 1, 'morph bind index');
        finite(bind.weight, 0, 1, 'morph bind weight');
      }
      for (const bind of [...list(expression.materialColorBinds, 1024, 'materialColorBinds'), ...list(expression.textureTransformBinds, 1024, 'textureTransformBinds')]) ref(bind.material, materials, 'expression material');
    }
  }
  if (vrm?.lookAt?.node !== undefined) ref(vrm.lookAt.node, nodes, 'lookAt.node');
  for (const annotation of list(vrm?.firstPerson?.meshAnnotations, 512, 'firstPerson.meshAnnotations')) ref(annotation.node, nodes, 'firstPerson.node');
  const old = root.VRM;
  if (old) {
    for (const bone of list(old.humanoid?.humanBones, 128, 'VRM humanBones')) ref(bone.node, nodes, 'VRM humanBone.node');
    if (old.meta?.texture !== undefined && old.meta.texture !== -1) ref(old.meta.texture, textures, 'VRM meta.texture');
    if (old.firstPerson?.firstPersonBone !== undefined && old.firstPerson.firstPersonBone !== -1) ref(old.firstPerson.firstPersonBone, nodes, 'VRM firstPersonBone');
    for (const annotation of list(old.firstPerson?.meshAnnotations, 512, 'VRM meshAnnotations')) ref(annotation.mesh, meshes, 'VRM meshAnnotation.mesh');
    for (const material of list(old.materialProperties, 128, 'VRM materialProperties')) {
      for (const index of Object.values(object(material.textureProperties ?? {}, 'VRM textureProperties'))) if (index !== -1) ref(index, textures, 'VRM material texture');
    }
    for (const group of list(old.blendShapeMaster?.blendShapeGroups, 128, 'blendShapeGroups')) for (const bind of list(group.binds, 4096, 'blendShape binds')) {
      const mesh = ref(bind.mesh, meshes, 'blendShape.mesh'); integer(bind.index, 0, Math.min(...mesh.primitives.map(p => p.targets?.length ?? 0)) - 1, 'blendShape.index');
    }
    const colliders = list(old.secondaryAnimation?.colliderGroups, 512, 'VRM colliderGroups'); colliders.forEach(collider => ref(collider.node, nodes, 'VRM collider.node'));
    for (const group of list(old.secondaryAnimation?.boneGroups, 512, 'VRM boneGroups')) {
      list(group.bones, 512, 'VRM bones').forEach(index => ref(index, nodes, 'VRM bone'));
      list(group.colliderGroups, 512, 'VRM colliderGroups').forEach(index => ref(index, colliders, 'VRM colliderGroup'));
      if (group.center !== undefined && group.center !== -1) ref(group.center, nodes, 'VRM center');
    }
  }
  const spring = root.VRMC_springBone;
  if (spring) {
    const colliders = list(spring.colliders, 512, 'spring colliders'); colliders.forEach(collider => ref(collider.node, nodes, 'spring collider.node'));
    const groups = list(spring.colliderGroups, 512, 'spring colliderGroups'); groups.forEach(group => list(group.colliders, 512, 'spring group colliders').forEach(index => ref(index, colliders, 'spring collider')));
    for (const entry of list(spring.springs, 512, 'springs')) {
      list(entry.joints, 512, 'spring joints').forEach(joint => ref(joint.node, nodes, 'spring joint.node'));
      list(entry.colliderGroups, 512, 'spring groups').forEach(index => ref(index, groups, 'spring group'));
      if (entry.center !== undefined) ref(entry.center, nodes, 'spring center');
    }
  }
  for (const node of nodes) {
    const constraint = node.extensions?.VRMC_node_constraint?.constraint;
    if (constraint) for (const entry of Object.values(constraint)) ref(entry.source, nodes, 'constraint.source');
  }
  // All resource indices used by mesh loading have been resolved above.
  return resources;
}

function inspect(input, motion) {
  const parsed = parseGlb(input, motion ? VRM_LIMITS.motionBytes : VRM_LIMITS.modelBytes), {json, bin, bytes} = parsed;
  const extension = json.extensions?.[motion ? 'VRMC_vrm_animation' : 'VRMC_vrm'] ?? (!motion ? json.extensions?.VRM : undefined);
  if (!extension) fail(motion ? '文件缺少 VRMA / VRMC_vrm_animation 扩展。' : '文件缺少 VRM 0/1 扩展。');
  object(extension, 'VRM extension');
  const version = !motion && json.extensions?.VRM && !json.extensions?.VRMC_vrm ? '0' : '1';
  if (version === '1' && !['1.0', '1.0-beta'].includes(extension.specVersion)) fail('不支持此 VRM/VRMA specVersion。');
  if (!motion && (!extension.humanoid || !extension.meta)) fail('VRM 缺少 humanoid 或 meta。');
  const resources = validateResources(json, bin), images = validateImages(json, bin, resources.views);
  const graph = validateGraph(json, resources.accessors, images.materials);
  const animation = validateAnimations(json, graph.nodes, graph.meshes, resources.accessors, motion);
  extensionRefs(json, graph, resources, images);
  const meta = extension.meta ?? {};
  const plain = value => typeof value === 'string' ? value.slice(0, 1024) : '';
  const authors = version === '0' ? [plain(meta.author)].filter(Boolean) : list(meta.authors, 128, 'meta.authors').map(plain).filter(Boolean);
  return {json, version, name: plain(meta.name ?? meta.title ?? json.animations?.[0]?.name), authors,
    stats: {bytes: bytes.length, nodes: graph.nodes.length, meshes: graph.meshes.length, vertices: graph.vertices,
      drawElements: graph.drawElements, morphBytes: graph.morphBytes,
      materials: images.materials.length, images: images.images.length, texturePixels: images.texturePixels, ...animation}};
}

export const inspectVrmGlb = input => inspect(input, false);
export const inspectVrmaGlb = input => inspect(input, true);

export function validateVrmRecipe(input = {}) {
  object(input, 'VRM recipe');
  for (const key of Object.keys(input)) if (!own(VRM_DEFAULT_RECIPE, key)) fail(`未知 VRM recipe 字段 ${key}。`);
  const result = {...VRM_DEFAULT_RECIPE, ...input};
  for (const [key, min, max] of [['yaw', -180, 180], ['pitch', -60, 60], ['zoom', .25, 4], ['targetY', 0, 2], ['startSeconds', 0, 3600], ['expressionWeight', 0, 1]]) finite(result[key], min, max, key);
  integer(result.durationTicks, 1, 250, 'durationTicks');
  if (!['idle', 'breathe', 'wave', 'dance', 'vrma'].includes(result.preset)) fail('VRM 动作预设无效。');
  if (typeof result.expression !== 'string' || result.expression.length > 80 || /[\x00-\x1f\x7f]/.test(result.expression)) fail('表情名称无效。');
  for (const key of ['loop', 'inPlace', 'spring']) if (typeof result[key] !== 'boolean') fail(`${key} 需要布尔值。`);
  return result;
}
export const validateRecipe = validateVrmRecipe;

function fileName(value, fallback) {
  if (value === undefined) return fallback;
  if (typeof value !== 'string' || !value.trim() || value.length > 255 || /[\/\\\x00-\x1f\x7f]/.test(value) || value === '.' || value === '..') fail('3D 工程文件名无效。');
  return value;
}
function hash(value) {
  if (value !== undefined && (typeof value !== 'string' || !/^[a-fA-F0-9]{64}$/.test(value))) fail('3D 来源 SHA-256 格式无效。');
  return value?.toLowerCase();
}
function encode64(bytes) {
  let result = '';
  for (let at = 0; at < bytes.length; at += 32768) result += String.fromCharCode(...bytes.subarray(at, at + 32768));
  return btoa(result);
}
function decode64(value, cap) {
  if (typeof value !== 'string' || value.length % 4 || value.length > 4 * Math.ceil(cap / 3) || !/^[A-Za-z0-9+/]*={0,2}$/.test(value)) fail('3D 来源 base64 格式或大小无效。');
  // Check canonical padding bits too: permissive atob otherwise hides corruption.
  const length = value.length / 4 * 3 - (value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0);
  if (length < 12 || length > cap) fail('3D 来源大小超过上限或不完整。');
  const decoded = atob(value), bytes = Uint8Array.from(decoded, char => char.charCodeAt(0));
  if (btoa(decoded) !== value) fail('3D 来源 base64 非规范编码。');
  return bytes.buffer;
}
function utf8Length(text) {
  let size = 0;
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code < 0x80) size++; else if (code < 0x800) size += 2;
    else if (code >= 0xd800 && code <= 0xdbff && text.charCodeAt(i + 1) >= 0xdc00 && text.charCodeAt(i + 1) <= 0xdfff) { size += 4; i++; }
    else size += 3;
    if (size > VRM_LIMITS.studioBytes) break;
  }
  return size;
}

// Optional hashes are provenance strings. The UI computes/verifies SHA-256 via
// WebCrypto; this synchronous module checks their format, never claims integrity.
export function serializeVrmStudioProject({modelBuffer, modelFileName, motionBuffer = null, motionFileName, recipe = {}, modelHash, motionHash}) {
  inspectVrmGlb(modelBuffer); if (motionBuffer) inspectVrmaGlb(motionBuffer);
  const normalized = validateVrmRecipe(recipe);
  if (normalized.preset === 'vrma' && !motionBuffer) fail('VRMA 预设需要动作文件。');
  if (!motionBuffer && (motionFileName != null || motionHash != null)) fail('没有 VRMA 数据但包含动作来源字段。');
  const project = {format: FORMAT, version: VRM_STUDIO_VERSION, modelFileName: fileName(modelFileName, 'avatar.vrm'),
    modelBase64: encode64(bytesOf(modelBuffer, VRM_LIMITS.modelBytes, 'VRM')), recipe: normalized};
  if (modelHash != null) project.modelHash = hash(modelHash);
  if (motionBuffer) {
    project.motionFileName = fileName(motionFileName, 'motion.vrma'); project.vrmaBase64 = encode64(bytesOf(motionBuffer, VRM_LIMITS.motionBytes, 'VRMA'));
    if (motionHash != null) project.motionHash = hash(motionHash);
  }
  const encoded = JSON.stringify(project); if (utf8Length(encoded) > VRM_LIMITS.studioBytes) fail('3D 工程大小超过 60 MiB 上限。');
  return encoded;
}

export function parseVrmStudioProject(encoded) {
  if (typeof encoded !== 'string' || encoded.length > VRM_LIMITS.studioBytes || utf8Length(encoded) > VRM_LIMITS.studioBytes) fail('3D 工程大小超过 60 MiB 上限。');
  let source; try { source = JSON.parse(encoded); } catch { fail('3D 工程 JSON 损坏。'); }
  object(source, '3D 工程');
  const keys = new Set(['format', 'version', 'modelFileName', 'modelBase64', 'motionFileName', 'vrmaBase64', 'recipe', 'modelHash', 'motionHash']);
  if (Object.keys(source).some(key => !keys.has(key)) || source.format !== FORMAT || source.version !== VRM_STUDIO_VERSION) fail('3D 工程格式或版本无效。');
  const modelBuffer = decode64(source.modelBase64, VRM_LIMITS.modelBytes);
  const motionBuffer = source.vrmaBase64 === undefined ? null : decode64(source.vrmaBase64, VRM_LIMITS.motionBytes);
  if (!motionBuffer && (own(source, 'motionFileName') || own(source, 'motionHash'))) fail('3D 工程缺少 VRMA 数据。');
  const recipe = validateVrmRecipe(source.recipe);
  if (recipe.preset === 'vrma' && !motionBuffer) fail('VRMA 预设需要动作文件。');
  return {modelBuffer, motionBuffer, modelFileName: fileName(source.modelFileName, 'avatar.vrm'),
    motionFileName: motionBuffer ? fileName(source.motionFileName, 'motion.vrma') : null, recipe,
    modelHash: hash(source.modelHash), motionHash: hash(source.motionHash),
    modelInfo: inspectVrmGlb(modelBuffer), motionInfo: motionBuffer ? inspectVrmaGlb(motionBuffer) : null};
}
