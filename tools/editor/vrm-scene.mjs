import {THREE, GLTFLoader, VRMLoaderPlugin, VRMUtils, VRMAnimationLoaderPlugin, createVRMAnimationClip, VRMLookAtQuaternionProxy} from './vendor/vrm-libs.mjs';
import {inspectVrmGlb, inspectVrmaGlb, validateVrmRecipe, VRM_DEFAULT_RECIPE} from './vrm-model.mjs';
import {presetPose, frameAlphaStats, VRM_TICK_SECONDS} from './vrm-motion.mjs';

const nextPaint = () => new Promise(resolve => setTimeout(resolve, 0));
const cancelledError = () => new DOMException('操作已取消', 'AbortError');
const ownedBuffer = input => input instanceof ArrayBuffer ? input.slice(0) : input.buffer.slice(input.byteOffset, input.byteOffset + input.byteLength);

function releaseObject(object) {
  if (!object) return;
  const geometries = new Set(), materials = new Set(), textures = new Set(), images = new Set(), skeletons = new Set();
  object.traverse(node => {
    if (node.geometry) geometries.add(node.geometry);
    if (node.skeleton) skeletons.add(node.skeleton);
    for (const material of Array.isArray(node.material) ? node.material : [node.material]) if (material) {
      materials.add(material);
      for (const value of Object.values(material)) if (value?.isTexture) textures.add(value);
      for (const uniform of Object.values(material.uniforms ?? {})) if (uniform?.value?.isTexture) textures.add(uniform.value);
    }
  });
  for (const texture of textures) { if (texture.source?.data) images.add(texture.source.data); texture.dispose(); }
  for (const image of images) image.close?.();
  for (const material of materials) material.dispose();
  for (const geometry of geometries) geometry.dispose();
  for (const skeleton of skeletons) skeleton.dispose();
  object.removeFromParent();
}

