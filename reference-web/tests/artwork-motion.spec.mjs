import {test,expect} from '../../web/node_modules/@playwright/test/index.mjs';

const base='http://127.0.0.1:3201';
const artwork=page=>page.locator('.section-illustration[data-section=factory]');
const movie=page=>artwork(page).locator('video');
async function open(page,{broken=false,mp4=false}={}){
 const observed={private:[],errors:[]};page.on('pageerror',e=>observed.errors.push(e.name));
 await page.route('**/*',async route=>{
  const r=route.request(),url=new URL(r.url());
  if(url.origin!==base||url.pathname.startsWith('/api/')||!['GET','HEAD'].includes(r.method())){observed.private.push(url.pathname);return route.abort();}
  if(/-loop\.(webm|mp4)$/.test(url.pathname)&&(broken||(mp4&&url.pathname.endsWith('.webm'))))return route.fulfill({status:404,body:''});
  if(['/factory','/home'].includes(url.pathname)){
   const response=await route.fetch();return route.fulfill({response,body:(await response.text()).replace('data-motion="running"','data-motion="running" data-public-landing="true"')});
  }
  return route.continue();
 });
 await page.goto('/factory');await expect(page.locator('html')).toHaveAttribute('data-ready','true');
 const skip=page.locator('.skip-typing');if(await skip.isVisible())await skip.click();
 await expect(artwork(page)).toBeVisible();await artwork(page).scrollIntoViewIfNeeded();
 return observed;
}
async function paintedChange(page,a,b){
 return page.evaluate(async([first,last])=>{
  const decode=async bytes=>{const bitmap=await createImageBitmap(new Blob([new Uint8Array(bytes)],{type:'image/png'}));const canvas=document.createElement('canvas');canvas.width=bitmap.width;canvas.height=bitmap.height;const ctx=canvas.getContext('2d');ctx.drawImage(bitmap,0,0);bitmap.close();return ctx.getImageData(0,0,canvas.width,canvas.height);};
  const [x,y]=await Promise.all([decode(first),decode(last)]);if(x.width!==y.width||x.height!==y.height)throw Error('artwork_geometry_changed');
  let changed=0,total=0;for(let row=2;row<x.height-2;row++)for(let col=2;col<x.width-2;col++){const i=4*(row*x.width+col);total++;if(Math.max(...[0,1,2].map(c=>Math.abs(x.data[i+c]-y.data[i+c])))>8)changed++;}return 100*changed/total;
 },[Array.from(a),Array.from(b)]);
}
test.afterEach(async({page})=>page.unrouteAll({behavior:'wait'}));

test('Factory artwork visibly moves and naturally loops in its unchanged frame',async({page},info)=>{
 await page.emulateMedia({reducedMotion:'no-preference'});const observed=await open(page);
 await expect.poll(()=>movie(page).evaluate(v=>v.currentTime)).toBeGreaterThan(.3);
 const state=await movie(page).evaluate(v=>{const rect=v.getBoundingClientRect(),p=v.closest('.section-art-media').getBoundingClientRect();window.artworkWraps=0;window.artworkLast=v.currentTime;v.addEventListener('timeupdate',()=>{if(v.currentTime<window.artworkLast)window.artworkWraps++;window.artworkLast=v.currentTime;});return {muted:v.muted,loop:v.loop,autoplay:v.autoplay,inline:v.playsInline,controls:v.controls,duration:v.duration,videoWidth:v.videoWidth,videoHeight:v.videoHeight,fit:getComputedStyle(v).objectFit,decoded:v.getVideoPlaybackQuality().totalVideoFrames,dropped:v.getVideoPlaybackQuality().droppedVideoFrames,width:rect.width,height:rect.height,frameWidth:p.width,frameHeight:p.height};});
 expect(state).toMatchObject({muted:true,loop:true,autoplay:true,inline:true,controls:false,duration:4,videoWidth:960,videoHeight:600,fit:'cover'});
 expect(state.width).toBeCloseTo(state.frameWidth,0);expect(state.height).toBeCloseTo(state.frameHeight,0);expect(state.width/state.height).toBeCloseTo(16/9,1);
 const picture=artwork(page).locator('picture'),a=await picture.screenshot();await page.waitForTimeout(1200);const b=await picture.screenshot();
 const changed=await paintedChange(page,a,b);expect(changed).toBeGreaterThan(1);
 const quality=await movie(page).evaluate(v=>{const q=v.getVideoPlaybackQuality();return {totalVideoFrames:q.totalVideoFrames,droppedVideoFrames:q.droppedVideoFrames};});expect(quality.totalVideoFrames-state.decoded).toBeGreaterThanOrEqual(15);expect((quality.droppedVideoFrames-state.dropped)/(quality.totalVideoFrames-state.decoded)).toBeLessThan(.2);
 await expect(movie(page).locator('source').first()).toHaveAttribute('src','/public/assets/supa-factory-artwork-loop.webm');
 await expect.poll(()=>page.evaluate(()=>window.artworkWraps),{timeout:6000}).toBeGreaterThan(0);
 await expect(page.locator('.section-illustration[data-section=intelligence]')).toHaveCount(1);
 await expect(page.locator('.section-illustration[data-section=intelligence] video')).toHaveCount(0);
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
 await info.attach('visible-motion',{body:JSON.stringify({changedPixelsPercent:changed,...state}),contentType:'application/json'});
 await info.attach('factory-motion',{body:b,contentType:'image/png'});
 await page.screenshot({path:`../artifacts/artwork-motion/${info.project.name}-factory.png`});
 expect(observed.private).toEqual([]);expect(observed.errors).toEqual([]);
});

