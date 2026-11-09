/** Read-only browser validation of the actual running local preview. No fixture rewrite. */
import {chromium} from '../web/node_modules/playwright/index.mjs';
import assert from 'node:assert/strict';
import {mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
const base='http://127.0.0.1:3200',out=resolve(import.meta.dirname,'../artifacts/nav-playback-diagnosis');
mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/usr/bin/google-chrome',headless:true});
const report={base,assets:[],playback:[],checks:[],passed:false};
const sections=['factory','economy','social','trading'];
async function waitFor(page,predicate){await page.waitForFunction(predicate,{},{timeout:10000});}
async function select(page,section,mobile){
 await page.goto(base+'/'+section,{waitUntil:'domcontentloaded'});
 await waitFor(page,()=>document.documentElement.dataset.ready==='true');
 if(mobile)await page.locator('.tabline').click();
 // Let the existing 780 ms wheel entrance finish before tracking one decoder.
 await page.waitForTimeout(850);
 await waitFor(page,()=>document.querySelector('.wheel')?.dataset.carouselState==='settled');
 await waitFor(page,()=>document.querySelector('.wheel-media[data-playing=true] video')?.readyState>=2);
}
const sample=page=>page.locator('video').evaluate(v=>({time:v.currentTime,frames:v.getVideoPlaybackQuality().totalVideoFrames,dropped:v.getVideoPlaybackQuality().droppedVideoFrames,paused:v.paused,src:new URL(v.currentSrc).pathname,controls:v.controls,muted:v.muted,loop:v.loop,inline:v.playsInline,width:v.videoWidth,height:v.videoHeight,seconds:v.duration}));
try{
 for(const viewport of ['desktop','tablet','mobile']){
  const mobile=viewport==='mobile';
  const context=await browser.newContext({viewport:mobile?{width:390,height:844}:viewport==='tablet'?{width:768,height:1024}:{width:1440,height:1000},isMobile:mobile,hasTouch:mobile,reducedMotion:'no-preference'});
  const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.name));
  const writes=[],privateReads=[];
  page.on('request',request=>{
   const path=new URL(request.url()).pathname;
   if(!['GET','HEAD'].includes(request.method()))writes.push(path);
   if(path.startsWith('/api/')&&!['/api/status','/api/agents'].includes(path))privateReads.push(path);
  });
  if(viewport==='desktop')for(const path of ['/','/agents','/intelligence','/memory','/economy','/social','/trading','/factory','/about'])assert.equal((await context.request.get(base+path)).status(),200,'public_section_remains_open');
  if(viewport==='desktop')for(const section of sections)for(const ext of ['webm','mp4']){
   const path=`/public/assets/supa-${section}-loop.${ext}`,r=await context.request.get(base+path);
   assert.equal(r.status(),200);assert.equal(r.headers()['content-type'],'video/'+ext);
   report.assets.push({path,status:r.status(),mime:r.headers()['content-type']});
  }
  for(const section of sections){
   await select(page,section,mobile);
   assert.equal(await page.locator('video').count(),1);
   await page.locator('video').evaluate(v=>{v.dataset.wraps='0';let previous=v.currentTime;v.addEventListener('timeupdate',()=>{if(previous-v.currentTime>2)v.dataset.wraps=String(Number(v.dataset.wraps)+1);previous=v.currentTime;});});
   await page.locator('video').evaluate(v=>{const c=document.createElement('canvas');c.width=200;c.height=125;const ctx=c.getContext('2d');ctx.drawImage(v,0,0,200,125);v._firstFrame=ctx.getImageData(0,0,200,125).data;});
   const before=await sample(page);await page.waitForTimeout(1300);const after=await sample(page);
   const changedPixels=await page.locator('video').evaluate(v=>{const c=document.createElement('canvas');c.width=200;c.height=125;const ctx=c.getContext('2d');ctx.drawImage(v,0,0,200,125);const now=ctx.getImageData(0,0,200,125).data,old=v._firstFrame;let changed=0;for(let i=0;i<now.length;i+=4)if(Math.max(Math.abs(now[i]-old[i]),Math.abs(now[i+1]-old[i+1]),Math.abs(now[i+2]-old[i+2]))>8)changed++;delete v._firstFrame;return 100*changed/25000;});
   assert(changedPixels>.2,'visible_card_size_pixel_change_required');
   assert(after.time!==before.time&&after.frames-before.frames>=15,'time_and_decoder_must_advance');
   assert(!after.paused&&!after.controls&&after.muted&&after.loop&&after.inline,'required_media_flags');
   await page.waitForFunction(()=>Number(document.querySelector('video')?.dataset.wraps)>=1,{},{timeout:6000});
   const wrapped=await sample(page);assert(wrapped.frames>after.frames,'loop_decodes_more_frames');
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
   const ratio=(wrapped.dropped-before.dropped)/(wrapped.frames-before.frames);
   assert(ratio<.2,'steady_state_decoder_drop_budget');
   report.playback.push({viewport,section,before,after,wrapped,changed_pixels_percent:changedPixels,natural_loop_verified:true,decoded_drop_ratio:ratio});
   await page.screenshot({path:resolve(out,`${viewport}-${section}.png`)});
  }
  // Change selected section using the wheel itself, not a reload.
  await page.locator('.wheel').focus();await page.keyboard.press('ArrowUp');
  await page.waitForFunction(()=>document.querySelector('video')?.currentSrc.includes('social-loop'));
  report.checks.push({viewport,selected_switch:true});
  await page.locator('.wheel').evaluate(el=>el.style.display='none');await waitFor(page,()=>!document.querySelector('video'));
  await page.locator('.wheel').evaluate(el=>el.style.removeProperty('display'));await waitFor(page,()=>!!document.querySelector('video'));
  await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,get:()=>true});document.dispatchEvent(new Event('visibilitychange'));});await waitFor(page,()=>!document.querySelector('video'));
  await page.evaluate(()=>{delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));});await waitFor(page,()=>!!document.querySelector('video'));
  await page.emulateMedia({reducedMotion:'reduce'});await waitFor(page,()=>!document.querySelector('video'));
  assert(await page.locator('.wheel-item[aria-selected=true] img').isVisible());
  report.checks.push({viewport,offscreen_release:true,hidden_release:true,reduced_motion_poster:true});
  await page.emulateMedia({reducedMotion:'no-preference'});
  await page.route('**/*-loop.webm',route=>route.fulfill({status:404,body:''}));await select(page,'economy',mobile);
  assert((await sample(page)).src.endsWith('.mp4'),'actual_mp4_decode_required');
  await page.route('**/*-loop.mp4',route=>route.fulfill({status:404,body:''}));
  await page.goto(base+'/factory');if(mobile)await page.locator('.tabline').click();
  await page.waitForTimeout(1000);await waitFor(page,()=>!document.querySelector('video'));
  assert(await page.locator('.wheel-item[aria-selected=true] img').isVisible());
  for(const path of ['/operator','/my-agents'])assert.equal((await context.request.get(base+path)).status(),401);
  assert.deepEqual(errors,[]);
  assert.deepEqual(writes,[]);assert.deepEqual(privateReads,[]);
  report.checks.push({viewport,mp4_decode:true,missing_media_poster:true,private_routes_denied:true,javascript_errors:errors,unexpected_private_reads:privateReads,writes});
  await context.close();
 }
 report.passed=true;
}finally{await browser.close();writeFileSync(resolve(out,'actual-preview-playback.json'),JSON.stringify(report,null,2)+'\n');}
console.log(JSON.stringify({passed:report.passed,assets:report.assets.length,card_viewport_checks:report.playback.length,results:report.playback.map(p=>({viewport:p.viewport,section:p.section,time_delta:p.after.time-p.before.time,decoded_delta:p.after.frames-p.before.frames,natural_loop:p.natural_loop_verified}))},null,2));
