'use strict';
const $ = (q, root = document) => root.querySelector(q);
const $$ = (q, root = document) => [...root.querySelectorAll(q)];
const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const pretty = value => JSON.stringify(value, null, 2);
const labels = {overview:'概览', captures:'流量录制', scenarios:'场景与故障', reports:'回归报告'};
const state = {view:'overview', scenarios:[], reports:[], captures:[], status:{}, scenario:null, step:0, advanced:false, capture:null, captureId:null, selected:new Set(), flow:null, report:null, busy:false, dirty:false};
let toastTimer, polling;
const statusLabel = value => ({passed:'通过',failed:'失败',error:'执行错误',skipped:'跳过'}[value] || value);
const badge = (value, text) => `<span class="badge ${esc(value)}">${esc(text || statusLabel(value))}</span>`;
const date = timestamp => new Date(timestamp).toLocaleString('zh-CN', {hour12:false});
const bytes = size => size < 1024 ? `${size} B` : `${(size/1024).toFixed(1)} KB`;
function toast(message, error = false) {
  const el = $('#toast'); el.textContent = message; el.className = `visible${error?' error':''}`;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => el.className = '', error ? 9000 : 4500);
}
async function api(path, options = {}) {
  const response = await fetch(path, {...options, headers:{'content-type':'application/json',...options.headers}});
  const text = await response.text(); let data;
  try { data = JSON.parse(text); } catch { throw new Error(`服务返回 ${response.status}，请检查工作台日志`); }
  if (!response.ok) throw new Error(data.error || `请求失败 (${response.status})`);
  return data;
}
const post = (path, body) => api(path, {method:'POST', body:pretty(body)});
const action = (selector, callback) => {const element=$(selector); if(element)element.onclick=()=>Promise.resolve().then(callback).catch(error=>toast(error.message,true));};
function download(name, data, type = 'application/json') {
  const url = URL.createObjectURL(new Blob([typeof data==='string'?data:pretty(data)],{type}));
  const a = document.createElement('a'); a.href=url; a.download=name; a.click(); setTimeout(()=>URL.revokeObjectURL(url),1000);
}
function heading(title, subtitle, actions = '', eyebrow = 'REPRODUCTION WORKBENCH') {
  return `<div class="page-heading"><div><div class="eyebrow">${esc(eyebrow)}</div><h1>${esc(title)}</h1><p class="subtitle">${esc(subtitle)}</p></div><div class="actions">${actions}</div></div>`;
}
function empty(title, description, button = '') {return `<div class="empty"><div class="empty-icon">◇</div><strong>${esc(title)}</strong><p>${esc(description)}</p>${button}</div>`;}
function updateConnection() {
  const active = state.status.recording;
  $('#connection').className = `badge ${active?'success':'muted'}`;
  $('#connection').textContent = active?'● 正在录制':'● 本地已连接';
  $('#connection-dot').style.background='var(--green)';
  $('#version').textContent=`v${state.status.version}`;
  $('#workspace-label').textContent=state.status.workspace;
  $('#workspace-label').title=state.status.workspace;
}
async function refresh(renderView = true) {
  const [status,scenarios,reports,captures] = await Promise.all([api('/api/status'),api('/api/scenarios'),api('/api/reports'),api('/api/captures')]);
  Object.assign(state,{status,scenarios:scenarios.items,reports:reports.items,captures:captures.items});
  updateConnection();
  if (scenarios.warnings.length || reports.warnings.length) toast('部分工作空间文件无法读取，请检查 JSON 文档',true);
  if(renderView) render();
}
function navigate(view) {
  if(!labels[view])view='overview';
  state.view=view; history.replaceState(null,'',`#${view}`); render(); window.scrollTo(0,0);
}
function render() {
  $$('nav [data-view]').forEach(button=>{button.classList.toggle('active',button.dataset.view===state.view);button.setAttribute('aria-current',button.dataset.view===state.view?'page':'false');});
  $('#breadcrumb').textContent=labels[state.view];
  const renderers={overview:overview,captures:captures,scenarios:scenarios,reports:reports};
  renderers[state.view]();
}
function sparkline() {return `<svg class="spark" viewBox="0 0 100 30" aria-hidden="true"><path d="M0 24 L12 20 L24 23 L36 12 L48 15 L60 7 L72 12 L84 6 L100 2" fill="none" stroke="currentColor" stroke-width="2"/></svg>`;}
function overview() {
  const passed=state.reports.filter(r=>r.status==='passed').length;
  const ratio=state.reports.length?Math.round(passed/state.reports.length*100):null;
  const recent=state.reports.slice(0,5);
  $('#content').innerHTML=heading('让每个网络问题，都能被复现。','从一段流量到可重复的业务验证，在一个工作空间内完成。','<button id="open-capture">⇄ 录制流量</button><button class="primary" id="quick-demo">▷ 运行示例场景</button>')+
  `<section class="stats" aria-label="工作空间统计">
  ${[['场景库',state.scenarios.length,'可编辑、导出与版本管理','◇'],['流量录制',state.captures.length,state.status.recording?'正在接收代理流量':'导入 JSONL 或开启录制','⇄'],['回归运行',state.reports.length,'每次运行保存独立报告','▤'],['历史通过率',ratio===null?'—':`${ratio}%`,state.reports.length?`${passed} 次通过 / ${state.reports.length} 次运行`:'运行首个场景后生成统计','✓']].map(([label,value,note,icon])=>`<article class="stat"><div class="stat-label">${label}<span class="stat-icon">${icon}</span></div><div class="stat-line"><strong class="stat-value">${value}</strong>${label==='历史通过率'?'<span class="mono muted">ALL RUNS</span>':''}</div><div class="stat-note">${esc(note)}</div></article>`).join('')}</section>
  <div class="grid-two"><section class="card hero"><div class="card-body"><span class="badge accent">可运行的本地实验</span><div class="hero-title">从「偶尔出错」<br>到「每次都能验证」。</div><p>示例使用真实 HTTP 完成登录、创建订单和结果查询。先建立正常基线，再注入故障，观察回归如何定位变化。</p><div class="actions"><button class="primary" id="configure-demo">开始网络实验 ↗</button><span class="mono muted">3 STEPS · NO ACCOUNT REQUIRED</span></div><div class="flow-steps">${[['01','捕获真实流量','保留请求、响应及完整性诊断'],['02','构建业务场景','提取变量，连接有状态的操作'],['03','复现并验证','注入故障，精确检查业务结果']].map(([n,t,d])=>`<div class="flow-step"><span class="step-number">${n}</span><strong>${t}</strong><p>${d}</p></div>`).join('')}</div></div></section>
  <section class="card"><div class="card-header"><div><h2>为可信的回归而设计</h2><p class="card-subtitle">检查业务结果，而不只看 HTTP 200</p></div><span class="section-label">CORE</span></div><div class="card-body"><ul class="checklist">${[['有状态的多步骤场景','独立 cookie 会话与 JSON 变量提取'],['可解释的语义断言','状态、字段、类型、响应头与延迟'],['可重复的 HTTP 故障','503、延迟、字段变化和连接中断'],['可接入持续集成','JSON / JUnit 报告与失败退出码']].map(([t,d])=>`<li><span class="check-icon">✓</span><div><strong>${t}</strong><p>${d}</p></div></li>`).join('')}</ul></div></section></div>
  <section class="card"><div class="card-header"><div><h2>最近的回归运行</h2><p class="card-subtitle">正常验证与故障实验均计入历史记录</p></div><button class="quiet" id="all-reports">查看全部 →</button></div>${recent.length?reportTable(recent):empty('还没有运行记录','运行内置场景，几秒钟内得到第一份真实报告。','<button id="first-run" class="primary">▷ 运行第一个场景</button>')}</section>`;
  action('#open-capture',()=>navigate('captures'));
  action('#quick-demo',quickDemo);action('#first-run',quickDemo);
  action('#configure-demo',()=>openScenario('demo-checkout'));action('#all-reports',()=>navigate('reports'));bindReportRows();
}
function reportTable(items) {
  return `<div class="table-wrap"><table><thead><tr><th>场景 / 运行时间</th><th>结果</th><th>通过步骤</th><th>耗时</th><th>故障配置</th><th></th></tr></thead><tbody>${items.map(r=>`<tr class="clickable" data-report="${esc(r.id)}"><td><strong>${esc(r.scenario_name)}</strong><small>${date(r.started_at_ms)}</small></td><td>${badge(r.status)}</td><td class="mono">${r.passed} 通过 · ${r.failed} 失败</td><td class="mono">${r.duration_ms} ms</td><td>${r.fault?badge('warning',faultName(r.fault.kind)):badge('muted','正常基线')}</td><td class="muted">↗</td></tr>`).join('')}</tbody></table></div>`;
}
function bindReportRows() {$$('[data-report]').forEach(row=>row.onclick=()=>openReport(row.dataset.report).catch(e=>toast(e.message,true)));}
async function quickDemo() {await openScenario('demo-checkout'); await execute(false);}
async function openScenario(id) {
  state.scenario=await api(`/api/scenarios/${encodeURIComponent(id)}`);state.step=0;state.advanced=false;state.dirty=false;navigate('scenarios');
}
function field(label,id,value,type='text',extra='') {return `<label class="field" for="${id}">${label}<input id="${id}" type="${type}" value="${esc(value)}" ${extra}></label>`;}
function jsonField(label,id,value,rows=4) {return `<label class="field" for="${id}">${label}<textarea class="code" id="${id}" rows="${rows}" spellcheck="false">${esc(pretty(value))}</textarea></label>`;}
function scenarios() {
  const s=state.scenario;
  $('#content').innerHTML=heading('把请求连成一个业务故事。','编辑有状态场景，注入确定性故障，用断言验证每一步。','<button id="import-scenario-button">↑ 导入场景</button><button id="new-scenario" class="primary">＋ 新建场景</button>','SCENARIOS & FAULT INJECTION')+
  `<div class="workspace-grid"><aside class="card library"><div class="card-header"><h2>场景库</h2>${badge('muted',`${state.scenarios.length} 个`)}</div><div class="library-list">${state.scenarios.map(item=>`<button class="item ${s?.id===item.id?'active':''}" data-scenario="${esc(item.id)}"><span class="item-title">◇ ${esc(item.name)}</span><span class="item-meta">${item.steps} 个步骤 · ${esc(item.base_url)}</span></button>`).join('')||empty('场景库为空','创建或导入一个场景。')}</div></aside><div class="editor-pane">${s?scenarioEditor(s):`<div class="card">${empty('选择一个场景开始','内置订单场景可以直接运行，也可以创建自己的 API 流程。','<button id="choose-demo" class="primary">打开内置示例 →</button>')}</div>`}</div></div>`;
  action('#choose-demo',()=>openScenario('demo-checkout'));action('#new-scenario',newScenario);
  action('#import-scenario-button',()=>$('#import-scenario').click());
  $$('[data-scenario]').forEach(button=>button.onclick=async()=>{try{collectEditor();if(state.dirty){await saveScenario(false);}await openScenario(button.dataset.scenario);}catch(e){toast(e.message,true);}});
  if(s)bindScenario();
}
function scenarioEditor(s) {
  const step=s.steps[state.step];
  return `<section class="card"><div class="card-header"><div><h2>场景定义</h2><p class="card-subtitle">保存后可导出到 GitHub，并用 omni-scenario 在 CI 中运行</p></div><div class="actions"><button id="duplicate-scenario">复制</button><button id="export-scenario">↓ JSON</button><button id="save-scenario" class="primary">保存场景</button></div></div><div class="tabs"><button data-editor="visual" class="${!state.advanced?'active':''}">可视化编辑</button><button data-editor="json" class="${state.advanced?'active':''}">完整 JSON</button></div><div class="card-body">
  ${state.advanced?`<label class="field" for="scenario-json">场景 JSON · schema v1<textarea id="scenario-json" class="code large-editor" spellcheck="false">${esc(pretty(s))}</textarea></label>`:
  `<div class="field-row">${field('场景名称','scenario-name',s.name)}${field('目标 Origin（协议 + 主机 + 端口）','scenario-origin',s.base_url,'url')}</div><div class="field-row">${field('描述','scenario-description',s.description)}${field('请求超时（毫秒）','scenario-timeout',s.timeout_ms,'number','min="50" max="30000"')}</div>${jsonField('默认变量（JSON；敏感值建议留空，在运行时提供）','scenario-vars',s.variables||{},3)}<div class="step-list mt" role="group" aria-label="场景步骤">${s.steps.map((item,i)=>`<button data-step="${i}" class="${i===state.step?'active':''}">${i+1}. ${esc(item.name)}</button>`).join('')}<button id="add-step">＋ 步骤</button></div>
  <div class="step-box"><div class="actions mb"><h3>步骤 ${state.step+1}</h3><span class="mono muted">${esc(step.id)}</span><button class="quiet" id="remove-step" ${s.steps.length===1?'disabled':''}>移除此步骤</button></div>${field('步骤名称','step-name',step.name)}<div class="field-row three mt"><label class="field" for="step-method">方法<select id="step-method">${['GET','POST','PUT','PATCH','DELETE','HEAD','OPTIONS'].map(m=>`<option ${step.method===m?'selected':''}>${m}</option>`).join('')}</select></label>${field('相对路径（支持 {{变量}}）','step-path',step.path)}${field('预期状态码','step-status',step.expect.status??'','number','min="100" max="599"')}</div><div class="field-row">${jsonField('请求头（JSON 对象）','step-headers',step.headers||{},3)}${jsonField('提取变量（名称 → JSON Pointer）','step-extract',step.extract||{},3)}</div>${jsonField('请求 JSON body（null 表示不发送 JSON body）','step-body',step.body??null,4)}<details open><summary>断言与基线 · JSON / headers / ignore_json / assertions / max_latency_ms</summary>${jsonField('完整断言配置（可组合多个检查）','step-expect',step.expect,8)}<p class="hint">动态字段用 ignore_json；跨步骤数据用 extract，例如 {"order_id":"/id"}，后续路径使用 {{order_id}}。</p></details>${step.body_base64?'<div class="alert">此步骤包含原始 base64 body；请在完整 JSON 模式编辑和检查敏感内容。</div>':''}</div>`}
  <p class="hint mt">不会自动重试或跟随重定向。一个步骤失败后，依赖步骤会跳过。录制转场景后请配置动态字段与变量提取。</p></div></section>
  <section class="card run-panel"><div class="card-header"><div><h2>运行与故障实验</h2><p class="card-subtitle">运行时覆盖只影响本次报告，保持场景基线稳定</p></div><span class="badge accent">HTTP LEVEL</span></div><div class="card-body"><div class="field-row">${field('本次目标 Origin（可留空使用场景配置）','run-target','', 'url',`placeholder="${esc(s.base_url)}"`)}<label class="field" for="fault-kind">故障类型<select id="fault-kind"><option value="none">正常运行 · 无故障</option value="status">HTTP 503 · 服务不可用</option><option value="delay">延迟 · 超时与时延检查</option><option value="json">JSON 字段变化 · 语义回归</option><option value="invalid_json">无效 JSON · 解析失败</option><option value="disconnect">连接中断 · 响应体读取失败</option></select></label></div><div id="fault-fields" class="fault-fields hidden">${field('精确匹配路径（不含 query）','fault-path',s.id==='demo-checkout'?'/lab/api/orders':step.path.split('?')[0])}${field('延迟（毫秒）','fault-delay',1000,'number','min="0" max="30000"')}${field('JSON Pointer','fault-pointer','/total')}${field('替换值（JSON）','fault-value','999')}</div><div class="mt">${jsonField('本次变量覆盖（JSON；不会写入报告或默认变量）','run-vars',{},2)}</div><div class="run-footer"><p>运行会发送真实请求，请选择测试环境。<br>故障网关匹配该路径的每一次请求；不提供 TCP 包级仿真。</p><button id="run-scenario" class="primary" ${state.busy?'disabled':''}>${state.busy?'运行中…':'▷ 运行场景'}</button></div></div></section>`;
}
function parseField(selector,label,fallback) {const value=$(selector);if(!value)return fallback;try{return JSON.parse(value.value);}catch{throw new Error(`${label} 不是有效 JSON`);}}
function collectEditor() {
  if(state.view!=='scenarios'||!state.scenario)return;
  if(state.advanced){state.scenario=parseField('#scenario-json','场景',state.scenario);return;}
  const s=state.scenario;if(!$('#scenario-name'))return;
  s.name=$('#scenario-name').value;s.base_url=$('#scenario-origin').value;s.description=$('#scenario-description').value;s.timeout_ms=Number($('#scenario-timeout').value);s.variables=parseField('#scenario-vars','默认变量',{});
  const step=s.steps[state.step];step.name=$('#step-name').value;step.method=$('#step-method').value;step.path=$('#step-path').value;step.headers=parseField('#step-headers','请求头',{});step.extract=parseField('#step-extract','提取变量',{});step.expect=parseField('#step-expect','断言',{});
  const status=$('#step-status').value;if(status)step.expect.status=Number(status);else delete step.expect.status;
  const body=parseField('#step-body','请求 body',null);if(body===null)delete step.body;else{step.body=body;delete step.body_base64;}
}
function bindScenario() {
  $$('#content input, #content textarea, #content select').forEach(el=>el.addEventListener('input',()=>{state.dirty=true;}));
  $$('[data-step]').forEach(button=>button.onclick=()=>{try{collectEditor();state.step=Number(button.dataset.step);scenarios();}catch(e){toast(e.message,true);}});
  $$('[data-editor]').forEach(button=>button.onclick=()=>{try{collectEditor();state.advanced=button.dataset.editor==='json';state.step=Math.min(state.step,state.scenario.steps.length-1);scenarios();}catch(e){toast(e.message,true);}});
  action('#save-scenario',()=>saveScenario(true));
  action('#export-scenario',()=>{collectEditor();download(`${state.scenario.id}.json`,state.scenario);});
  action('#duplicate-scenario',()=>{collectEditor();state.scenario=structuredClone(state.scenario);state.scenario.id=crypto.randomUUID();state.scenario.name+='（副本）';state.dirty=true;scenarios();});
  action('#add-step',()=>{collectEditor();state.scenario.steps.push({id:`step-${crypto.randomUUID().slice(0,8)}`,name:'新的请求',method:'GET',path:'/',headers:{},extract:{},expect:{status:200}});state.step=state.scenario.steps.length-1;state.dirty=true;scenarios();});
  action('#remove-step',()=>{collectEditor();state.scenario.steps.splice(state.step,1);state.step=Math.max(0,state.step-1);state.dirty=true;scenarios();});
  $('#fault-kind').onchange=()=>$('#fault-fields').classList.toggle('hidden',$('#fault-kind').value==='none');
  if($('#step-status'))$('#step-status').oninput=()=>{try{const expect=JSON.parse($('#step-expect').value);if($('#step-status').value)expect.status=Number($('#step-status').value);else delete expect.status;$('#step-expect').value=pretty(expect);}catch{/* Keep invalid JSON visible for repair. */}};
  if($('#step-expect'))$('#step-expect').oninput=()=>{try{$('#step-status').value=JSON.parse($('#step-expect').value).status??'';}catch{/* Validate on save. */}};
  action('#run-scenario',()=>execute(true));
}
function newScenario() {
  state.scenario={schema_version:1,id:crypto.randomUUID(),name:'我的 API 回归场景',description:'',base_url:'http://127.0.0.1:8080',variables:{},timeout_ms:5000,steps:[{id:'step-1',name:'验证服务状态',method:'GET',path:'/',headers:{},extract:{},expect:{status:200}}]};state.step=0;state.advanced=false;state.dirty=true;navigate('scenarios');
}
async function saveScenario(notify) {
  collectEditor();await post('/api/scenarios',state.scenario);state.dirty=false;
  state.scenarios=(await api('/api/scenarios')).items;
  if(notify){toast('场景已保存');scenarios();}
}
async function execute(readControls) {
  if(state.busy)return;
  const options={variables:{},fault:null};
  if(readControls) {
    options.target=$('#run-target').value||null;options.variables=parseField('#run-vars','运行变量',{});
    const kind=$('#fault-kind').value;
    if(kind!=='none'){let value;try{value=JSON.parse($('#fault-value').value);}catch{throw new Error('故障替换值必须是有效 JSON');}options.fault={kind,path:$('#fault-path').value,delay_ms:Number($('#fault-delay').value),status:503,pointer:$('#fault-pointer').value,value};}
  }
  await saveScenario(false);state.busy=true;
  if($('#run-scenario')){$('#run-scenario').disabled=true;$('#run-scenario').textContent='正在执行真实 HTTP 请求…';}
  try {
    const report=await post(`/api/scenarios/${encodeURIComponent(state.scenario.id)}/run`,options);
    state.report=report;await refresh(false);navigate('reports');toast(report.status==='passed'?'全部断言通过，报告已保存':'检测到回归失败，查看具体断言定位变化',report.status!=='passed');
  } finally {state.busy=false;if(state.view==='scenarios')scenarios();}
}
function captures() {
  const active=state.status.recording;
  $('#content').innerHTML=heading('看见请求，保留问题现场。','录制或导入 JSONL，检查完整性，再将选中的请求生成场景。','<button id="import-capture-button">↑ 导入 JSONL</button>','TRAFFIC CAPTURE')+
  `<section class="recording-banner"><div><strong>${active?'<span class="pulse-dot"></span> 正在录制真实流量':'开启本地代理录制'}</strong><p>${active?`将应用代理指向 <code>${esc(active.proxy)}</code>`:'仅管理工作台启动的代理进程；在测试应用中设置 HTTP / HTTPS 代理。'}</p></div><div class="recording-actions">${active?'<button id="record-demo">录制示例流量</button><a class="badge accent" href="/api/ca" download="omni-proxy-ca.crt">↓ CA 证书</a><button id="stop-recording" class="danger">■ 停止录制</button>':'<label class="field" for="proxy-port"><span class="screen-reader">代理端口</span><input id="proxy-port" type="number" min="1024" max="65535" value="9090" aria-label="代理端口"></label><button class="primary" id="start-recording">● 开始录制</button>'}</div></section>
  <div class="alert info">HTTPS 需在测试客户端信任下载的本地 CA 证书。工作台不修改系统代理或系统证书。界面会屏蔽常见敏感字段；原始录制文件仍含完整流量。</div>
  <section class="card mb"><div class="toolbar"><label class="field grow" for="capture-select">选择录制<select id="capture-select"><option value="">选择一个录制文件…</option>${state.captures.map(c=>`<option value="${esc(c.id)}" ${state.captureId===c.id?'selected':''}>${esc(c.id.slice(0,8))} · ${bytes(c.bytes)} · ${date(c.modified_ms)}</option>`).join('')}</select></label><label class="field grow" for="flow-search">过滤请求<input id="flow-search" placeholder="搜索 URL、方法或状态码…" type="search"></label><button id="refresh-capture">↻ 更新流量</button></div></section><div id="capture-content">${captureContent()}</div>`;
  action('#import-capture-button',()=>$('#import-capture').click());
  action('#start-recording',async()=>{const button=$('#start-recording');button.disabled=true;button.textContent='启动中…';try{const result=await post('/api/recording/start',{port:Number($('#proxy-port').value)});state.captureId=result.id;toast(`录制已启动：${result.proxy}`);await refresh(false);await loadCapture(result.id);}finally{if(state.view==='captures')captures();}});
  action('#stop-recording',async()=>{await post('/api/recording/stop',{});await refresh(false);if(state.captureId)await loadCapture(state.captureId);toast('代理已停止，录制文件已保留');});
  action('#record-demo',async()=>{const button=$('#record-demo');button.disabled=true;try{await post('/api/recording/demo',{});await new Promise(r=>setTimeout(r,300));await refresh(false);await loadCapture(active.id);toast('已通过真实代理录制 3 个 HTTP 请求');}finally{if(state.view==='captures')captures();}});
  action('#refresh-capture',async()=>{await refresh(false);if(state.captureId)await loadCapture(state.captureId);else captures();});
  $('#capture-select').onchange=()=>loadCapture($('#capture-select').value).catch(e=>toast(e.message,true));
  $('#flow-search').oninput=()=>{drawFlowTable($('#flow-search').value);};bindCaptureContent();
}
async function loadCapture(id) {
  if(!id){state.capture=null;state.captureId=null;state.flow=null;state.selected.clear();captures();return;}
  if(state.captureId!==id){state.selected.clear();state.flow=null;}
  state.captureId=id;state.capture=await api(`/api/captures/${encodeURIComponent(id)}`);
  if(state.flow===null&&state.capture.requests.length)state.flow=state.capture.requests[0].index;
  if(state.view==='captures')captures();
}
function captureContent() {
  if(!state.capture)return `<section class="card">${empty('准备好捕获第一个问题','开始录制后可用“录制示例流量”检查完整链路，或导入现有 flow log。')}</section>`;
  const warnings=state.capture.issues;
  return `${warnings.length?`<div class="alert"><strong>${warnings.length} 项完整性提示</strong><br>${warnings.slice(0,8).map(esc).join('<br>')}${warnings.length>8?'<br>…':''}<br>录制进行中可能有尚未返回的请求。停止并刷新后，完整性检查仍不通过则无法生成回归基线。</div>`:''}<div class="capture-layout"><section class="card"><div class="card-header"><h2>请求列表 <span class="muted">${state.capture.requests.length}</span></h2>${badge(warnings.length?'warning':'success',warnings.length?'需要检查':'录制结构完整')}</div><div id="flow-table"></div><div class="toolbar"><label class="field grow" for="capture-scenario-name">生成场景名称<input id="capture-scenario-name" value="录制回归场景"></label><button id="capture-to-scenario" class="primary" ${warnings.length?'disabled':''}>选中 → 场景</button></div><div class="card-body hint">生成前选择同一 Origin 的请求。自动参数化常见敏感字段；动态 ID 和 token 的提取关系需在编辑器中配置。</div></section><section class="card inspector"><div class="card-header"><h2>请求与响应详情</h2><span class="section-label">INSPECTOR</span></div><div class="card-body" id="flow-detail"></div></section></div>`;
}
function drawFlowTable(filter='') {
  if(!$('#flow-table')||!state.capture)return;
  const rows=state.capture.requests.filter(r=>`${r.uri} ${r.method} ${r.response?.status||''}`.toLowerCase().includes(filter.toLowerCase()));
  $('#flow-table').innerHTML=rows.length?`<div class="table-wrap"><table><thead><tr><th><input type="checkbox" id="select-all" aria-label="选择所有显示的请求" ${rows.every(r=>state.selected.has(r.index))?'checked':''}></th><th>方法 / URL</th><th>状态</th><th>耗时</th></tr></thead><tbody>${rows.map(r=>`<tr class="clickable ${state.flow===r.index?'selected':''}" data-flow="${r.index}"><td><input type="checkbox" data-select="${r.index}" aria-label="选择请求 ${r.index}" ${state.selected.has(r.index)?'checked':''}></td><td class="url-cell"><strong class="mono">${esc(r.method)}</strong><small title="${esc(r.uri)}">${esc(r.uri)}</small></td><td>${badge(!r.replayable||r.response?.issues.length?'warning':r.response?.status>=400?'failed':'success',r.response?.status||'等待响应')}</td><td class="mono muted">${r.response?`${r.response.duration_ms} ms`:'—'}</td></tr>`).join('')}</tbody></table></div>`:empty('暂无匹配请求','刷新流量或调整搜索条件。');
  $$('[data-flow]').forEach(row=>row.onclick=event=>{if(event.target.matches('input'))return;state.flow=Number(row.dataset.flow);drawFlowTable(filter);drawFlowDetail();});
  $$('[data-select]').forEach(el=>el.onchange=()=>{const index=Number(el.dataset.select);if(el.checked)state.selected.add(index);else state.selected.delete(index);});
  if($('#select-all'))$('#select-all').onchange=()=>{const checked=$('#select-all').checked;rows.forEach(r=>checked?state.selected.add(r.index):state.selected.delete(r.index));drawFlowTable(filter);};
}
function drawFlowDetail() {
  if(!$('#flow-detail'))return;const request=state.capture.requests.find(r=>r.index===state.flow);
  if(!request){$('#flow-detail').innerHTML=empty('选择一个请求','查看请求头、响应和 body。');return;}
  const headers=items=>`<div class="key-values">${items.map(([k,v])=>`<span class="key">${esc(k)}</span><span class="mono">${esc(v)}</span>`).join('')}</div>`;
  const response=request.response;
  $('#flow-detail').innerHTML=`<div class="actions mb">${badge('accent',request.method)}${badge(response?.status>=400?'failed':'success',response?.status||'等待响应')}<span class="mono muted">#${request.index}</span></div><pre>${esc(request.uri)}</pre>${request.issues.length?`<div class="alert mt">${request.issues.map(esc).join('<br>')}</div>`:''}<h3>请求 Headers</h3>${headers(request.headers)}<h3>请求 Body</h3><pre>${esc(pretty(request.body))}</pre>${response?`<h3>响应 Headers</h3>${headers(response.headers)}<h3>响应 Body</h3><pre>${esc(pretty(response.body))}</pre>${response.issues.length?`<div class="alert mt">${response.issues.map(esc).join('<br>')}</div>`:''}`:'<p class="hint mt">尚无配对的响应。</p>'}`;
}
function bindCaptureContent() {
  drawFlowTable();drawFlowDetail();
  action('#capture-to-scenario',async()=>{if(!state.selected.size)throw new Error('请先勾选要生成场景的请求');const scenario=await post(`/api/captures/${encodeURIComponent(state.captureId)}/scenario`,{indexes:[...state.selected],name:$('#capture-scenario-name').value});await refresh(false);state.scenario=scenario;state.step=0;state.advanced=false;state.dirty=false;navigate('scenarios');toast('场景已生成，请配置动态变量提取后再运行');});
}
function faultName(kind) {return ({status:'HTTP 503',delay:'延迟',json:'JSON 字段变化',invalid_json:'无效 JSON',disconnect:'连接中断'}[kind]||kind);}
async function openReport(id) {state.report=await api(`/api/reports/${encodeURIComponent(id)}`);navigate('reports');}
function reports() {
  const r=state.report;
  $('#content').innerHTML=heading('每一次验证，都有据可查。','查看断言、定位语义变化，并把报告接入持续集成。',r?'<button id="report-json">↓ JSON 报告</button><button id="report-junit">↓ JUnit XML</button><button id="report-scenario">打开场景 ↗</button>':'','REGRESSION REPORTS')+
  (r?`<div class="workspace-grid"><aside class="card library"><div class="card-header"><h2>运行历史</h2><button class="quiet" id="report-list">全部 →</button></div><div class="library-list">${state.reports.map(item=>`<button class="item ${r.id===item.id?'active':''}" data-report="${esc(item.id)}"><span class="item-title">${item.status==='passed'?'✓':'×'} ${esc(item.scenario_name)}</span><span class="item-meta">${date(item.started_at_ms)} · ${item.fault?faultName(item.fault.kind):'正常运行'}</span></button>`).join('')}</div></aside><div class="editor-pane">${reportDetail(r)}</div></div>`:
  `<section class="card"><div class="card-header"><h2>全部运行</h2>${badge('muted',`${state.reports.length} 次`)}</div>${state.reports.length?reportTable(state.reports):empty('还没有回归报告','先运行场景，结果会自动保存到本地工作空间。','<button id="report-first-run" class="primary">▷ 运行示例</button>')}</section>`);
  bindReportRows();action('#report-first-run',quickDemo);action('#report-list',()=>{state.report=null;reports();});
  if(r){action('#report-json',()=>download(`${r.scenario_id}-${r.id.slice(0,8)}.json`,r));action('#report-junit',()=>{const a=document.createElement('a');a.href=`/api/reports/${encodeURIComponent(r.id)}/junit`;a.download='regression.xml';a.click();});action('#report-scenario',()=>openScenario(r.scenario_id));
    $('#compare-report').onchange=()=>compareReports($('#compare-report').value).catch(e=>toast(e.message,true));}
}
function reportDetail(r) {
  const previous=state.reports.filter(item=>item.id!==r.id&&item.scenario_id===r.scenario_id);
  return `<section class="report-summary"><div class="report-result"><span class="result-icon ${r.status==='passed'?'':'failed'}">${r.status==='passed'?'✓':'×'}</span><div><h2>${r.status==='passed'?'所有断言通过':'检测到回归失败'}</h2><p>${esc(r.scenario_name)} · ${date(r.started_at_ms)}</p></div></div><div class="report-numbers"><div><strong>${r.passed}/${r.steps.length}</strong><small>步骤通过</small></div><div><strong>${r.duration_ms}<span class="mono"> ms</span></strong><small>运行耗时</small></div></div></section>
  <div class="alert ${r.fault?'':'info'}">${r.fault?`故障配置：<strong>${esc(faultName(r.fault.kind))}</strong> · 精确路径 <code>${esc(r.fault.path)}</code>${r.fault.kind==='json'?` · ${esc(r.fault.pointer)} → ${esc(pretty(r.fault.value))}`:r.fault.kind==='delay'?` · ${r.fault.delay_ms} ms`:''}`:'正常运行 · 未注入故障'}<br>目标：${esc(r.target)} · Run ID：<span class="mono">${esc(r.id)}</span></div>
  ${r.steps.map((step,i)=>`<details class="report-step" ${step.status==='failed'||step.status==='error'?'open':''}><summary><strong>${i+1}. ${esc(step.name)}</strong><span class="actions">${badge(step.status)}<span class="mono muted">${step.http_status||'—'} · ${step.duration_ms} ms</span></span></summary><div class="report-step-content"><p class="mono mb">${esc(step.method)} ${esc(step.path)}</p>${step.checks.map(c=>`<div class="assertion ${c.passed?'':'fail'}"><span class="check-state">${c.passed?'✓':'×'}</span><code>${esc(c.field)}</code><p>${esc(c.message)}</p></div>`).join('')}${step.status==='skipped'?'<p class="hint">前序步骤失败，依赖步骤未发送。</p>':''}${step.extracted.length?`<p class="hint mt">已提取变量：${step.extracted.map(esc).join(', ')}（报告不记录变量值）</p>`:''}${step.response!==null?`<h3 class="mt">实际响应（常见敏感字段已屏蔽）</h3><pre>${esc(pretty(step.response))}</pre>`:''}</div></details>`).join('')}
  <section class="card compare"><div class="card-header"><h2>与历史运行比较</h2></div><div class="card-body"><label class="field" for="compare-report">选择同场景的对照运行<select id="compare-report"><option value="">选择一份历史报告…</option>${previous.map(item=>`<option value="${esc(item.id)}">${date(item.started_at_ms)} · ${statusLabel(item.status)} · ${item.fault?faultName(item.fault.kind):'正常运行'}</option>`).join('')}</select></label><div id="comparison" class="mt"><p class="hint">按步骤 ID 比较结果与耗时。响应内容可展开报告检查；此处不推断故障根因。</p></div></div></section>`;
}
async function compareReports(id) {
  if(!id){$('#comparison').textContent='请选择历史运行';return;}
  const other=await api(`/api/reports/${encodeURIComponent(id)}`);const current=state.report;
  const ids=[...new Set([...other.steps.map(s=>s.id),...current.steps.map(s=>s.id)])];
  $('#comparison').innerHTML=`<div class="table-wrap"><table><thead><tr><th>步骤</th><th>对照结果</th><th>当前结果</th><th>耗时变化</th></tr></thead><tbody>${ids.map(id=>{const a=other.steps.find(s=>s.id===id),b=current.steps.find(s=>s.id===id);const delta=a&&b?b.duration_ms-a.duration_ms:null;return `<tr><td>${esc(b?.name||a?.name)}</td><td>${a?badge(a.status):'无此步骤'}</td><td>${b?badge(b.status):'无此步骤'}</td><td class="mono">${delta===null?'—':`${delta>=0?'+':''}${delta} ms`}</td></tr>`;}).join('')}</tbody></table></div>`;
}
$$('nav [data-view]').forEach(button=>button.onclick=()=>{try{collectEditor();navigate(button.dataset.view);}catch(error){toast(error.message,true);}});
$('#refresh').onclick=()=>refresh().catch(e=>toast(e.message,true));
$('#theme').onclick=()=>{const theme=document.documentElement.dataset.theme==='dark'?'light':'dark';document.documentElement.dataset.theme=theme;localStorage.setItem('omni-theme',theme);};
document.documentElement.dataset.theme=localStorage.getItem('omni-theme')||(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light');
$('#import-scenario').onchange=async event=>{const file=event.target.files[0];if(!file)return;try{if(file.size>2*1024*1024)throw new Error('场景文件上限为 2 MiB');const scenario=JSON.parse(await file.text());await post('/api/scenarios',scenario);await refresh(false);await openScenario(scenario.id);toast('场景已导入');}catch(error){toast(error.message,true);}event.target.value='';};
$('#import-capture').onchange=async event=>{const file=event.target.files[0];if(!file)return;try{if(file.size>48*1024*1024)throw new Error('录制导入上限为 48 MiB');const result=await post('/api/captures',{content:await file.text()});await refresh(false);await loadCapture(result.id);toast('录制已导入');}catch(error){toast(error.message,true);}event.target.value='';};
document.addEventListener('keydown',event=>{if(event.key==='Escape'&&$('#toast').classList.contains('visible'))$('#toast').className='';if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='s'&&state.view==='scenarios'){event.preventDefault();saveScenario(true).catch(e=>toast(e.message,true));}});
window.addEventListener('beforeunload',event=>{if(state.dirty){event.preventDefault();event.returnValue='';}});
state.view=labels[location.hash.slice(1)]?location.hash.slice(1):'overview';
refresh().then(()=>{polling=setInterval(async()=>{try{const status=await api('/api/status');state.status=status;updateConnection();if(state.view==='captures'&&status.recording&&state.captureId===status.recording.id&&!document.activeElement?.matches('input,textarea,select')){state.capture=await api(`/api/captures/${encodeURIComponent(state.captureId)}`);const filter=$('#flow-search')?.value||'';const target=$('#capture-content');if(target){target.innerHTML=captureContent();bindCaptureContent();drawFlowTable(filter);}}}catch{ $('#connection').textContent='连接已断开';$('#connection').className='badge failed';$('#connection-dot').style.background='var(--red)';}},3000);}).catch(error=>{ $('#content').innerHTML=heading('无法连接本地工作台','请检查服务是否正在运行。')+`<div class="alert error">${esc(error.message)}</div>`; });