test('offscreen, hidden, reduced-motion and navigation release page playback',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});const observed=await open(page);await expect(movie(page)).toHaveCount(1);
 await movie(page).evaluate(v=>window.releasedArtwork=v);
 await page.locator('.section-illustration[data-section=intelligence]').scrollIntoViewIfNeeded();await expect(movie(page)).toHaveCount(0);
 expect(await page.evaluate(()=>window.releasedArtwork.paused)).toBe(true);
 await artwork(page).scrollIntoViewIfNeeded();await expect(movie(page)).toHaveCount(1);
 await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,get:()=>true});document.dispatchEvent(new Event('visibilitychange'));});await expect(movie(page)).toHaveCount(0);
 await page.evaluate(()=>{delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));});await expect(movie(page)).toHaveCount(1);
 const geometry=await artwork(page).locator('picture').boundingBox();
 await page.emulateMedia({reducedMotion:'reduce'});
 await expect.poll(()=>page.evaluate(()=>({motion:document.documentElement.dataset.motion,matches:matchMedia('(prefers-reduced-motion: reduce)').matches,videos:document.querySelectorAll('.section-motion-media video').length}))).toEqual({motion:'still',matches:true,videos:0});
 const poster=artwork(page).locator('picture > img');await expect(poster).toBeVisible();expect(await poster.evaluate(v=>v.complete&&v.naturalWidth>0)).toBe(true);
 expect(await artwork(page).locator('picture').boundingBox()).toEqual(geometry);
 await page.emulateMedia({reducedMotion:'no-preference'});await expect(movie(page)).toHaveCount(1);
 if(page.viewportSize().width<=700){await page.locator('.tabline').click();await expect(movie(page)).toHaveCount(0);await page.locator('.tabline').click();await expect(movie(page)).toHaveCount(1);}
 await page.locator('.brand').click();await expect(page.locator('.section-motion-media video')).toHaveCount(0);
 expect(observed.private).toEqual([]);expect(observed.errors).toEqual([]);
});

test('reduced motion never requests clips; failed clips retain original responsive poster',async({page})=>{
 await page.emulateMedia({reducedMotion:'reduce'});let requests=0;page.on('request',r=>{if(/-loop\.(webm|mp4)$/.test(r.url()))requests++;});
 const observed=await open(page,{broken:true});await expect(movie(page)).toHaveCount(0);expect(requests).toBe(0);
 await page.emulateMedia({reducedMotion:'no-preference'});await expect.poll(()=>requests).toBeGreaterThan(0);await expect(movie(page)).toHaveCount(0);
 await expect(artwork(page).locator('picture > img')).toBeVisible();
 expect(await artwork(page).locator('.section-motion-media').evaluate(el=>getComputedStyle(el).backgroundColor)).toBe('rgba(0, 0, 0, 0)');
 expect(observed.private).toEqual([]);expect(observed.errors).toEqual([]);
});

test('MP4 fallback decodes when WebM is unavailable',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});const observed=await open(page,{mp4:true});
 await expect.poll(()=>movie(page).evaluate(v=>v.currentSrc)).toMatch(/\.mp4$/);
 await expect.poll(()=>movie(page).evaluate(v=>v.currentTime)).toBeGreaterThan(.2);
 expect(observed.private).toEqual([]);expect(observed.errors).toEqual([]);
});

test('preference-change payload immediately releases the decoder before a fresh media query',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});
 await page.addInitScript(()=>{const native=window.matchMedia.bind(window);window.artworkQueries=[];window.matchMedia=query=>{const result=native(query);if(query==='(prefers-reduced-motion: reduce)')window.artworkQueries.push(result);return result;};});
 await open(page);await expect(movie(page)).toHaveCount(1);
 await page.evaluate(()=>{for(const query of window.artworkQueries)query.dispatchEvent(new MediaQueryListEvent('change',{matches:true,media:query.media}));});
 await expect(movie(page)).toHaveCount(0);
 // Visibility callbacks must not restart playback while a reduced-motion
 // event is newer than the query snapshot (or an emulated query update).
 await page.locator('.section-illustration[data-section=intelligence]').scrollIntoViewIfNeeded();
 await artwork(page).scrollIntoViewIfNeeded();await expect(movie(page)).toHaveCount(0);
 await page.evaluate(()=>{for(const query of window.artworkQueries)query.dispatchEvent(new MediaQueryListEvent('change',{matches:false,media:query.media}));});
 await expect(movie(page)).toHaveCount(1);
});
