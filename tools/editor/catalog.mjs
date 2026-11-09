const escape = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;'}[c]));
const supported = source => source.kind === 'pack' && source.archiveUrl && source.archiveSha256 && source.archivePrefix !== undefined && source.packPrefix !== undefined;

export function setupCatalog({openProject, confirmChange, toast}) {
  const button = document.createElement('button');
  button.id = 'browse-materials'; button.className = 'quiet'; button.textContent = '素材目录';
  document.getElementById('open-project').before(button);
  const dialog = document.createElement('dialog');
  dialog.id = 'material-catalog';
  dialog.innerHTML = `<header class="catalog-header"><div><span class="eyebrow">MATERIAL LIBRARY</span><h2>从已有角色开始</h2></div><button id="close-catalog" class="icon-button" aria-label="关闭素材目录">×</button></header>
    <div class="catalog-search"><input id="catalog-query" placeholder="搜索角色、仓库或来源…" aria-label="搜索素材来源"><select id="catalog-filter" aria-label="筛选素材来源"><option value="all">全部来源</option><option value="pack">可导入动作</option><option value="directory">原站目录</option><option value="live2d">Live2D 对照</option></select></div>
    <p id="catalog-summary" class="catalog-summary"></p><div class="catalog-body"><nav id="catalog-items" aria-label="素材来源列表"></nav><section id="catalog-detail" aria-label="素材详情"></section></div>
    <footer class="catalog-footer"><span>原包可在桌宠的素材来源界面直接导入；编辑器读取整帧动作片段。</span><span id="catalog-status" role="status"></span></footer>`;
  document.body.append(dialog);
  let catalog, selected, busy = false, controller;
  const $ = id => document.getElementById(id);
  function render() {
    $('catalog-query').disabled = busy; $('catalog-filter').disabled = busy;
    const query = $('catalog-query').value.toLocaleLowerCase().trim(), kind = $('catalog-filter').value;
    const sources = catalog.sources.filter(s => (kind === 'all' || s.kind === kind) &&
      `${s.name} ${s.repository ?? ''} ${s.description ?? ''} ${s.structure ?? ''}`.toLocaleLowerCase().includes(query));
    if (!sources.some(s => s.id === selected)) selected = sources[0]?.id;
    $('catalog-summary').textContent = `${catalog.sources.length} 个已核实来源 · ${catalog.sources.filter(supported).length} 个可导入包 · 核验 ${catalog.checkedAt}`;
    $('catalog-items').innerHTML = sources.map(s => `<button class="catalog-item ${s.id === selected ? 'selected' : ''}" data-source="${escape(s.id)}" aria-pressed="${s.id === selected}"><strong>${escape(s.name)}</strong><small>${supported(s) ? 'PNG + XML · 可导入动作' : s.kind === 'live2d' ? 'Live2D · 结构对照' : '原站目录'}</small></button>`).join('') || '<p class="muted">没有匹配的来源。</p>';
    const source = sources.find(s => s.id === selected);
    $('catalog-detail').innerHTML = source ? `<span class="catalog-kind">${supported(source) ? '整帧动画包' : source.kind === 'live2d' ? 'Live2D 模型' : '素材来源'}</span><h3>${escape(source.name)}</h3><p>${escape(source.description)}</p>
      <dl><dt>内部结构</dt><dd>${escape(source.structure || '待具体包核验')}</dd>${source.pngCount !== undefined ? `<dt>文件统计</dt><dd>${source.pngCount} 张 PNG · ${source.actionCount ?? '—'} 个动作 · ${source.behaviorCount ?? '—'} 个行为</dd>` : ''}
      ${source.repository ? `<dt>仓库</dt><dd>${escape(source.repository)}</dd>` : ''}${source.revision ? `<dt>核实版本</dt><dd class="catalog-revision">${escape(source.revision)}</dd>` : ''}<dt>来源与使用说明</dt><dd>${escape(source.licenseNote)}</dd></dl>
      <div class="catalog-import-note">${supported(source) ? '读取后会得到可编辑的整帧动画，不会自动拆成头、手和身体。原包的表达式、行为状态机和移动物理，请在桌宠素材目录中原样导入使用。' : source.kind === 'live2d' ? '纹理图集和 moc3 不是独立 PNG 部件。当前仅供查看结构，不能直接导入为桌宠或部件动画。' : '从原作者页面选择素材，下载后可用桌宠的本地导入。这里只提供入口，不镜像站点素材。'}</div>
      <div class="catalog-actions"><a href="${escape(source.url)}" target="_blank" rel="noopener noreferrer">访问原始来源 ↗</a><button id="import-catalog-source" class="primary" ${!supported(source) || busy ? 'disabled' : ''}>${busy ? '正在读取…' : '导入动作到编辑器'}</button></div>` : '';
  }
  button.onclick = async () => {
    try {
      const response = await fetch('/api/catalog');
      if (!response.ok) throw new Error('无法读取素材目录');
      catalog = await response.json();
      selected ??= catalog.sources.find(supported)?.id;
      $('catalog-status').textContent = '';
      render(); dialog.showModal();
    } catch (error) { toast(error.message); }
  };
  $('close-catalog').onclick = () => dialog.close();
  dialog.addEventListener('close', () => { controller?.abort(); });
  $('catalog-query').oninput = render; $('catalog-filter').onchange = render;
  $('catalog-items').onclick = e => { const source = e.target.closest('[data-source]'); if (source && !busy) { selected = source.dataset.source; render(); } };
  $('catalog-detail').onclick = async e => {
    if (!e.target.closest('#import-catalog-source') || busy) return;
    const source = catalog.sources.find(s => s.id === selected);
    if (!supported(source)) return;
    if (!await confirmChange('读取这个角色的动作？', '将替换当前编辑工程。需要保留时，请先取消并保存工程。原包仅下载到本机，分层不会自动生成。')) return;
    busy = true; controller = new AbortController(); render();
    $('catalog-status').textContent = '下载、校验并提取动作…';
    try {
      const response = await fetch('/api/catalog/open', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({id: source.id}), signal: controller.signal});
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || '素材读取失败');
      if (!dialog.open) return;
      await openProject(result); dialog.close();
      toast(`已读取 ${result.actions.length} 个整帧动作、${result.assets.length} 张素材；源包没有自动拆分为部件。`);
    } catch (error) {
      if (error.name !== 'AbortError') { $('catalog-status').textContent = '读取失败，可访问原站后本地导入。'; toast(error.message); }
    } finally { busy = false; controller = null; if (dialog.open) render(); }
  };
}
