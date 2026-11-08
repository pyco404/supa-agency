import {test,expect} from '../../web/node_modules/@playwright/test/index.mjs';
import {readFile} from 'node:fs/promises';
import {resolve} from 'node:path';

const base='http://127.0.0.1:3201';
const evidence=resolve(import.meta.dirname,'../../artifacts/nav-motion');
async function preview(page,{clips=true,brokenWebm=false,brokenAll=false}={}){
 const observed={clips:[],private:[],errors:[]};page.on('pageerror',e=>observed.errors.push(e.name));
 await page.route('**/*',async route=>{
  const request=route.request(),url=new URL(request.url());
  if(url.origin!==base||url.pathname.startsWith('/api/')||!['GET','HEAD'].includes(request.method())){observed.private.push(url.pathname);return route.abort();}
  if(url.pathname==='/platform.js'&&!clips){
   const response=await route.fetch();let body=await response.text();
   for(const id of ['economy','social','trading','factory'])body=body.replace(`${id}: Object.freeze({ webm: '/public/assets/supa-${id}-loop.webm', mp4: '/public/assets/supa-${id}-loop.mp4'`,`${id}: Object.freeze({ webm: null, mp4: null`);
   return route.fulfill({response,body});
  }
  if(/-loop\.(webm|mp4)$/.test(url.pathname)){
   observed.clips.push(url.pathname);
   if(brokenAll||(brokenWebm&&url.pathname.endsWith('.webm')))return route.fulfill({status:404,body:''});
   if(process.env.SUPA_NAV_MOTION_CANDIDATES==='1'){
    const format=url.pathname.split('.').pop(),file=resolve(import.meta.dirname,'../motion-candidates',url.pathname.split('/').pop());
    return route.fulfill({contentType:'video/'+format,body:await readFile(file)});
   }
   return route.continue();
  }
  if(['/', '/home','/agents','/intelligence','/memory','/economy','/social','/trading','/factory','/about'].includes(url.pathname)){
   const response=await route.fetch({url:base+(url.pathname==='/home'?'/':url.pathname)});
   return route.fulfill({response,body:(await response.text()).replace('data-motion="running"','data-motion="running" data-public-landing="true"')});
  }
  return route.continue();
 });return observed;
}
async function open(page,path='/economy'){
 await page.goto(path);await expect(page.locator('html')).toHaveAttribute('data-ready','true');
 if(page.viewportSize().width<=700)await page.locator('.tabline').click();
 await expect(page.locator('.wheel')).toHaveAttribute('data-carousel-state','settled');
}
const video=page=>page.locator('.wheel-motion');
test.afterEach(async({page})=>{await page.unrouteAll({behavior:'wait'});});

