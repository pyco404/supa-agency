// Supa Agency / SUPA
// One agent. One purpose. One token.
// Persistent agents. Application-enforced permissions.
import { createWheel, createCardMedia } from './wheel.js';
import { createScenes } from './scene.js';
import { navigation as privateNavigation, publicNavigation, publicSectionView, sectionArtwork, navigationMotion, githubNotice, overview, sectionView, read, units } from './platform.js';

const $ = selector => document.querySelector(selector);
const root = document.documentElement;
const publicLanding = root.dataset.publicLanding === 'true';
const navigation = publicLanding ? publicNavigation : privateNavigation;
// Visits belong to this document: a full reload starts a fresh typing sequence.
// Never persist page content, authorization or typing history in browser storage.
const typedSections=new Set();
function repeatSection(id){
  const repeat=typedSections.has(id);typedSections.add(id);
  return repeat;
}
// Only source-matched, approved loops belong in page artwork. Intelligence's
// Purpose Qualification illustration has no matching loop and remains static.
const pageArtworkMotion=Object.freeze({factory:navigationMotion.factory});
const artworkMedia=new Set();
function clearArtworkMedia(){for(const item of artworkMedia)item.destroy();artworkMedia.clear();}
function syncArtworkMedia(){for(const item of artworkMedia)item.sync();}
function animateArtwork(picture,id,poster){
  if(!pageArtworkMotion[id]||!('IntersectionObserver' in window))return;
  const media=createCardMedia({thumb:poster,media:pageArtworkMotion[id]});
  media.element.classList.add('section-motion-media');picture.append(media.element);
  let visible=false;
  const item={sync(){media.setActive(visible&&picture.isConnected&&!reduced.matches&&!document.hidden&&document.body.dataset.view==='projects'&&!$('.pane').inert);},destroy(){observer.disconnect();media.destroy();}};
  const observer=new IntersectionObserver(entries=>{visible=entries.some(entry=>entry.isIntersecting);item.sync();},{threshold:0});
  artworkMedia.add(item);observer.observe(picture);
}
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
reduced.addEventListener('change', event => { root.dataset.motion=event.matches?'still':'running';syncArtworkMedia();if(event.matches)revealTyping?.(); });
root.dataset.motion = reduced.matches?'still':'running';
document.addEventListener('visibilitychange',syncArtworkMedia);
const node = (tag,className,text) => { const el=document.createElement(tag);if(className)el.className=className;if(text!==undefined)el.textContent=String(text);return el; };
const palettes=[['#080908','#ded9bd','#aaa58b'],['#0b0b0a','#e6e0cf','#a7a18f'],['#0a0c08','#d9dbc3','#a7ac90']];
let sceneData, revealTyping=null;
let theme=0,wheel,scenes,itemIndex=0,typing=0,typingTimer=0,leaving=false,entryAttempt=0,worm=false;
let state={status:{error:'Connecting to local backend…'},agents:{error:'Loading saved agents…'},retrievedAt:'not yet retrieved'},agentId='',renderVersion=0,reloadVersion=0;
let commandHistory=[],historyIndex=0,liveTimer=0,liveBusy=false,viewFingerprint='';
try{const saved=publicLanding?null:localStorage.getItem('supa.selected-agent');if(/^[0-9a-f-]{36}$/i.test(saved??''))agentId=saved;}catch{/* Optional private identity preference. */}
function rememberAgent(){try{if(agentId)localStorage.setItem('supa.selected-agent',agentId);else localStorage.removeItem('supa.selected-agent');}catch{/* Authentication never depends on browser storage. */}}
$('.buffer').addEventListener('input',event=>{const form=event.target.closest('form');if(form)form.dataset.unsavedChanges='true';});
$('.buffer').addEventListener('change',event=>{const form=event.target.closest('form');if(form)form.dataset.unsavedChanges='true';});
let autonomyAttempt=null;
try{const id=sessionStorage.getItem('supa.offline-request');if(/^[0-9a-f-]{36}$/i.test(id??''))autonomyAttempt={id,status:'unknown'};}catch{/* Optional recovery hint; server UUID uniqueness remains authoritative. */}
try{const saved=Number(localStorage.getItem('supa.dark-theme'));if(Number.isInteger(saved)&&saved>=0&&saved<palettes.length)theme=saved;}catch{/* Optional preference. */}
function applyTheme(){['--bg','--ink','--dim'].forEach((name,index)=>root.style.setProperty(name,palettes[theme][index]));}
applyTheme();
let characterShortcuts=true;
try{characterShortcuts=localStorage.getItem('supa.character-shortcuts')!=='off';}catch{/* Optional local preference. */}
const shortcutControl=$('.shortcut-toggle');
function syncShortcutControl(){shortcutControl.setAttribute('aria-pressed',String(characterShortcuts));shortcutControl.textContent=`Global character shortcuts ${characterShortcuts?'on':'off'}`;}
syncShortcutControl();
shortcutControl.addEventListener('click',()=>{
  characterShortcuts=!characterShortcuts;syncShortcutControl();
  try{localStorage.setItem('supa.character-shortcuts',characterShortcuts?'on':'off');}catch{/* Optional local preference. */}
  $('.announcement').textContent=`${shortcutControl.textContent}. Arrow keys and focused terminal/navigation controls remain available.`;
});
let pickerSearch='',pickerSearchAt=0;
// Keep the native value as backing state; the visible picker never uses an OS popup.
function closeAgentPicker(focus=false){pickerSearch='';pickerSearchAt=0;$('#agent-options').hidden=true;$('#agent-picker-trigger').setAttribute('aria-expanded','false');if(focus)$('#agent-picker-trigger').focus({preventScroll:true});}
function syncAgentPicker(){
  const native=$('#agent-select'),list=$('#agent-options'),trigger=$('#agent-picker-trigger'),restore=list.contains(document.activeElement);
  $('#agent-selected-name').textContent=native.selectedOptions[0]?.textContent ?? 'Choose an agent';trigger.title=$('#agent-selected-name').textContent;trigger.disabled=native.options.length<2;
  list.replaceChildren();
  for(const option of native.options){
    const button=node('button','agent-option',option.textContent);button.type='button';button.setAttribute('role','option');button.setAttribute('aria-selected',String(option.value===native.value));button.tabIndex=-1;
    button.addEventListener('click',()=>{native.value=option.value;native.dispatchEvent(new Event('change'));closeAgentPicker(true);});list.append(button);
  }
  closeAgentPicker(restore);
}
function openAgentPicker(last=false){
  const trigger=$('#agent-picker-trigger'),list=$('#agent-options');if(trigger.disabled)return;
  const box=trigger.getBoundingClientRect(),below=Math.max(0,innerHeight-box.bottom-12),above=Math.max(0,box.top-12),up=below<180&&above>below;
  list.style.width=`${Math.min(box.width,innerWidth-16)}px`;list.style.left=`${Math.max(8,Math.min(box.left,innerWidth-box.width-8))}px`;
  list.style.top=up?'auto':`${box.bottom+4}px`;list.style.bottom=up?`${innerHeight-box.top+4}px`:'auto';list.style.maxHeight=`${Math.min(240,up?above:below)}px`;
  list.hidden=false;trigger.setAttribute('aria-expanded','true');
  const options=[...list.children],active=last?options.at(-1):options.find(option=>option.getAttribute('aria-selected')==='true')??options[0];active?.focus({preventScroll:true});active?.scrollIntoView({block:'nearest'});
}
$('#agent-picker-trigger').addEventListener('click',()=>$('#agent-options').hidden?openAgentPicker():closeAgentPicker());
$('.agent-picker').addEventListener('keydown',event=>{
  if(event.ctrlKey||event.metaKey||event.altKey)return;
  const options=[...$('#agent-options').children],index=options.indexOf(document.activeElement);
  if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
    event.preventDefault();event.stopPropagation();if($('#agent-options').hidden){openAgentPicker(event.key==='ArrowUp'||event.key==='End');return;}
    const next=event.key==='Home'?0:event.key==='End'?options.length-1:Math.max(0,Math.min(options.length-1,index+(event.key==='ArrowDown'?1:-1)));options[next]?.focus({preventScroll:true});options[next]?.scrollIntoView({block:'nearest'});
  }else if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeAgentPicker(true);}else if(event.key==='Tab')closeAgentPicker(true);else if(event.key.length===1&&event.key!==' '){
    event.preventDefault();event.stopPropagation();if($('#agent-options').hidden)openAgentPicker();
    const now=performance.now(),key=event.key.toLowerCase();pickerSearch=now-pickerSearchAt<700?pickerSearch+key:key;pickerSearchAt=now;
    const cycle=[...pickerSearch].every(character=>character===key),query=cycle?key:pickerSearch,start=cycle?Math.max(0,options.indexOf(document.activeElement)+1):0;
    const match=[...options.slice(start),...options.slice(0,start)].find(option=>option.textContent.toLowerCase().startsWith(query));
    match?.focus({preventScroll:true});match?.scrollIntoView({block:'nearest'});
  }
});
document.addEventListener('pointerdown',event=>{if(!$('.agent-picker').contains(event.target))closeAgentPicker();});
window.addEventListener('resize',()=>closeAgentPicker($('#agent-options').contains(document.activeElement)));
document.addEventListener('scroll',event=>{const target=event.target;if(target===document||target instanceof Element&&target.contains($('#agent-picker-trigger')))closeAgentPicker($('#agent-options').contains(document.activeElement));},true);
let restoringRoute=false;
const ready=boot();
async function boot(){
  const data=sceneData={days:[],account:'Supa Agency',connection:'connecting',agentCount:null,retrievedAt:null};
  scenes=createScenes({intro:$('.intro-canvas'),blueprint:$('.blueprint'),entity:$('.entity'),transition:$('.transition'),spark:$('.spark'),reduced,data});
  document.fonts.ready.then(()=>scenes?.redraw());
  wheel=createWheel({host:$('.wheel'),canvas:$('.wheel-ink'),items:navigation,reducedMotion:reduced,onSelect:index=>{if(!restoringRoute)void select(index);}});
  if(publicLanding){
    $('.agent-picker').hidden=true;$('#agent-label').hidden=true;
    $('.reload-records').hidden=true;$('.reload-records').disabled=true;
    $('[data-stat=repos]').textContent='—';$('[data-stat=stars]').textContent='—';$('[data-stat=followers]').textContent='PUBLIC';
    $('.snapshot > span').textContent='PUBLIC VIEWS';$('.snapshot-date').textContent='Read-only';
    $('.bar').setAttribute('aria-label','Public exploration status');
    $('.study-note').textContent='PUBLIC · Read-only exploration / no live activity or private records';
    data.connection='public';
  }else migrateLegacyRoute();
  restoreRoute();
  root.dataset.ready='true';
  if(!publicLanding){void reload(data);liveTimer=setInterval(()=>void refreshLive(),15000);}
  return true;
}
// Read-only polling pauses while hidden, typing or interacting. It never starts a cycle.
async function refreshLive(){
  if(publicLanding)return;
  if($('.buffer').contains(document.activeElement)&&/INPUT|TEXTAREA|SELECT/.test(document.activeElement?.tagName))return;
  if($('.buffer').querySelector('form[data-unsaved-changes=true]'))return;
  if(liveBusy||$('.buffer').querySelector('[data-economic-action-pending=true]')||document.hidden||document.body.dataset.view!=='projects'||$('.buffer').getAttribute('aria-busy')==='true'||!$('.command').hidden||!$('#agent-options').hidden||$('.projects').classList.contains('drawer-open'))return;
  liveBusy=true;
  try{
    const version=reloadVersion,next=await overview();
    if($('.buffer').querySelector('form[data-unsaved-changes=true]'))return;
    if(version!==reloadVersion||$('.buffer').querySelector('[data-economic-action-pending=true]')||document.hidden||document.body.dataset.view!=='projects')return;
    const changed=JSON.stringify([state.status,state.agents])!==JSON.stringify([next.status,next.agents]);
    state=next;
    if(changed){await reload();return;}
    await select(itemIndex,true);
  }finally{liveBusy=false;}
}
async function reload(currentScene=sceneData){
  if(publicLanding)return;
  const version=++reloadVersion;$('.reload-records').disabled=true;$('.reload-records').textContent='Retrieving…';
  try {
    const next=await overview();if(version!==reloadVersion)return;state=next;
    if(!state.agents.data?.some(agent=>agent.id===agentId))agentId=state.agents.data?.[0]?.id ?? '';
    rememberAgent();
    const selectEl=$('#agent-select');selectEl.replaceChildren(node('option','',state.agents.error?'Agent records unavailable':state.agents.data?.length?'Choose an agent':'No saved agents'));selectEl.firstChild.value='';
    for(const agent of state.agents.data ?? []){const option=node('option','',`${agent.name} / ${agent.lifecycle}`);option.value=agent.id;selectEl.append(option);}
    selectEl.value=agentId;syncAgentPicker();
    $('[data-stat=repos]').textContent=state.agents.data?.length ?? '—';
    $('[data-stat=stars]').textContent=state.status.data?.status ?? 'offline';
    $('[data-stat=followers]').textContent='SIM';
    $('.snapshot-date').textContent=state.retrievedAt.slice(11,19)+' UTC';$('.snapshot-date').title=state.retrievedAt;
    $('.bar').title=`Saved records retrieved ${state.retrievedAt}; not a blockchain observation timestamp.`;
    if(currentScene){currentScene.connection=state.status.data?.status ?? 'disconnected';currentScene.agentCount=state.agents.data?.length ?? null;currentScene.retrievedAt=state.retrievedAt;scenes.redraw();}
    $('.announcement').textContent=state.agents.error ?? 'Saved records retrieved.';
    if(document.body.dataset.view==='projects')void select(itemIndex);
  }finally{if(version===reloadVersion){$('.reload-records').disabled=false;$('.reload-records').textContent='Reload saved records';}}
}
$('.reload-records').addEventListener('click',()=>{closeCommand(false);$('.buffer').focus({preventScroll:true});void reload();});
$('#agent-select').addEventListener('change',event=>{agentId=event.target.value;syncAgentPicker();void select(itemIndex);});
function updateOfficialMetadata(path){
  const url=new URL(path,'https://supaagency.xyz').href;
  $('link[rel=canonical]').setAttribute('href',url);
  $('meta[property="og:url"]').setAttribute('content',url);
  for(const selector of ['meta[property="og:title"]','meta[name="twitter:title"]'])$(selector).setAttribute('content',document.title);
}
function setView(projects){
  closeAgentPicker();
  entryAttempt++;leaving=false;scenes?.cancelWarp();cancelTyping();renderVersion++;$('.buffer').setAttribute('aria-busy','false');
  if(!projects){clearArtworkMedia();document.title='Supa Agency — One agent. One purpose. One token.';updateOfficialMetadata('/');}
  document.body.dataset.view=projects?'projects':'intro';$('.intro').hidden=projects;$('.projects').hidden=!projects;closeCommand(false);setDrawer(false,false);scenes?.setMode(projects?'projects':'intro');
  if(projects){void select(itemIndex);requestAnimationFrame(()=>{window.dispatchEvent(new Event('resize'));wheel.reveal();$('.buffer').focus({preventScroll:true});});}
}
async function enter(event){event?.preventDefault();if(!await ready||leaving||document.body.dataset.view!=='intro')return;leaving=true;const attempt=++entryAttempt,completed=await scenes.warp();if(!completed||attempt!==entryAttempt)return;itemIndex=0;wheel.select(0);history.pushState({view:'projects'},'',`${publicLanding?'/home':'/'}${location.search}`);setView(true);}
$('.enter').addEventListener('click',enter);
$('.skip').addEventListener('click',event=>{
  event.preventDefault();
  if(location.hash!=='#workspace')history.pushState({view:document.body.dataset.view},'',`${location.pathname}${location.search}#workspace`);
  $('#workspace').focus();
});
$('.github').addEventListener('click',()=>window.alert(githubNotice));
$('.brand').addEventListener('click',async event=>{event.preventDefault();if(!await ready)return;history.pushState({view:'intro'},'',`/${location.search}`);setView(false);$('.enter').focus({preventScroll:true});});
window.addEventListener('popstate',restoreRoute);
function migrateLegacyRoute(){
  if(!['/site','/site/'].includes(location.pathname))return;
  let id;try{id=decodeURIComponent(location.hash.slice(1))||'home';}catch{id='not-found';}
  const item=navigation.find(entry=>entry.id===id);
  if(!item){location.replace(`/${encodeURIComponent(id)}${location.search}`);return;}
  history.replaceState({view:'projects'},'',`${item.path}${location.search}`);
}
function restoreRoute(){
  if(location.pathname==='/token-launch')history.replaceState({view:'projects'},'',`/factory${location.search}`);
  if(publicLanding&&location.pathname==='/'){setView(false);return;}
  const item=navigation.find(entry=>entry.path===location.pathname);
  if(!item)return;
  restoringRoute=true;
  itemIndex=navigation.indexOf(item);wheel.select(itemIndex);
  setView(publicLanding||item.id!=='home'||history.state?.view==='projects');
  restoringRoute=false;
}
function changeTheme(){theme=(theme+1)%palettes.length;applyTheme();try{localStorage.setItem('supa.dark-theme',String(theme));}catch{/* Optional preference. */}scenes.redraw();window.dispatchEvent(new Event('resize'));$('.announcement').textContent=`Dark theme ${theme+1} of ${palettes.length}`;}
$('.theme-key').addEventListener('click',changeTheme);
function setDrawer(open,focus=true){
  $('.projects').classList.toggle('drawer-open',open);$('.tabline').setAttribute('aria-expanded',String(open));$('.menu-indicator').textContent=open?'×':'☰';
  $('.tabline').setAttribute('aria-label',open?'Close navigation':'Open navigation');
  document.body.classList.toggle('menu-open',open);
  $('.pane').inert=open;$('.bar').inert=open;syncArtworkMedia();
  if(focus)(open?$('.wheel'):$('.tabline')).focus({preventScroll:true});
  requestAnimationFrame(()=>window.dispatchEvent(new Event('resize')));
}
$('.tabline').addEventListener('click',()=>setDrawer($('.tabline').getAttribute('aria-expanded')!=='true'));
$('.wheel').addEventListener('click',event=>{if(event.target.closest('.wheel-item')&&matchMedia('(max-width:700px)').matches){setDrawer(false,false);$('.buffer').focus({preventScroll:true});}});
function line(text,className='terminal-line'){const el=node('p',className,text);$('.buffer').append(el);return el;}
async function select(index,background=false){
  if(!publicLanding)rememberAgent();
  const item=navigation[index];if(!item)return;
  // Saved lifecycle is an identity attribute, never proof of a healthy worker.
  scenes?.setLifecycle(state.agents.error?null:state.agents.data?.find(agent=>agent.id===agentId)?.lifecycle);
  const version=background?renderVersion:++renderVersion;
  if(!background){itemIndex=index;cancelTyping();}
  if(!background&&document.body.dataset.view==='projects'){
    if(location.pathname!==item.path)history.pushState({view:'projects'},'',`${item.path}${location.search}`);
    document.title=`${item.label.split(' · ')[0]} / Supa Agency`;
    updateOfficialMetadata(item.path);
  }
  const buffer=$('.buffer');
  if(!background){
    $('.tab-file').textContent=item.label.split(' · ')[0];$('.winbar').textContent=`supa@${publicLanding?'public':'local'} — ${item.label} / read-only`;
    clearArtworkMedia();buffer.replaceChildren();buffer.setAttribute('aria-busy','true');buffer.dataset.typingState='loading';
    $('.blueprint').hidden=item.id!=='home';scenes?.setSelection(index);
    line(`supa@${publicLanding?'public':'local'} ~`,'prompt');line(publicLanding?'Opening public view…':'Loading saved view…','prompt');
  }
  const rows=publicLanding?publicSectionView(item.id):await sectionView(item.id,state,agentId);
  if(publicLanding&&item.id==='agents'&&document.body.dataset.agentAvatars==='true'){
    try{const response=await fetch('/api/public/agent-profiles',{credentials:'omit',cache:'no-store',redirect:'error',signal:AbortSignal.timeout(5000)});if(response.ok){const data=await response.json();if(Array.isArray(data.agents)&&data.agents.length&&data.agents.length<=100){const approved=data.agents.filter(a=>/^[0-9a-f-]{36}$/.test(a.agent_id)&&typeof a.name==='string'&&a.name.length<=80&&typeof a.summary==='string'&&a.summary.length<=280&&Number.isSafeInteger(a.revision));if(approved.length){rows.splice(2,rows.length-2,{kind:'heading',text:'Creator-approved public agents'},...approved.map(agent=>({kind:'public-agent',agent})),{kind:'notice',text:'Supa Agent remains our flagship personality. Only explicitly approved public profiles appear here.'});}}}}catch{/* An unavailable public projection must never fall back to private records. */}
  }
  if(version!==renderVersion||document.body.dataset.view!=='projects')return;
  $('.winbar').textContent=`supa@${publicLanding?'public':'local'} — ${item.label} / ${rows.some(row=>row.kind==='live'&&row.view.enabled)?'operator controls':rows.some(row=>row.kind==='economy')?'bounded economic controls':rows.some(row=>['autonomy','schedule','launch'].includes(row.kind))?'offline controls':'read-only'}`;
  const fingerprint=JSON.stringify(rows.filter(row=>!row.text?.startsWith('Retrieved  ')));
  if(background&&buffer.querySelector('form[data-unsaved-changes=true]'))return;
  if(background&&(fingerprint===viewFingerprint||buffer.contains(document.activeElement)||!$('.command').hidden||buffer.getAttribute('aria-busy')==='true'))return;
  viewFingerprint=fingerprint;if(background)cancelTyping();
  clearArtworkMedia();buffer.replaceChildren();line(`supa@${publicLanding?'public':'local'} ~`,'prompt');
  if(item.id==='about')buffer.append($('#terminal-architecture-template').content.firstElementChild.cloneNode(true));
  for(const row of rows){
    if(row.kind==='agent'){
      const button=node('button','terminal-entry',row.text);button.addEventListener('click',()=>{agentId=row.agent.id;$('#agent-select').value=agentId;syncAgentPicker();wheel.select(2);void select(2);});buffer.append(button);line(`Identity ${row.agent.id}\nCreated ${row.agent.created_at}`,'prompt');
    }else if(row.kind==='public-agent'){
      const figure=document.createElement('figure'),image=document.createElement('img'),link=document.createElement('a');image.src='/api/public/agent-avatars/'+row.agent.agent_id+'/'+row.agent.revision;image.width=96;image.height=96;image.alt='Avatar for '+row.agent.name;image.loading='lazy';link.href='/agents/'+row.agent.agent_id;link.textContent=row.agent.name;figure.append(image,link,node('p','terminal-line',row.agent.summary));buffer.append(figure);
    }else if(row.kind==='artwork'){
      if(!publicLanding||!Object.hasOwn(sectionArtwork,row.id))continue;
      const artwork=sectionArtwork[row.id],stem=`/public/assets/supa-${row.id}`;
      const figure=node('figure','section-illustration'),picture=node('picture'),source=node('source'),image=node('img','section-image'),caption=node('figcaption');
      figure.dataset.section=row.id;source.type='image/avif';source.srcset=`${stem}-960.avif 960w`;
      image.width=960;image.height=540;image.loading='lazy';image.decoding='async';image.alt=artwork.alt;
      image.sizes='(max-width:700px) calc(100vw - 56px), (max-width:1100px) 55vw, 604px';
      image.srcset=`${stem}-480.webp 480w, ${stem}-960.webp 960w`;image.src=`${stem}-480.webp`;
      picture.append(source,image);caption.append(node('span','section-art-title',artwork.title),node('span','','Concept artwork / not live activity'));
      const frame=node('div','section-art-media');frame.append(picture);
      figure.append(frame,caption);buffer.append(figure);
      animateArtwork(frame,row.id,`${stem}-960.webp`);
    }else if(row.kind==='autonomy')buffer.append(autonomyPanel(row.policy));
    else if(row.kind==='economy')buffer.append(economyPanel(row.view,row.agentId));
    else if(row.kind==='livework')buffer.append(liveWorkPanel(row.view,row.agentId));
    else if(row.kind==='live')buffer.append(livePanel(row.view,row.role,row.agentId));
    else if(row.kind==='launch')buffer.append(launchPanel(row.policy));
    else if(row.kind==='schedule')buffer.append(schedulePanel(row.status,row.goals,row.role));
    else if(row.kind==='link'){const p=line('');const a=node('a','',row.text);a.href=row.href;a.target='_blank';a.rel='noopener noreferrer';p.append(a);}
    else if(row.kind==='detail'){const details=node('details','calculation');details.append(node('summary','',row.text),node('p','',row.body));buffer.append(details);}
    else if(row.kind==='draft'){const block=node('figure','saved-draft');block.append(node('blockquote','',row.text),node('figcaption','prompt',row.meta));buffer.append(block);}
    else if(row.kind==='navigate'){const button=node('button','terminal-entry',row.text);button.addEventListener('click',()=>{const target=navigation.findIndex(item=>item.id===row.id);wheel.select(target);void select(target);});buffer.append(button);}
    else if(row.kind==='github'){const button=node('button','terminal-entry',row.text);button.type='button';button.addEventListener('click',()=>window.alert(githubNotice));buffer.append(button);}
    else if(row.kind==='disabled'){const button=node('button','terminal-entry',row.text);button.disabled=true;buffer.append(button);}
    else if(row.kind==='heading')buffer.append(node('h2','terminal-heading',row.text));
    else line(row.text,row.kind==='command'?'terminal-command':row.kind==='heading'?'terminal-heading':row.kind==='notice'?'terminal-notice':'terminal-line');
  }
  buffer.scrollTop=0;$('.pane').classList.remove('is-changing');void buffer.offsetWidth;$('.pane').classList.add('is-changing');
  $('.announcement').textContent=`Selected ${item.label}`;$('.key-feedback').textContent=': commands · ↑↓ navigate';typeTerminal(repeatSection(item.id));
}
// First visits type in document order; public revisits reveal complete rows in order.
function cancelTyping(){
  scenes?.stopTyping(true);
  typing++;clearTimeout(typingTimer);revealTyping=null;
  $('.skip-typing').hidden=true;
}
$('.skip-typing').addEventListener('click',()=>{revealTyping?.();closeCommand(false);});
function typeTerminal(repeat=false) {
  clearTimeout(typingTimer);
  const buffer=$('.buffer'),token=++typing,rows=[...buffer.children],skip=$('.skip-typing');
  const ghost=document.querySelectorAll('.entity-state b')[1];
  const originals=[];
  let rowIndex=0,textIndex=0,character=0,totalTyped=0;
  function finish(){
    if(token!==typing)return;
    clearTimeout(typingTimer);
    originals.forEach(({text,value})=>{text.textContent=value;});
    rows.forEach(row=>{row.hidden=false;row.inert=false;});
    buffer.setAttribute('aria-busy','false');buffer.setAttribute('aria-live','polite');buffer.dataset.typingState='complete';
    scenes?.stopTyping();
    if(ghost)ghost.textContent='idle';
    const restoreFocus=document.activeElement===skip;
    skip.hidden=true;revealTyping=null;
    if(restoreFocus)buffer.focus({preventScroll:true});
  }
  if(reduced.matches||(repeat&&!publicLanding)){finish();return;}
  rows.forEach(row=>{
    row.hidden=true;row.inert=true;
    if(repeat||row.classList.contains('terminal-architecture'))return;
    const walker=document.createTreeWalker(row,NodeFilter.SHOW_TEXT);
    let text;while((text=walker.nextNode())){originals.push({text,value:text.textContent,row});text.textContent='';}
  });
  buffer.setAttribute('aria-busy','true');buffer.setAttribute('aria-live','off');buffer.dataset.typingState=repeat?'revealing':'typing';
  if(ghost)ghost.textContent='typing';
  skip.hidden=false;revealTyping=finish;
  function tick(){
    if(token!==typing)return;
    if(reduced.matches){finish();return;}
    const row=rows[rowIndex];if(!row){finish();return;}
    const follow=buffer.scrollHeight-buffer.scrollTop-buffer.clientHeight<32;
    row.hidden=false;
    if(repeat){
      row.inert=false;rowIndex++;
      // Bound a public revisit to about 600 ms, including long Factory pages.
      typingTimer=setTimeout(tick,Math.min(60,600/Math.max(1,rows.length)));return;
    }
    const record=originals[textIndex];
    if(!record||record.row!==row){row.inert=false;rowIndex++;typingTimer=setTimeout(tick,120);return;}
    if(character<record.value.length){
      character++;record.text.textContent=record.value.slice(0,character);
      if(follow)buffer.scrollTop=buffer.scrollHeight;
      scenes?.onTyping(record.text,character,record.value[character-1],++totalTyped);
      typingTimer=setTimeout(tick,row.classList.contains('terminal-command')?26:16);
    }else{character=0;textIndex++;typingTimer=setTimeout(tick,8);}
  }
  typingTimer=setTimeout(tick,repeat?0:150);
}
function openCommand(prefix=''){closeAgentPicker(); $('.command').hidden=false;$('.command-input').value=prefix;$('.command-input').focus();historyIndex=commandHistory.length; }
function economyPanel(view,id){
  const details=node('details','calculation autonomy-panel');details.dataset.economy='jobs';details.append(node('summary','','Services, jobs and earned revenue'));
  details.append(node('p','terminal-notice','Operator-controlled offline work. Simulated units and Devnet test USDC stay separate. No real spending or transfers. Reload records after an uncertain response; never retry automatically.'));
  details.append(node('p','prompt',view.accounts.policy?`Configured simulated reserve: ${units(view.accounts.policy.reserve_atoms)}. Cumulative spending ceiling: ${units(view.accounts.policy.ceiling_atoms)}.`:'No simulated spending policy is approved; cost-bearing acceptance is disabled.'));
  for(const account of view.accounts.networks){
    details.append(node('h3','terminal-heading',account.network==='simulated'?'Simulated contracts':'Devnet test contracts'));
    for(const [key,label]of [['cash_atoms','Cash'],['liability_atoms','Customer liabilities'],['earned_atoms','Earned revenue'],['inference_atoms','Inference expense'],['infrastructure_atoms','Infrastructure expense'],['available_atoms','Available operating capital'],['net_result_atoms','Net result']])details.append(node('p','prompt',`${label}: ${units(account[key])}`));
    details.append(node('p','terminal-notice',account.runway_reason));
  }
  const status=node('p','terminal-notice');status.setAttribute('role','status');status.setAttribute('aria-live','polite');
  async function send(path,body,button){
    details.dataset.economicActionPending='true';button.disabled=true;status.textContent='Saving explicit economic action…';
    try{const response=await fetch(path,{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/json','X-Supa-Action':'economic-workflow'},body:JSON.stringify(body),signal:AbortSignal.timeout(20000)});const result=await response.json();if(!response.ok){status.textContent=`Action not confirmed: ${result.error??'backend unavailable'}. Reload saved records before another action.`;return null;}status.textContent=`Saved job ${result.id} / ${result.state}. Reload Economy to inspect current records.`;return result;}catch{status.textContent='Outcome uncertain. Reload saved records before another action. No automatic retry.';return null;}
  }
  const refresh=node('button','','Reload economic records');refresh.type='button';refresh.addEventListener('click',()=>void select(navigation.findIndex(n=>n.id==='economy')));details.append(refresh);
  const services=view.services;
  if(!services.length)details.append(node('p','terminal-notice','No services are registered for this agent. An owner must explicitly offer a bounded service.'));
  else{
    const form=node('form','autonomy-form'),selectService=node('select'),label=node('label','','Service');selectService.id='economic-service';label.htmlFor=selectService.id;
    for(const service of services){const option=node('option','',`${service.spec.name} / ${units(service.spec.price_atoms)} per job`);option.value=service.id;selectService.append(option);}
    const network=node('select');network.id='economic-network';const networkLabel=node('label','','Job network');networkLabel.htmlFor=network.id;for(const [value,text]of [['simulated','Simulated contract'],['devnet','Devnet test tokens / registered wallet required']]){const option=node('option','',text);option.value=value;network.append(option);}
    const input=node('textarea');input.id='economic-input';input.rows=3;input.maxLength=8000;input.required=true;const inputLabel=node('label','','Supplied input / no secrets');inputLabel.htmlFor=input.id;
    const consent=node('input');consent.type='checkbox';consent.required=true;const consentLabel=node('label','autonomy-consent');consentLabel.append(consent,document.createTextNode('Create one contract on the selected test network under my authenticated identity. No real funds or paid inference.'));
    const submit=node('button','','Create simulated job');submit.type='submit';network.addEventListener('change',()=>{submit.textContent=network.value==='devnet'?'Create Devnet test job':'Create simulated job';});form.append(label,selectService,networkLabel,network,inputLabel,input,node('p','terminal-notice','Optional structured input: {"objective":"...","material":"...","requirements":["literal evidence phrase"]}. Text matching does not verify truth. Independent retrieval is unavailable.'),consentLabel,submit);details.append(form);
    form.addEventListener('submit',async event=>{event.preventDefault();event.stopPropagation();if(!form.reportValidity()||!consent.checked)return;await send('/api/jobs',{request_id:crypto.randomUUID(),service_id:selectService.value,network:network.value,input:input.value,valid_for_seconds:3600},submit);});
  }
  for(const job of view.jobs){
    const block=node('details','calculation');block.dataset.jobId=job.id;block.append(node('summary','',`${job.spec.name} / ${job.state} / ${job.network}`),node('p','prompt',`Job ${job.id} / revision ${job.revision} / attempts ${job.attempts}`),node('p','prompt',`Payment ${units(job.received_atoms)} / required ${units(job.spec.price_atoms)}. Expires ${job.expires_at}.`));
    if(job.network==='devnet')block.append(node('p','terminal-notice',`Devnet payment requires the registered public wallet, canonical Devnet USDC mint and exactly one memo: ${job.id}. No transaction is constructed or signed here.`));
    const refundable=['completed','failed','expired','cancelled','disputed'].includes(job.state)?job.received_atoms-job.earned_atoms:0;block.append(node('p','prompt',`Refund eligibility: ${units(refundable)}. Refund transfer is disabled.`));
    if(job.output?.work){
      const work=job.output.work;block.append(node('h3','terminal-heading',`Bounded offline work / ${work.status}`));
      block.append(node('p','prompt',work.objective),node('p','terminal-notice',`${work.provider_requests} deterministic provider request(s) / ${work.revision_count} plan revision(s). No internet retrieval or paid inference.`));
      for(const step of work.steps??[])block.append(node('p','prompt',`${step.tool.replaceAll('_',' ')} / ${step.state}`));
      for(const evidence of work.evidence??[])block.append(node('p','prompt',`Supplied material:${evidence.line} / unverified statement\n${evidence.quote}`));
      for(const item of work.requirements??[])block.append(node('p','terminal-notice',`${item.supported_by_literal_match?'Text match':'Missing evidence'} / ${item.requirement}`));
      if(work.abstention_reason)block.append(node('p','terminal-notice',work.abstention_reason));
      if(work.memory)block.append(node('p','terminal-notice',`Memory: ${work.memory.verified_references_used} verified reference(s) used; ${work.memory.hypotheses_excluded} hypotheses excluded. Private contents withheld.`));
      if(job.assessment?.private_memory_references?.length)block.append(node('p','terminal-notice',`Operator-only memory references: ${job.assessment.private_memory_references.map(reference=>reference.memory_id).join(', ')}. Inspect Memory for source events.`));
      if(work.economics)block.append(node('p','prompt',`Economic recommendation: ${work.economics.recommendation} / offered ${units(work.economics.offered_revenue_atoms)} / estimated cost ${units(work.economics.estimated_cost_atoms)} (${job.network}). Recommendation grants no permission.`));
      const report=node('details','calculation');report.append(node('summary','','Findings and evidence / full saved report'));const body=node('pre','',JSON.stringify(job.output,null,2));body.style.whiteSpace='pre-wrap';body.style.overflowWrap='anywhere';report.append(body);block.append(report,node('p','prompt',`Output SHA-256 ${job.output_sha256}`));
    }
    else if(job.output){const pre=node('pre','',JSON.stringify(job.output,null,2));pre.style.whiteSpace='pre-wrap';pre.style.overflowWrap='anywhere';block.append(pre,node('p','prompt',`Output SHA-256 ${job.output_sha256}`));}
    if(job.failure_code)block.append(node('p','terminal-notice',job.failure_code));
    const consent=node('input');consent.type='checkbox';const label=node('label','autonomy-consent');label.append(consent,document.createTextNode('I explicitly authorize the selected bounded action; acceptance reserves simulated costs.'));block.append(label);
    const ops={created:['request_payment','cancel'],awaiting_payment:['cancel','expire'],funded:['accept','cancel','dispute'],accepted:['execute','cancel'],executing:['recover'],failed:['retry','dispute'],completed:['dispute']}[job.state]??[];
    for(const op of ops){const button=node('button','',op.replaceAll('_',' '));button.type='button';button.addEventListener('click',async()=>{if(!consent.checked){status.textContent='Explicit authorization is required.';return;}await send(`/api/jobs/${job.id}/commands`,{request_id:crypto.randomUUID(),expected_revision:job.revision,operation:op,authorize:true},button);});block.append(button);}
    if(job.network==='simulated'&&job.state==='awaiting_payment'){const pay=node('button','','Record simulated payment');pay.type='button';pay.addEventListener('click',async()=>{if(!consent.checked){status.textContent='Explicit authorization is required.';return;}await send(`/api/jobs/${job.id}/simulated-payment`,{request_id:crypto.randomUUID(),amount_atoms:Math.max(1,job.spec.price_atoms-job.received_atoms),authorize_simulated:true},pay);});block.append(pay);}
    if(job.network==='devnet'){
      const instructions=node('p','prompt','Expand to load read-only payment instructions.');block.append(instructions);let loaded=false;
      block.addEventListener('toggle',async()=>{if(!block.open||loaded)return;loaded=true;const result=await read(`/api/jobs/${job.id}`);const p=result.data?.payment_request;instructions.textContent=p?`Devnet test USDC / mint ${p.mint}. Recipient ${p.recipient??'unavailable'}. Amount ${units(p.amount_atoms)}; memo ${p.memo}. Expires ${p.expires_at}.`:(result.error??'Payment instructions unavailable.');});
      const signature=node('input');signature.id=`signature-${job.id}`;signature.maxLength=90;const signatureLabel=node('label','','Finalized Devnet transaction signature');signatureLabel.htmlFor=signature.id;const verify=node('button','','Verify read-only Devnet receipt');verify.type='button';verify.addEventListener('click',async()=>{if(!consent.checked||!signature.value){status.textContent='Authorization and a finalized signature are required.';return;}await send(`/api/jobs/${job.id}/verify-payment`,{signature:signature.value,authorize_read_only_verification:true},verify);});block.append(signatureLabel,signature,verify);
    }
    details.append(block);
  }
  if(!view.jobs.length)details.append(node('p','terminal-notice','No customer jobs have been saved.'));
  details.append(status);return details;
}
function autonomyPanel(policy){
  const details=node('details','calculation autonomy-panel'),summary=node('summary','','Run an offline autonomy workflow');details.append(summary);
  details.append(node('p','terminal-notice','A new test agent receives your goal, selects permitted tools for three deterministic cycles, then pauses. No GPT calls, external actions or real spending.'));
  details.append(node('p','prompt','Offline progress estimates are execution heuristics, not proof that the goal was completed or a measure of intelligence.'));
  details.append(node('p','prompt',`Tools: ${policy.tools.join(', ')}. Simulated seed: ${policy.simulated_seed_microusd/1000000} USD; maximum simulated expenses: ${policy.simulated_expense_limit_microusd/1000000} USD.`));
  const form=node('form','autonomy-form'),label=node('label','','Goal / operator-only'),goal=node('textarea');goal.id='autonomy-goal';goal.name='goal';goal.rows=3;goal.maxLength=500;goal.minLength=3;goal.required=true;label.htmlFor=goal.id;
  const consentLabel=node('label','autonomy-consent'),consent=node('input');consent.type='checkbox';consent.required=true;consent.name='authorize_offline';consentLabel.append(consent,document.createTextNode('I authorize a new test identity, three offline cycles and up to 0.0033 simulated USD of expenses. No real funds.'));
  const run=node('button','','Run authorized offline workflow');run.type='submit';form.append(label,goal,consentLabel,run);
  const status=node('p','terminal-notice');status.setAttribute('role','status');status.setAttribute('aria-live','polite');
  const inspect=node('button','','Inspect saved run'),reset=node('button','','Prepare another workflow');inspect.type=reset.type='button';
  function sync(){
    form.hidden=!!autonomyAttempt;inspect.hidden=!autonomyAttempt;reset.hidden=!autonomyAttempt||autonomyAttempt.status==='pending';inspect.disabled=autonomyAttempt?.status==='pending';
    status.textContent=!autonomyAttempt?'Authorization is required for each run.':autonomyAttempt.status==='pending'?`Running offline workflow ${autonomyAttempt.id}…`:autonomyAttempt.status==='completed'?`Saved ${autonomyAttempt.cycles} completed offline cycles. Agent paused. Request ${autonomyAttempt.id}.`:`Outcome not confirmed here. Inspect saved run ${autonomyAttempt.id} before starting another workflow. No automatic retry.`;
  }
  form.addEventListener('submit',async event=>{
    event.preventDefault();event.stopPropagation();if(autonomyAttempt||!form.reportValidity()||!consent.checked)return;
    const attempt=autonomyAttempt={id:crypto.randomUUID(),status:'pending'};try{sessionStorage.setItem('supa.offline-request',attempt.id);}catch{/* Durable server identity still prevents duplicate execution. */}sync();
    try{
      const response=await fetch('/api/autonomy',{method:'POST',credentials:'same-origin',cache:'no-store',headers:{'Content-Type':'application/json','X-Supa-Action':'offline-autonomy'},body:JSON.stringify({request_id:attempt.id,goal:goal.value.trim(),authorize_offline:true}),signal:AbortSignal.timeout(35000)});
      const result=await response.json();
      if(!response.ok||result.agent_id!==attempt.id||result.mode!=='offline'||result.lifecycle!=='paused'||result.paid_requests!==0||!Array.isArray(result.cycles)||result.cycles.length!==3)throw Error();
      attempt.status='completed';attempt.cycles=result.cycles.length;
    }catch{attempt.status='unknown';}
    sync();$('.announcement').textContent=attempt.status==='completed'?'Offline workflow saved; agent paused.':'Workflow outcome uncertain. Inspect saved records; do not retry automatically.';
  });
  inspect.addEventListener('click',async()=>{
    const id=autonomyAttempt?.id;if(!id)return;await reload();
    if(!state.agents.data?.some(agent=>agent.id===id)){status.textContent='No saved identity was returned for this request. Check backend activity before preparing another workflow.';return;}
    agentId=id;$('#agent-select').value=id;syncAgentPicker();wheel.select(2);void select(2);
  });
  reset.addEventListener('click',()=>{if(autonomyAttempt?.status==='pending')return;autonomyAttempt=null;goal.value='';consent.checked=false;try{sessionStorage.removeItem('supa.offline-request');}catch{}sync();goal.focus();});
  details.append(form,status,inspect,reset);sync();return details;
}
function livePanel(view,role,id){
  const providerName=view.policy?.model?.startsWith('claude-')?'Claude':'GPT',scopeName=providerName==='Claude'?'Anthropic workspace':'OpenAI project';
  const panel=node('details','calculation autonomy-panel live-panel');panel.append(node('summary','',`Controlled ${providerName} session`));
  panel.append(node('p','prompt','One explicitly authorized request. Provider charges are separate from simulated expenses. No continuous live scheduling. The model proposes; tools require a second approval.'));
  const feedback=node('p','terminal-notice');feedback.setAttribute('role','status');feedback.setAttribute('aria-live','polite');
  const reload=node('button','',`Reload saved ${providerName} sessions`);reload.type='button';reload.addEventListener('click',()=>void select(itemIndex));
  async function send(path,body,button){button.disabled=true;feedback.textContent='Saving request. Do not resubmit while its outcome is unknown.';
    try{const response=await fetch(path,{method:'POST',credentials:'same-origin',cache:'no-store',headers:{'Content-Type':'application/json','X-Supa-Action':'controlled-inference'},body:JSON.stringify(body),signal:AbortSignal.timeout(65000)});const result=await response.json();if(!response.ok||result.agent_id!==id)throw Error();feedback.textContent=`Saved session ${result.id} / ${result.status}. Reload saved ${providerName} sessions to inspect the result.`;}
    catch{feedback.textContent=`Rejected or uncertain outcome. Reload saved ${providerName} sessions and inspect accounting before another request. No automatic retry.`;}
  }
  if(view.enabled&&view.policy){const p=view.policy;panel.append(node('p','terminal-notice',`Configured model / ${p.model}\nMaximum requests / 1\nInput token ceiling / ${p.input_token_bound}\nOutput token ceiling / ${p.max_output_tokens}\nCumulative budget / ${p.budget_limit_microusd} microUSD\nBudget reference / ${p.budget_id}\nMaximum reservation / ${view.minimum_reservation_microusd} microUSD`));
    if(role==='owner'){
      const form=node('form','autonomy-form schedule-form live-form'),goalLabel=node('label','','Goal for this session'),goal=node('textarea');goal.name='live-goal';goal.required=true;goal.maxLength=500;goal.rows=3;goalLabel.append(goal);
      const costLabel=node('label','','Session ceiling / real API microUSD'),cost=node('input');cost.type='number';cost.name='live-ceiling';cost.min=String(view.minimum_reservation_microusd??1);cost.max=String(Math.min(100000,p.budget_limit_microusd));cost.required=true;cost.step='1';costLabel.append(cost);
      const modelLabel=node('label','','Approved model'),model=node('select'),option=node('option','',p.model);option.value=p.model;model.append(option);model.name='live-model';modelLabel.append(model);
      const consentLabel=node('label','autonomy-consent'),consent=node('input');consent.type='checkbox';consent.required=true;consent.name='authorize-paid';consentLabel.append(consent,document.createTextNode(`I authorize one paid ${providerName} request for this goal and these limits, valid for five minutes. Relevant private memories from this agent will be sent to the configured ${scopeName}. No retry, financial action, or automatic tool execution.`));
      const submit=node('button','',`Authorize one paid ${providerName} request`);submit.type='submit';form.append(goalLabel,modelLabel,costLabel,consentLabel,submit);
      for(const input of [goal,cost,model])input.addEventListener('input',()=>{consent.checked=false;});
      form.addEventListener('submit',event=>{event.preventDefault();event.stopPropagation();if(submit.disabled||!form.reportValidity()||!consent.checked)return;const now=new Date();const request={request_id:crypto.randomUUID(),goal:goal.value.trim(),model:model.value,authorize_paid:true,consented_at:now.toISOString(),expires_at:new Date(now.getTime()+300000).toISOString(),max_requests:1,max_input_tokens:p.input_token_bound,max_output_tokens:p.max_output_tokens,max_cost_microusd:Number(cost.value)};for(const input of [goal,cost,model,consent])input.disabled=true;void send(`/api/agents/${id}/live`,request,submit);});panel.append(form);
    }else panel.append(node('p','prompt','Only the agent owner can authorize paid inference or approve its proposed tool.'));
  }else panel.append(node('p','terminal-notice','Paid inference is disabled on this server. Saved sessions remain readable.'));
  for(const session of view.sessions){const entry=node('details','calculation');entry.append(node('summary','',`${session.policy.model} / ${session.status} / ${session.created_at}`));
    entry.append(node('p','',`Session ${session.id}\nApproved by ${session.approved_by}\nGoal / ${session.consent.goal}\nMetadata requests / ${session.metadata_requests}\nGeneration attempts / ${session.generation_requests}\n${session.policy.model.startsWith('claude-')?'Workspace':'Project'}-scoped metadata access / ${session.project_verified?'verified before request':'not verified'}\nRetrieved memories / ${session.memory_ids.join(', ')||'none'}\nAccounted estimate / ${session.accounted_microusd} microUSD; reserved ${session.reserved_microusd}\nUsage / ${session.usage?`${session.usage.input_tokens} input + ${session.usage.output_tokens} output tokens`:'unavailable; reservation retained if uncertain'}${session.failure_code?'\nFailure / '+session.failure_code:''}`));
    if(session.proposal)entry.append(node('p','',`Model decision summary / ${session.proposal.summary}\nCited memories / ${session.proposal.memory_ids.join(', ')||'none'}\nProposed actions / ${session.proposal.actions.map(a=>a.tool).join(', ')||'abstain'}`));
    if(session.tool_result)entry.append(node('p','',`Tool outcome / ${session.tool_result.result}; ${session.tool_result.executed_actions} actions`));
    if(view.enabled&&role==='owner'&&session.status==='awaiting_approval')for(const [approve,label]of [[true,'Approve proposed memory read'],[false,'Decline proposed actions']]){const button=node('button','',label);button.type='button';button.addEventListener('click',()=>{entry.querySelectorAll('button').forEach(b=>b.disabled=true);void send(`/api/agents/${id}/live/${session.id}/approval`,{approve,confirm_review:true},button);});entry.append(button);}
    panel.append(entry);
  }
  panel.append(feedback,reload);return panel;
}
function workflowFailure(status){return ({401:'Sign-in expired or unavailable. Open Operator sign-in; execution limits are not renewed.',403:'Permission denied. Check your authorized role and inspect saved records.',400:'Request rejected. Check the fields and finite limits.',422:'Request rejected. Check the fields and finite limits.',409:'Saved state changed or this request already exists. Reload saved records; do not replay it.'})[status]??'Outcome uncertain. Inspect saved records before another authorization.';}
function launchPanel(policy){
  const panel=node('details','calculation autonomy-panel launch-panel');panel.open=true;panel.append(node('summary','','Create a test agent'));
  panel.append(node('p','terminal-notice','Disposable development / offline reasoning only. Simulated budget; no payment, paid model or external action. Creating an identity does not start it.'));
  const form=node('form','autonomy-form'),fields={};
  for(const [key,label,value]of [['name','Agent name',''],['personality','Personality / voice','Curious, witty and careful. Admit uncertainty and use only verified evidence.'],['goal','Goal','']]){
    const wrap=node('label','',label),input=node(key==='name'?'input':'textarea');input.name=key;input.id='launch-'+key;input.required=true;input.minLength=3;input.maxLength=key==='name'?80:500;input.value=value;if(key!=='name')input.rows=3;wrap.append(input);form.append(wrap);fields[key]=input;
  }
  const budgetLabel=node('label','','Simulated operating budget / microUSD'),budget=node('input');budget.name='simulated_budget_microusd';budget.type='number';budget.min='3300';budget.max='1000000';budget.step='1';budget.value='10000';budget.required=true;budgetLabel.append(budget);form.append(budgetLabel);const budgetHint=node('p','prompt');const showBudget=()=>{budgetHint.textContent=`${units(Number(budget.value))} simulated USD. This is the seed balance; the session expense ceiling is authorized separately.`;};budget.addEventListener('input',showBudget);showBudget();form.append(budgetHint);
  const label=node('label','autonomy-consent'),consent=node('input');consent.type='checkbox';consent.name='launch-consent';consent.required=true;label.append(consent,document.createTextNode('Create this test identity, save this goal and assign only the simulated seed above. I will separately authorize a finite offline session.'));
  const submit=node('button','','Create authorized test agent');submit.type='submit';const feedback=node('p','terminal-notice');feedback.setAttribute('role','status');
  form.append(label,submit);panel.append(form,feedback);form.addEventListener('input',event=>{if(event.target!==consent)consent.checked=false;});
  form.addEventListener('submit',async event=>{
    event.preventDefault();event.stopPropagation();if(submit.disabled||!form.reportValidity()||!consent.checked)return;
    const input={request_id:crypto.randomUUID(),name:fields.name.value.trim(),personality:fields.personality.value.trim(),goal:fields.goal.value.trim(),simulated_budget_microusd:Number(budget.value),authorize_offline:true};
    form.dataset.economicActionPending='true';for(const element of form.elements)element.disabled=true;feedback.textContent='Saving the inactive identity, goal and simulated budget…';
    try{const response=await fetch('/api/launch',{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/json','X-Supa-Action':'offline-launch'},body:JSON.stringify(input),signal:AbortSignal.timeout(30000)}),result=await response.json();
      if(!response.ok||result.paid_requests!==0||result.mode!=='offline')throw Object.assign(Error(),{result,status:response.status});
      agentId=result.agent_id;await reload();wheel.select(2);await select(2);$('.announcement').textContent='Test agent saved, inactive. Open Agent lifecycle and bounded scheduling to authorize its first session.';
    }catch(error){feedback.textContent=`${workflowFailure(error.status)} ${error.result?.agent_id?'Inspect saved agent '+error.result.agent_id+'. ':''}Reload saved records before another request. No automatic retry. Request ${input.request_id}.`;}
    finally{form.dataset.economicActionPending='false';}
  });return panel;
}
function schedulePanel(view,goals,role){
  const details=node('details','calculation autonomy-panel schedule-panel');details.append(node('summary','','Agent lifecycle and bounded scheduling'));
  const stateText=node('p','terminal-notice');stateText.dataset.scheduleState='';
  stateText.textContent=`${view.lifecycle} / ${view.enabled?'authorized schedule':'scheduling disabled'} / revision ${view.revision}\nCycles started ${view.cycles_started}${view.policy?' / '+view.policy.max_cycles:''}\nConservative allowance used ${view.reserved_microusd} simulated microUSD${view.policy?' / '+view.policy.simulated_budget_microusd:''}\nNext eligible ${view.next_cycle_at}\nAuthorization expires ${view.expires_at??'not authorized'}${view.last_error?'\nLast outcome / '+view.last_error:''}`;
  details.append(stateText,node('p','terminal-notice',({created:'Created / inactive. Select a goal and authorize finite limits to start.',active:'Active authorization. Saved cycles and lease observations show actual work.',paused:'Paused. Resume preserves the same cycle, budget and expiry limits.',suspended:'Suspended. Inspect the last outcome. Exhausted or expired limits need explicit renewal.',terminated:'Stopped permanently. Saved history remains readable.'})[view.lifecycle]),node('p','prompt','Offline only. Actual expenses appear in Economy; allowance consumption is a conservative limit, not another charge. An enabled offline scheduler service is also required. Status polls every 15 seconds when idle.'));
  const form=node('form','autonomy-form schedule-form'),operation=node('select'),operationLabel=node('label','','Lifecycle operation');operation.id='schedule-operation';operationLabel.htmlFor=operation.id;
  const choices=[];if(role==='owner'&&!view.policy&&['created','paused','suspended'].includes(view.lifecycle))choices.push(['start','Start scheduling']);
  if(role==='owner'&&view.policy&&['paused','suspended'].includes(view.lifecycle))choices.push(['renew','Renew with new finite limits']);
  if(role!=='viewer'&&view.policy&&['paused','suspended'].includes(view.lifecycle))choices.push(['resume','Resume scheduling']);
  if(role!=='viewer'&&view.lifecycle==='active')choices.push(['pause','Pause agent']);if(role!=='viewer'&&view.lifecycle!=='terminated')choices.push(['stop','Stop permanently']);
  form.hidden=!choices.length;if(!choices.length)details.append(node('p','terminal-notice',view.lifecycle==='terminated'?'This identity is permanently terminated. It cannot start or resume. Saved history remains available.':'Your role permits reading saved records only.'));
  for(const [value,label]of choices){const option=node('option','',label);option.value=value;operation.append(option);}
  const setup=node('fieldset','schedule-policy'),legend=node('legend','','New scheduling authorization');setup.append(legend);
  const selected=[];for(const goal of goals.filter(goal=>!goal.completed)){const label=node('label','autonomy-consent'),check=node('input');check.type='checkbox';check.value=goal.id;check.name='approved-goal';label.append(check,document.createTextNode(goal.description));setup.append(label);selected.push(check);}
  if(!selected.length)setup.append(node('p','prompt','No unfinished goals are available. Scheduling cannot start.'));
  const fields={};for(const [key,label,value,min,max]of [['interval_seconds','Cycle interval / seconds',15,5,86400],['max_cycles','Maximum cycles',3,1,100],['simulated_budget_microusd','Simulated expense ceiling / microUSD',3300,1100,110000],['valid_for_seconds','Authorization duration / seconds',3600,60,86400]]){
    const wrap=node('label','',label),input=node('input');input.type='number';input.name=key;input.value=String(value);input.min=String(min);input.max=String(max);input.step='1';input.required=true;wrap.append(input);setup.append(wrap);fields[key]=input;
  }
  const reasonLabel=node('label','','Operator reason'),reason=node('textarea');reason.name='reason';reason.rows=2;reason.maxLength=500;reason.required=true;reasonLabel.append(reason);
  const consentLabel=node('label','autonomy-consent'),consent=node('input');consent.type='checkbox';consent.required=true;consent.name='schedule-authorization';consentLabel.append(consent,document.createTextNode('I authorize this operation on the selected agent. Start or explicit renewal uses only its existing memory/goal tools, 1,100 simulated microUSD per cycle and the limits above. Resume never resets limits. Stop permanently terminates this identity. No paid inference or financial transactions.'));
  const submit=node('button','','Apply authorized lifecycle operation');submit.type='submit';submit.disabled=!choices.length;
  const feedback=node('p','terminal-notice');feedback.setAttribute('role','status');feedback.setAttribute('aria-live','polite');
  const refresh=node('button','','Reload saved schedule');refresh.type='button';refresh.addEventListener('click',()=>void select(itemIndex));
  function updateSetup(){setup.disabled=!['start','renew'].includes(operation.value);setup.hidden=setup.disabled;consent.checked=false;}
  setup.addEventListener('input',()=>{consent.checked=false;});reason.addEventListener('input',()=>{consent.checked=false;});
  operation.addEventListener('change',()=>{selected.forEach(input=>input.checked=false);updateSetup();});form.append(operationLabel,operation,setup,reasonLabel,consentLabel,submit);details.append(form,feedback,refresh);updateSetup();
  form.addEventListener('submit',async event=>{
    event.preventDefault();event.stopPropagation();if(submit.disabled||!form.reportValidity()||!consent.checked)return;
    const policy=['start','renew'].includes(operation.value)?{goal_ids:selected.filter(input=>input.checked).map(input=>input.value),...Object.fromEntries(Object.entries(fields).map(([key,input])=>[key,Number(input.value)]))}:null;
    if(policy&&!policy.goal_ids.length){feedback.textContent='Select at least one unfinished goal to authorize.';return;}
    const command={request_id:crypto.randomUUID(),expected_revision:view.revision,operation:operation.value,authorize_offline:true,reason:reason.value.trim(),policy,...(operation.value==='renew'?{consented_at:new Date().toISOString()}:{})};
    submit.disabled=true;operation.disabled=true;setup.disabled=true;reason.disabled=true;consent.disabled=true;refresh.disabled=true;feedback.textContent='Saving authorized operation…';
    try{
      const response=await fetch(`/api/agents/${view.agent_id}/schedule`,{method:'POST',credentials:'same-origin',cache:'no-store',headers:{'Content-Type':'application/json','X-Supa-Action':'offline-autonomy'},body:JSON.stringify(command),signal:AbortSignal.timeout(35000)});
      const result=await response.json();if(!response.ok||result.agent_id!==view.agent_id||result.mode!=='offline'||result.paid_requests_enabled!==false)throw Object.assign(Error(),{status:response.status});
      delete form.dataset.unsavedChanges;
      feedback.textContent=`Saved ${command.operation}. Agent ${result.lifecycle}; ${result.cycles_started} cycles started. Reload saved schedule for current records.`;
      stateText.textContent=`${result.lifecycle} / ${result.enabled?'authorized schedule':'scheduling disabled'} / revision ${result.revision}\nCycles started ${result.cycles_started}\nAllowance used ${result.reserved_microusd} simulated microUSD`;
    }catch(error){feedback.textContent=`${workflowFailure(error.status)} Reload saved schedule before another authorization. No automatic retry. Request ${command.request_id}.`;}
    refresh.disabled=false;$('.announcement').textContent=feedback.textContent;
  });
  return details;
}
function closeCommand(focus=true){$('.command').hidden=true;if(focus)returnFocus();}
function returnFocus(){if(document.body.dataset.view==='intro'){$('.enter').focus({preventScroll:true});return;}if(matchMedia('(max-width:700px)').matches){setDrawer(false,false);$('.tabline').focus({preventScroll:true});}else $('.wheel').focus({preventScroll:true});}
$('.command-toggle').addEventListener('click',()=>openCommand('supa '));
$('.command-close').addEventListener('click',()=>closeCommand());
$('.command-input').addEventListener('keydown',event=>{if(['ArrowUp','ArrowDown'].includes(event.key)){event.preventDefault();historyIndex=Math.max(0,Math.min(commandHistory.length,historyIndex+(event.key==='ArrowUp'?-1:1)));event.target.value=commandHistory[historyIndex] ?? '';}});
$('.command').addEventListener('submit',event=>{
  event.preventDefault();const value=$('.command-input').value.trim().replace(/^:/,'');if(value){commandHistory.push(value);commandHistory=commandHistory.slice(-50);}closeCommand();
  if(value==='theme'){changeTheme();return;}
  if(['q','quit'].includes(value)){history.pushState({view:'intro'},'',`/${location.search}`);setView(false);$('.enter').focus();return;}
  const command=value.replace(/^supa\s+/,'');
  if(command==='help'){line(publicLanding?'Public UI commands: supa status, supa agents, supa capabilities. ↑↓ choose views, j/k scroll, Escape returns navigation. Private records require sign-in.':'UI commands only: supa status, supa agents, supa inspect <name or UUID>, supa intelligence, supa memory, supa economy, supa social, supa capabilities. ↑↓ choose views, j/k scroll, : opens command, Escape returns navigation. No shell execution.','terminal-notice');$('.announcement').textContent='Command help added to terminal.';return;}
  if(command.startsWith('inspect ')){
    const query=command.slice(8).toLowerCase();const agent=state.agents.data?.find(a=>a.id===query||a.name.toLowerCase()===query);
    if(!agent){const message='No matching saved agent. Use supa agents.';$('.announcement').textContent=message;line(message,'terminal-notice');$('.buffer').scrollTop=$('.buffer').scrollHeight;return;}
    agentId=agent.id;$('#agent-select').value=agentId;syncAgentPicker();wheel.select(2);void select(2);return;
  }
  const id=({status:'home',capabilities:'about'})[command]??command,index=navigation.findIndex(item=>item.id===id);
  if(index>=0){
    restoringRoute=true;wheel.select(index);restoringRoute=false;
    if(document.body.dataset.view==='intro'){itemIndex=index;history.pushState({view:'projects'},'',`${navigation[index].path}${location.search}`);setView(true);}
    else void select(index);
  }
  else{$('.announcement').textContent='Unknown command. Try supa help.';line('Unknown command. Try supa help.','terminal-notice');}
});
$('.worm-switch').addEventListener('click',()=>{worm=!worm;$('.worm-switch').setAttribute('aria-pressed',String(worm));$('.worm-switch').textContent=worm?'awake':'asleep';scenes?.setWorm(worm);});
window.addEventListener('keydown',async event=>{
  if(event.defaultPrevented||event.ctrlKey||event.metaKey||event.altKey)return;
  if(event.key==='Escape'){closeCommand(false);returnFocus();return;}
  if(event.target instanceof HTMLInputElement||event.target instanceof HTMLSelectElement||event.target instanceof HTMLTextAreaElement)return;
  if(!characterShortcuts&&event.key.length===1&&!(event.target instanceof Element&&event.target.closest('.wheel,.buffer')))return;
  if(event.key==='Tab'&&$('.projects').classList.contains('drawer-open')){
    const active=$('.wheel-item[aria-selected=true]');if(event.shiftKey&&document.activeElement===$('.tabline')){event.preventDefault();active?.focus();}else if(!event.shiftKey&&document.activeElement===active){event.preventDefault();$('.tabline').focus();}return;
  }
  if(!await ready)return;scenes.onInput(event.key);
  if(event.key===':'){event.preventDefault();openCommand();return;}
  if(document.body.dataset.view==='intro'){if(event.key==='Enter'&&!event.target.closest('button,a'))await enter(event);return;}
  if(event.target instanceof Element&&event.target.closest('.buffer')){const scroll={j:24,ArrowDown:24,k:-24,ArrowUp:-24,d:$('.buffer').clientHeight/2,u:-$('.buffer').clientHeight/2};if(event.key in scroll){event.preventDefault();$('.buffer').scrollBy({top:scroll[event.key]});return;}if(event.key==='h'){event.preventDefault();returnFocus();return;}}
  const steps={j:1,ArrowDown:1,k:-1,ArrowUp:-1};
  if(event.key in steps){event.preventDefault();wheel.step(steps[event.key]);}
  else if(event.key==='g')$('.buffer').scrollTo({top:0});
  else if(event.key==='G')$('.buffer').scrollTo({top:$('.buffer').scrollHeight});
});
window.addEventListener('pagehide',event=>{if(!event.persisted){clearInterval(liveTimer);cancelTyping();clearArtworkMedia();wheel?.destroy();scenes?.destroy();}});

function liveWorkPanel(view,id){
  const panel=node('details','calculation autonomy-panel');panel.dataset.liveWork='evaluation';const providerName=view.policy?.model?.startsWith('claude-')?'Claude':'GPT';panel.append(node('summary','',`${providerName} work evaluation / separate inference accounting`));
  panel.append(node('p','terminal-notice','Owner-only evaluation of unpaid supplied-input jobs. No customer payment or commercial delivery. Model interpretations require human review; exact citations and integer totals are checked by the application. No automatic paid retry.'));
  const feedback=node('p','terminal-notice');feedback.setAttribute('role','status');feedback.setAttribute('aria-live','polite');
  const reload=node('button','','Reload work evaluations');reload.type='button';reload.addEventListener('click',()=>void select(itemIndex));panel.append(reload);
  if(!view.enabled||!view.policy)panel.append(node('p','terminal-notice','Live work is disabled. Credentials, a separate approved budget, exact scope and fresh owner consent are required.'));
  else if(!view.jobs.length)panel.append(node('p','terminal-notice','No unpaid owner-supplied job is available. Create a supplied-input simulated job without recording a payment.'));
  else {
    const policy=view.policy,form=node('form','autonomy-form'),jobs=node('select'),label=node('label','','Unpaid evaluation job');jobs.id='live-work-job';label.htmlFor=jobs.id;
    for(const job of view.jobs){const option=node('option','',`${job.kind} / ${job.objective}`);option.value=job.id;jobs.append(option);}
    const memory=node('input');memory.type='checkbox';const memoryLabel=node('label','autonomy-consent');memoryLabel.append(memory,document.createTextNode('Use up to two relevant, evidence-backed private memories for this evaluation.'));
    const ceiling=node('input');ceiling.type='number';ceiling.id='live-work-ceiling';ceiling.min=String(view.minimum_reservation_microusd);ceiling.max='100000';ceiling.step='1';ceiling.value=ceiling.min;ceiling.required=true;const ceilingLabel=node('label','','Maximum provider cost / microUSD');ceilingLabel.htmlFor=ceiling.id;
    const consent=node('input');consent.type='checkbox';consent.required=true;const consentLabel=node('label','autonomy-consent');consentLabel.append(consent,document.createTextNode('I authorize exactly one provider request for this job and model within the displayed token and cost limits. Real API charges are separate from simulated funds.'));
    const submit=node('button','',`Authorize one ${providerName} work evaluation`);submit.type='submit';
    form.append(node('p','prompt',`Model ${policy.model}. Budget ${policy.budget_id}. Input ceiling ${policy.input_token_bound}; output ceiling ${policy.max_output_tokens}. Reservation ${units(view.minimum_reservation_microusd)} USD. No financial actions or hosted tools.`),label,jobs,memoryLabel,ceilingLabel,ceiling,consentLabel,submit);panel.append(form);
    form.addEventListener('submit',async event=>{
      event.preventDefault();event.stopPropagation();if(!form.reportValidity()||!consent.checked)return;
      const job=view.jobs.find(j=>j.id===jobs.value),now=new Date(),scope={job_id:job.id,expected_revision:job.revision,input_sha256:job.input_sha256,approved_tools:[...job.approved_tools,...(memory.checked?['read_memory']:[])],use_memory:memory.checked,consent:{request_id:crypto.randomUUID(),goal:job.objective,model:policy.model,authorize_paid:true,consented_at:now.toISOString(),expires_at:new Date(now.getTime()+120000).toISOString(),max_requests:1,max_input_tokens:policy.input_token_bound,max_output_tokens:policy.max_output_tokens,max_cost_microusd:Number(ceiling.value)}};
      panel.dataset.economicActionPending='true';submit.disabled=true;jobs.disabled=true;memory.disabled=true;consent.disabled=true;feedback.textContent='Evaluation requested. Do not resubmit; reload saved records after an uncertain result.';
      try{const response=await fetch(`/api/agents/${id}/live-work`,{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/json','X-Supa-Action':'controlled-inference'},body:JSON.stringify(scope),signal:AbortSignal.timeout(65000)});const result=await response.json();feedback.textContent=response.ok&&result.id===scope.consent.request_id?`Saved evaluation ${result.id} / ${result.status}. Reload to inspect evidence and usage.`:'Evaluation not confirmed. Reload saved records; no automatic retry.';}catch{feedback.textContent='Outcome uncertain. Reload saved records and review reservations before any new consent.';}
    });
  }
  for(const session of view.sessions){
    const work=session.work;if(!work)continue;const block=node('details','calculation');block.dataset.workSession=session.id;block.append(node('summary','',`${work.provider_evidence==='mock'?'Mocked GPT work':work.provider_evidence==='anthropic'?'Claude work':'GPT work'} / ${session.status} / ${session.policy.model}`),node('p','prompt',`Session ${session.id} / unpaid job ${work.authorization.job_id}${session.provider_scope?" / provider scope "+session.provider_scope:""}`),node('p','terminal-notice',`Reserved ${units(session.reserved_microusd)} USD / accounted ${units(session.accounted_microusd)} USD${work.provider_evidence==='mock'?' / synthetic provider usage; no real API charge':''}. Simulated operating funds and customer liabilities are unchanged.`));
    const offer=view.jobs.find(job=>job.id===work.authorization.job_id);
    if(offer&&session.usage&&session.status!=='unknown')block.append(node('p','terminal-notice',`Hypothetical offer ${units(offer.hypothetical_service_revenue_atoms)} nominal USD; less usage-based inference ${units(offer.hypothetical_service_revenue_atoms-session.accounted_microusd)} nominal USD. This is an illustrative comparison, not revenue or spendable funds.`));
    if(session.usage)block.append(node('p','prompt',`Provider usage: ${session.usage.input_tokens} input / ${session.usage.output_tokens} output / ${session.usage.cached_input_tokens} cached / ${session.usage.cache_write_tokens} cache-write tokens.`));
    else block.append(node('p','terminal-notice','Provider usage unavailable. An uncertain reservation must be reviewed; it cannot be retried automatically.'));
    if(work.report){block.append(node('h3','terminal-heading',`Model interpretation / ${work.report.status}`),node('p','prompt',work.report.summary),node('p','terminal-notice','Interpretation is unverified model analysis, not an independently established fact or commercial completion.'));
      for(const finding of work.report.findings){block.append(node('p','prompt',finding.observation),node('p','terminal-notice',finding.interpretation));for(const evidence of finding.evidence)block.append(node('p','prompt',`${evidence.source}:${evidence.line} / exact supplied quote\n${evidence.quote}`));}
      for(const missing of work.report.missing_information)block.append(node('p','terminal-notice',`Missing information: ${missing}`));
      block.append(node('p','prompt',`Memory references: ${work.report.memory_ids.join(', ')||'none'}. Proposed read-only tools: ${work.report.tools.join(', ')}.`));
      const full=node('details','calculation');full.append(node('summary','','Full model report and deterministic checks'));const pre=node('pre','',JSON.stringify({report:work.report,baseline:work.baseline,checks:work.tool_results},null,2));pre.style.whiteSpace='pre-wrap';pre.style.overflowWrap='anywhere';full.append(pre);block.append(full);
    }
    if(session.failure_code)block.append(node('p','terminal-notice',session.failure_code));panel.append(block);
  }
  panel.append(feedback);return panel;
}