/** A local, independently disposable WebGL studio. All times are local seconds. */
export async function createVrmScene(container, callbacks = {}) {
  const {onTime = () => {}, onError = () => {}, onMetadata = () => {}, onWarning = () => {}} = callbacks;
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({alpha:true, antialias:true, preserveDrawingBuffer:true, powerPreference:'low-power'});
  } catch { throw new Error('无法启动 3D 预览：此浏览器没有可用的 WebGL 2，请启用硬件加速后重试。'); }
  renderer.setPixelRatio(1);
  renderer.setSize(384, 384);
  renderer.setClearColor(0x000000, 0);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.NoToneMapping;
  const canvas = renderer.domElement;
  canvas.className = 'vrm-scene-canvas';
  canvas.style.width = '100%'; canvas.style.height = '100%'; canvas.style.objectFit = 'contain';
  canvas.setAttribute('aria-label', 'VRM 透明背景三维预览');
  container.append(canvas);
  const scene = new THREE.Scene();
  scene.add(new THREE.HemisphereLight(0xffffff, 0xb9c0ce, 2));
  const light = new THREE.DirectionalLight(0xffffff, 2.2); light.position.set(-1, 2, 3); scene.add(light);
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, .001, 1000);
  let vrm = null, animation = null, mixer = null, clipAction = null;
  let recipe = {...VRM_DEFAULT_RECIPE}, metadata = null;
  let height = 1, centerX = 0, floorY = 0, centerZ = 0;
  let generation = 0, animationGeneration = 0, disposed = false, contextLost = false, baking = false;
  let playing = false, raf = 0, previousTime = null, elapsed = 0, springTime = null;
  let restHips = null;

  function requireLive() {
    if (disposed) throw new Error('3D 场景已关闭');
    if (contextLost) throw new Error('3D 图形上下文已丢失，请关闭并重新打开工作室。');
  }
  function requireModel() { requireLive(); if (!vrm) throw new Error('请先导入 VRM 模型'); }
  function loadingManager() {
    const manager = new THREE.LoadingManager();
    manager.setURLModifier(url => {
      // GLTFLoader creates blob URLs for embedded buffer-view textures. Input
      // validators reject source URIs before any loader can run.
      if (url.startsWith(`blob:${location.origin}/`) || /^data:image\/(png|jpeg);base64,/i.test(url)) return url;
      throw new Error('VRM 只支持内嵌纹理，禁止读取外部资源');
    });
    return manager;
  }
  function loaderFor(kind) {
    const loader = new GLTFLoader(loadingManager());
    loader.register(parser => kind === 'model' ? new VRMLoaderPlugin(parser) : new VRMAnimationLoaderPlugin(parser));
    return loader;
  }
  function cameraFromRecipe() {
    const half = height * .65 / recipe.zoom;
    camera.left = -half; camera.right = half; camera.top = half; camera.bottom = -half;
    camera.near = Math.max(height / 10000, .0001); camera.far = Math.max(100, height * 20);
    const target = new THREE.Vector3(centerX, floorY + recipe.targetY * height / 2, centerZ);
    const yaw = recipe.yaw * Math.PI / 180, pitch = recipe.pitch * Math.PI / 180, distance = height * 3;
    camera.position.set(target.x + distance * Math.sin(yaw) * Math.cos(pitch), target.y + distance * Math.sin(pitch), target.z + distance * Math.cos(yaw) * Math.cos(pitch));
    camera.lookAt(target); camera.updateProjectionMatrix();
  }
  function bindAnimation() {
    mixer?.stopAllAction();
    if (mixer && vrm) mixer.uncacheRoot(vrm.scene);
    mixer = null; clipAction = null;
    if (!vrm || !animation) return;
    if (vrm.lookAt && !vrm.scene.children.some(child => child instanceof VRMLookAtQuaternionProxy)) {
      const proxy = new VRMLookAtQuaternionProxy(vrm.lookAt); proxy.name = 'VRMLookAtQuaternionProxy'; vrm.scene.add(proxy);
    }
    mixer = new THREE.AnimationMixer(vrm.scene);
    clipAction = mixer.clipAction(createVRMAnimationClip(animation, vrm));
    clipAction.setLoop(THREE.LoopRepeat, Infinity); clipAction.play();
  }
  function applyPose(localSeconds) {
    // Deactivate before resetting bones. PropertyMixer otherwise skips writing
    // unchanged tracks after a seek, leaving those bones in the reset T-pose.
    mixer?.stopAllAction();
    vrm.humanoid.resetNormalizedPose();
    vrm.expressionManager?.resetValues();
    vrm.lookAt?.reset();
    const seconds = recipe.startSeconds + localSeconds;
    if (recipe.preset === 'vrma') {
      if (!mixer || !clipAction) throw new Error('此动作需要先导入 VRMA 动画');
      const duration = clipAction.getClip().duration;
      const sample = recipe.loop ? ((seconds % duration) + duration) % duration : Math.max(0, Math.min(duration, seconds));
      // LoopOnce prevents a clamped final pose from wrapping through mixer.setTime.
      clipAction.setLoop(THREE.LoopOnce, 1); clipAction.clampWhenFinished = true;
      clipAction.reset().play(); mixer.setTime(sample);
    } else {
      const pose = presetPose(recipe.preset, seconds, height);
      for (const [bone, rotation] of Object.entries(pose.rotations)) {
        const node = vrm.humanoid.getNormalizedBoneNode(bone);
        node?.quaternion.setFromEuler(new THREE.Euler(...rotation, 'XYZ'));
      }
      const hips = vrm.humanoid.getNormalizedBoneNode('hips');
      if (hips && restHips) hips.position.copy(restHips).add(new THREE.Vector3(...pose.hips));
    }
    if (recipe.inPlace && restHips) {
      const hips = vrm.humanoid.getNormalizedBoneNode('hips');
      if (hips) { hips.position.x = restHips.x; hips.position.z = restHips.z; }
    }
    if (recipe.expression !== 'neutral' || recipe.expressionWeight > 0) vrm.expressionManager?.setValue(recipe.expression, recipe.expressionWeight);
    vrm.humanoid.update(); vrm.scene.updateMatrixWorld(true);
    vrm.lookAt?.update(0); vrm.expressionManager?.update(); vrm.nodeConstraintManager?.update();
    for (const material of vrm.materials ?? []) material.update?.(0);
  }
  function poseAt(localSeconds) {
    const tickTime = Math.max(0, Math.min(recipe.durationTicks - 1, Math.round(localSeconds / VRM_TICK_SECONDS))) * VRM_TICK_SECONDS;
    if (!recipe.spring || !vrm.springBoneManager) {
      vrm.springBoneManager?.reset(); applyPose(tickTime); springTime = null;
    } else {
      if (springTime == null || tickTime < springTime) {
        applyPose(0); vrm.springBoneManager.reset();
        // One second of fixed-step warmup at the initial pose; no wall time enters
        // simulation. Seeking and baking therefore begin with the same hair state.
        for (let tick = 0; tick < 25; tick++) { applyPose(0); vrm.springBoneManager.update(VRM_TICK_SECONDS); }
        springTime = 0;
      }
      while (springTime + VRM_TICK_SECONDS / 2 < tickTime) {
        springTime += VRM_TICK_SECONDS; applyPose(springTime); vrm.springBoneManager.update(VRM_TICK_SECONDS);
      }
    }
    vrm.scene.updateMatrixWorld(true);
  }
  function renderAt(seconds = elapsed) {
    requireModel();
    if (!Number.isFinite(seconds)) throw new Error('预览时间无效');
    elapsed = Math.max(0, Math.min(recipe.durationTicks * VRM_TICK_SECONDS, seconds));
    poseAt(elapsed); cameraFromRecipe(); renderer.render(scene, camera);
    return elapsed;
  }
  function renderNativePreview(target) {
    requireModel();
    if (baking) return;
    // Rasterize at the actual export resolution, rather than resampling the
    // larger preview (which gives different antialiasing and silhouette pixels).
    const size = renderer.getSize(new THREE.Vector2());
    const ctx = target.getContext('2d');
    try {
      renderer.setSize(128,128,false); renderer.render(scene,camera);
      ctx.clearRect(0,0,128,128); ctx.drawImage(canvas,0,0);
    } finally {
      renderer.setSize(size.x,size.y,false); renderer.render(scene,camera);
    }
  }
  function stopRaf() { if (raf) cancelAnimationFrame(raf); raf = 0; previousTime = null; }
  function animate(now) {
    raf = 0;
    if (!playing || disposed || contextLost || baking || document.hidden || !container.isConnected) return;
    try {
      if (previousTime != null) {
        const duration = recipe.durationTicks * VRM_TICK_SECONDS;
        elapsed += Math.min(.2, (now - previousTime) / 1000);
        if (elapsed >= duration) { if (recipe.loop) { elapsed %= duration; springTime = null; } else { elapsed = duration; playing = false; } }
      }
      previousTime = now; renderAt(elapsed); onTime(elapsed);
      if (playing) raf = requestAnimationFrame(animate);
    } catch (error) { playing = false; onError(error); }
  }
  function play(value) {
    requireLive(); playing = Boolean(value); stopRaf();
    if (playing && vrm && !document.hidden && !baking) raf = requestAnimationFrame(animate);
  }
  function visibilityChange() { stopRaf(); if (!document.hidden && playing && !baking && !disposed && !contextLost) raf = requestAnimationFrame(animate); }
  function lost(event) { event.preventDefault(); contextLost = true; playing = false; stopRaf(); onError(new Error('3D 图形上下文已丢失，请关闭并重新打开工作室。')); }
  document.addEventListener('visibilitychange', visibilityChange);
  canvas.addEventListener('webglcontextlost', lost);

  async function loadModel(buffer) {
    requireLive(); if (baking) throw new Error('请等待烘焙完成后再导入模型');
    const inspected = inspectVrmGlb(buffer), request = ++generation;
    const gltf = await loaderFor('model').parseAsync(ownedBuffer(buffer), '');
    const loaded = gltf.userData.vrm;
    if (disposed || request !== generation || contextLost) { releaseObject(gltf.scene); throw cancelledError(); }
    if (!loaded?.humanoid) { releaseObject(gltf.scene); throw new Error('文件没有可用的 VRM 人形骨骼'); }
    try {
      VRMUtils.rotateVRM0(loaded);
      loaded.humanoid.resetNormalizedPose(); loaded.humanoid.update(); loaded.scene.updateMatrixWorld(true);
      const bounds = new THREE.Box3().setFromObject(loaded.scene), size = bounds.getSize(new THREE.Vector3());
      if (!Number.isFinite(size.y) || size.y < .01 || size.y > 100 || !Number.isFinite(size.x) || size.x > 100 || !Number.isFinite(size.z) || size.z > 100) throw new Error('模型实际尺寸无效或过大');
      const center = bounds.getCenter(new THREE.Vector3());
      play(false); mixer?.stopAllAction(); if (mixer && vrm) mixer.uncacheRoot(vrm.scene);
      releaseObject(vrm?.scene); vrm = loaded;
      height = size.y; floorY = bounds.min.y; centerX = center.x; centerZ = center.z;
      restHips = vrm.humanoid.getNormalizedBoneNode('hips')?.position.clone();
      springTime = null; elapsed = 0; scene.add(vrm.scene); bindAnimation();
      metadata = {...inspected, height, expressions:Object.keys(vrm.expressionManager?.expressionMap ?? {})};
      // Loading a model must succeed even if a prior recipe needs a missing VRMA.
      if (recipe.preset === 'vrma' && !animation) recipe = {...recipe,preset:'idle'};
      renderAt(0); onMetadata(metadata); return metadata;
    } catch (error) { if (vrm !== loaded) releaseObject(loaded.scene); throw error; }
  }
  async function loadAnimation(buffer) {
    requireModel(); if (baking) throw new Error('请等待烘焙完成后再导入动画');
    const inspected = inspectVrmaGlb(buffer), request = ++animationGeneration, modelGeneration = generation;
    const gltf = await loaderFor('animation').parseAsync(ownedBuffer(buffer), '');
    try {
      if (disposed || request !== animationGeneration || modelGeneration !== generation || contextLost) throw cancelledError();
      const candidate = gltf.userData.vrmAnimations?.[0];
      if (!candidate || !Number.isFinite(candidate.duration) || candidate.duration <= 0) throw new Error('文件没有可用的 VRMA 动画');
      animation = candidate; bindAnimation(); springTime = null;
      renderAt(0); return {...inspected,durationSeconds:candidate.duration};
    } finally { releaseObject(gltf.scene); }
  }
  function setRecipe(input) {
    requireLive(); if (baking) throw new Error('烘焙期间不能修改场景');
    const candidate = validateVrmRecipe(input);
    if (candidate.preset === 'vrma' && !animation) throw new Error('此动作需要先导入 VRMA 动画');
    recipe = candidate; springTime = null;
    if (vrm) renderAt(elapsed);
    return {...recipe};
  }
  async function bake({recipe:input = recipe, onProgress = () => {}, isCancelled = () => false} = {}) {
    requireModel(); if (baking) throw new Error('已有烘焙任务正在运行');
    const nextRecipe = validateVrmRecipe(input);
    if (nextRecipe.preset === 'vrma' && !animation) throw new Error('请先导入 VRMA 动画');
    const previousRecipe = recipe, previousElapsed = elapsed, resume = playing, request = generation;
    play(false); baking = true; recipe = nextRecipe; springTime = null;
    const assets = [], alpha = new Uint8Array(128 * 128 * 4); let clippedFrames = 0;
    const check = () => { if (disposed || contextLost || request !== generation || isCancelled()) throw cancelledError(); };
    try {
      renderer.setSize(128, 128, false);
      for (let tick = 0; tick < recipe.durationTicks; tick++) {
        check(); renderAt(tick * VRM_TICK_SECONDS);
        const gl = renderer.getContext(); gl.readPixels(0, 0, 128, 128, gl.RGBA, gl.UNSIGNED_BYTE, alpha);
        const stats = frameAlphaStats(alpha);
        if (!stats.visiblePixels) throw new Error(`第 ${tick + 1} 帧没有可见模型，请调整镜头后重试`);
        if (!stats.transparentPixels) throw new Error(`第 ${tick + 1} 帧没有透明背景，请缩小镜头后重试`);
        if (stats.clipped) clippedFrames++;
        assets.push({src:canvas.toDataURL('image/png'),name:`VRM-${String(tick + 1).padStart(3,'0')}.png`});
        onProgress({completed:tick + 1,total:recipe.durationTicks,clippedFrames});
        if (tick % 4 === 3) await nextPaint();
      }
      check(); if (clippedFrames) onWarning(`${clippedFrames} 帧接触画布边缘，可减小缩放避免裁切。`);
      return {assets,ticks:1,frameCount:assets.length,clippedFrames};
    } finally {
      baking = false; recipe = previousRecipe; elapsed = previousElapsed; springTime = null;
      if (!disposed && !contextLost) { renderer.setSize(384,384,false); renderAt(elapsed); if (resume) play(true); }
    }
  }
  function dispose() {
    if (disposed) return;
    disposed = true; generation++; animationGeneration++; playing = false; stopRaf();
    document.removeEventListener('visibilitychange',visibilityChange); canvas.removeEventListener('webglcontextlost',lost);
    mixer?.stopAllAction(); if (mixer && vrm) mixer.uncacheRoot(vrm.scene);
    releaseObject(vrm?.scene); vrm = null; mixer = null; clipAction = null; animation = null;
    renderer.renderLists.dispose(); renderer.dispose(); renderer.forceContextLoss(); canvas.remove();
  }
  return {canvas,loadModel,loadAnimation,setRecipe,renderAt,renderNativePreview,play,bake,dispose,getMetadata:()=>metadata,getRecipe:()=>({...recipe}),getTime:()=>elapsed};
}