test('only the selected visible card loops, preserves geometry and releases inactive media',async({page},info)=>{
 await page.emulateMedia({reducedMotion:'no-preference'});const observed=await preview(page);await open(page,'/home');
 await expect(video(page)).toHaveCount(0);expect(observed.clips).toEqual([]);
 await page.locator('.wheel').focus();await page.keyboard.press('End'); // About stays static.
 await expect(video(page)).toHaveCount(0);
 await open(page,'/economy');
 await expect(video(page)).toHaveCount(1);await expect(page.locator('.wheel-media[data-playing=true]')).toHaveCount(1);
 const geometry=await video(page).evaluate(v=>{const r=v.getBoundingClientRect(),image=v.parentElement.querySelector('img'),i=image.getBoundingClientRect();return {controls:v.controls,autoplay:v.autoplay,muted:v.muted,inline:v.playsInline,loop:v.loop,fit:getComputedStyle(v).objectFit,width:v.clientWidth,height:v.clientHeight,match:Math.abs(r.width-i.width)<1&&Math.abs(r.height-i.height)<1,clipped:getComputedStyle(v.parentElement).overflow,tab:v.tabIndex};});
 expect(geometry).toMatchObject({controls:false,autoplay:true,muted:true,inline:true,loop:true,fit:'cover',match:true,clipped:'hidden',tab:-1});expect(geometry.width/geometry.height).toBeGreaterThan(1.25);
 // Actual decoder advances, and wraps the four-second fixture without a new source.
 await expect.poll(()=>video(page).evaluate(v=>v.currentTime)).toBeGreaterThan(.2);
 await video(page).evaluate(v=>v.currentTime=3.85);
 await expect.poll(()=>video(page).evaluate(v=>v.currentTime),{timeout:3000}).toBeLessThan(2);
 await page.screenshot({path:resolve(evidence,info.project.name+'-selected.png')});
 await page.locator('.wheel').focus();await page.keyboard.press('ArrowDown');
 await expect(page.locator('.wheel-item[aria-selected=true]')).toHaveAttribute('data-index','5');
 await expect(video(page)).toHaveCount(1);await expect(video(page).locator('source').first()).toHaveAttribute('src',/supa-social-loop.webm$/);
 await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,get:()=>true});document.dispatchEvent(new Event('visibilitychange'));});await expect(video(page)).toHaveCount(0);
 await page.evaluate(()=>{delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));});await expect(video(page)).toHaveCount(1);
 await page.locator('.wheel').evaluate(el=>el.style.display='none');await expect(video(page)).toHaveCount(0);
 await page.locator('.wheel').evaluate(el=>el.style.removeProperty('display'));await expect(video(page)).toHaveCount(1);
 await page.keyboard.press(':');
 const control=page.locator('.branch-motion');await control.click();await expect(video(page)).toHaveCount(0);
 await control.click();await expect(video(page)).toHaveCount(1);await page.locator('.command-close').click();
 if(page.viewportSize().width<=700){await page.keyboard.press('Escape');await expect(video(page)).toHaveCount(0);}
 expect(observed.private).toEqual([]);expect(observed.errors).toEqual([]);
});

test('reduced motion never fetches clips and preference changes restore posters',async({page})=>{
 const observed=await preview(page);await open(page);await expect(video(page)).toHaveCount(0);expect(observed.clips).toEqual([]);
 await page.emulateMedia({reducedMotion:'no-preference'});await expect(video(page)).toHaveCount(1);
 await page.emulateMedia({reducedMotion:'reduce'});await expect(video(page)).toHaveCount(0);
 await expect(page.locator('.wheel-item[aria-selected=true] img')).toBeVisible();expect(observed.errors).toEqual([]);
});

test('MP4 fallback decodes when WebM is unavailable, and total failure preserves poster',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});const observed=await preview(page,{brokenWebm:true});await open(page);
 await expect(page.locator('.wheel-media[data-playing=true]')).toHaveCount(1);
 await expect.poll(()=>video(page).evaluate(v=>v.currentSrc)).toMatch(/\.mp4$/);
 expect(observed.clips.some(p=>p.endsWith('.mp4'))).toBe(true);
 await page.unrouteAll({behavior:'wait'});const failed=await preview(page,{brokenAll:true});await open(page);
 await expect.poll(()=>failed.clips.length).toBeGreaterThan(0);await expect(video(page)).toHaveCount(0);
 await expect(page.locator('.wheel-item[aria-selected=true] img')).toBeVisible();
 const requests=failed.clips.length;await page.waitForTimeout(300);expect(failed.clips.length).toBe(requests);expect(failed.errors).toEqual([]);
});

test('missing-media configuration stays static, preserving navigation and private boundaries',async({page,request},info)=>{
 const observed=await preview(page,{clips:false});await open(page);await expect(video(page)).toHaveCount(0);expect(observed.clips).toEqual([]);
 await expect(page.locator('.wheel-item')).toHaveCount(9);await page.screenshot({path:resolve(evidence,info.project.name+'-static.png')});
 for(const path of ['/operator','/my-agents','/api/agents/11111111-1111-4111-8111-111111111111/memory'])expect((await request.get(path)).status()).toBe(401);
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);expect(observed.private).toEqual([]);
});

test('media rejects external or traversal URLs and destroy releases its decoder',async({page})=>{
 await preview(page,{clips:false});await open(page);
 const result=await page.evaluate(async()=>{
  const {createCardMedia}=await import('/wheel.js');
  const unsafe=createCardMedia({thumb:'/public/assets/supa-economy-thumb.webp',media:{webm:'https://outside.invalid/secret.webm',mp4:'/public/assets/../private.mp4'}});document.body.append(unsafe.element);unsafe.setActive(true);const rejected=!unsafe.element.querySelector('video');unsafe.destroy();unsafe.element.remove();
  const safe=createCardMedia({thumb:'/public/assets/supa-economy-thumb.webp',media:{webm:'/public/assets/supa-economy-loop.webm'}});document.body.append(safe.element);safe.setActive(true);const v=safe.element.querySelector('video');safe.destroy();const released=!v.isConnected&&v.querySelectorAll('source').length===0&&v.paused;safe.element.remove();return {rejected,released};
 });expect(result).toEqual({rejected:true,released:true});
});

test('autoplay rejection leaves the static fallback without repeated attempts',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});
 await page.addInitScript(()=>{HTMLMediaElement.prototype.play=function(){return Promise.reject(new DOMException('Autoplay blocked','NotAllowedError'));};});
 const observed=await preview(page);await open(page);await expect(video(page)).toHaveCount(0);
 await expect(page.locator('.wheel-item[aria-selected=true] img')).toBeVisible();expect(observed.errors).toEqual([]);
});

test('all four published clips play through the real card layer without geometry changes',async({page},info)=>{
 await page.emulateMedia({reducedMotion:'no-preference'});const observed=await preview(page),metrics=[];
 for(const section of ['factory','economy','social','trading']){
  await open(page,'/'+section);await expect(video(page)).toHaveCount(1);await expect(page.locator('.wheel-media[data-playing=true]')).toHaveCount(1);
  await expect.poll(()=>video(page).evaluate(v=>v.currentTime)).toBeGreaterThan(.3);
  const before=await video(page).evaluate(v=>{const b=v.parentElement.getBoundingClientRect();return {x:b.x,y:b.y,w:b.width,h:b.height};});
  const sample=async()=>video(page).evaluate(v=>{const q=v.getVideoPlaybackQuality();return {width:v.videoWidth,height:v.videoHeight,seconds:v.duration,decoded:q.totalVideoFrames,dropped:q.droppedVideoFrames};});
  const startup=await sample();await page.waitForTimeout(2000);const quality=await sample();
  const after=await video(page).evaluate(v=>{const b=v.parentElement.getBoundingClientRect();return {x:b.x,y:b.y,w:b.width,h:b.height};});
  for(const key of ['x','y','w','h'])expect(Math.abs(after[key]-before[key])).toBeLessThan(1);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  expect(quality.width).toBe(480);expect(quality.height).toBe(300);expect(quality.seconds).toBeCloseTo(4,1);
  const decoded=quality.decoded-startup.decoded,dropped=quality.dropped-startup.dropped;
  expect(decoded).toBeGreaterThanOrEqual(20);expect(dropped/decoded).toBeLessThan(.2);metrics.push({section,...quality,startup_decoded:startup.decoded,startup_dropped:startup.dropped,sample_decoded:decoded,sample_dropped:dropped});
  await page.screenshot({path:resolve(import.meta.dirname,'../../artifacts/navigation-motion-candidates',info.project.name+'-'+section+'.png')});
 }
 expect(observed.private).toEqual([]);expect(observed.errors).toEqual([]);await info.attach('candidate-playback-quality',{body:JSON.stringify(metrics),contentType:'application/json'});
});
