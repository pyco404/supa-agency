import {test,expect} from '../../web/node_modules/@playwright/test/index.mjs';
async function ready(page,path='/site/#home'){await page.goto(path);await expect(page.locator('html')).toHaveAttribute('data-ready','true');}
async function commands(page){if(await page.locator('.command').isHidden())await page.locator('.command-toggle').click();}
test('typing can be revealed immediately without opening terminal utilities',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});await ready(page);const skip=page.getByRole('button',{name:'Show all text',exact:true});await expect(page.locator('.buffer')).toHaveAttribute('data-typing-state','typing');await expect(skip).toBeVisible();await expect(page.locator('.command')).toBeHidden();
 const box=await skip.boundingBox();expect(box.height).toBeGreaterThanOrEqual(24);expect(box.width).toBeGreaterThanOrEqual(24);await skip.focus();await page.keyboard.press('Enter');await expect(page.locator('.buffer')).toHaveAttribute('data-typing-state','complete');await expect(page.locator('.buffer')).toBeFocused();expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
});
test('terminal focus contrast remains visible in all three dark palettes',async({page})=>{
 await ready(page,'/');
 for(let palette=0;palette<3;palette++){
  await page.evaluate(value=>localStorage.setItem('supa.dark-theme',String(value)),palette);await ready(page);await page.keyboard.press('Tab');await page.locator('.buffer').focus();
  const ratio=await page.locator('.buffer').evaluate(e=>{const rgb=s=>s.match(/[\d.]+/g).slice(0,3).map(Number);const l=c=>c.map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;}).reduce((a,v,i)=>a+v*[.2126,.7152,.0722][i],0);const a=l(rgb(getComputedStyle(e).outlineColor)),b=l(rgb(getComputedStyle(document.body).backgroundColor));return e.matches(':focus-visible')?(Math.max(a,b)+.05)/(Math.min(a,b)+.05):0;});expect(ratio).toBeGreaterThanOrEqual(3);
 }
});
test('global character shortcuts can be disabled and preference survives reload',async({page})=>{
 await ready(page);await commands(page);const toggle=page.locator('.shortcut-toggle');await toggle.click();await expect(toggle).toHaveAttribute('aria-pressed','false');await page.locator('.command-close').click();await page.locator('.brand').focus();const old=await page.locator('.tab-file').textContent();await page.keyboard.press('j');await page.keyboard.press(':');await expect(page.locator('.tab-file')).toHaveText(old);await expect(page.locator('.command')).toBeHidden();await page.keyboard.press('ArrowDown');await expect(page.locator('.tab-file')).not.toHaveText(old);
 await page.reload();await expect(page.locator('html')).toHaveAttribute('data-ready','true');await commands(page);await expect(page.getByRole('button',{name:'Global character shortcuts off',exact:true})).toHaveAttribute('aria-pressed','false');await page.locator('.command-input').fill('supa help');await page.keyboard.press('Enter');await expect(page.locator('.buffer')).toContainText('UI commands only');
});
test('unmatched inspect commands show a visible recovery message',async({page})=>{
 await ready(page);await commands(page);await page.locator('.command-input').fill('supa inspect no-such-saved-identity');await page.keyboard.press('Enter');const notice=page.locator('.buffer .terminal-notice').filter({hasText:'No matching saved agent. Use supa agents.'});await expect(notice).toBeInViewport();await expect(page.locator('.announcement')).toContainText('No matching saved agent');
});
test('agent picker cycles shared initials and searches a typed prefix',async({page})=>{
 const directory=['Alpha','Alpine','Beta'].map((name,i)=>({id:`11111111-1111-4111-8111-11111111111${i+1}`,name,lifecycle:'paused',created_at:'2026-10-31T00:00:00Z'}));await page.route('**/api/agents',r=>r.fulfill({json:directory}));await ready(page);const trigger=page.locator('#agent-picker-trigger');await expect(trigger).toBeEnabled();await trigger.click();await page.keyboard.press('a');await expect(page.locator('.agent-option:focus')).toContainText('Alpine');await page.keyboard.press('a');await expect(page.locator('.agent-option:focus')).toContainText('Alpha');await page.keyboard.press('Escape');await trigger.click();await page.keyboard.type('Alpi',{delay:40});await expect(page.locator('.agent-option:focus')).toContainText('Alpine');await page.keyboard.press('Enter');await expect(trigger).toContainText('Alpine');await expect(trigger).toBeFocused();await expect(page.locator('#agent-options')).toBeHidden();
});
test('settled entity avoids redundant glyph work and invalidates on theme and resize',async({page})=>{
 await page.addInitScript(()=>{window.entityPaints=0;const original=CanvasRenderingContext2D.prototype.fillText;CanvasRenderingContext2D.prototype.fillText=function(...args){if(this.canvas.classList.contains('entity'))window.entityPaints++;return original.apply(this,args);};});
 await page.emulateMedia({reducedMotion:'no-preference'});await ready(page);await page.getByRole('button',{name:'Show all text',exact:true}).click();await page.evaluate(()=>document.fonts.ready);await page.waitForTimeout(150);const count=await page.evaluate(()=>window.entityPaints);await page.waitForTimeout(600);expect(await page.evaluate(()=>window.entityPaints)-count).toBeLessThanOrEqual(3000);
 await commands(page);await page.locator('.command-input').fill('theme');const before=await page.evaluate(()=>window.entityPaints);await page.keyboard.press('Enter');await expect.poll(()=>page.evaluate(()=>window.entityPaints)).toBeGreaterThan(before);const resized=await page.evaluate(()=>window.entityPaints);const size=page.viewportSize();await page.setViewportSize({width:size.width-10,height:size.height});await expect.poll(()=>page.evaluate(()=>window.entityPaints)).toBeGreaterThan(resized);await page.emulateMedia({reducedMotion:'reduce'});await page.waitForTimeout(100);const pixels=await page.locator('.entity').evaluate(c=>c.toDataURL());await page.waitForTimeout(250);expect(await page.locator('.entity').evaluate(c=>c.toDataURL())).toBe(pixels);
});

test('completed text keeps a blinking caret and honors reduced motion',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});await ready(page,'/about');
 const buffer=page.locator('.buffer');await expect(buffer).toHaveAttribute('data-typing-state','typing');
 expect(await buffer.locator(':scope > :last-child').evaluate(el=>getComputedStyle(el,'::after').content)).toBe('none');
 await page.getByRole('button',{name:'Show all text',exact:true}).click();await expect(buffer).toHaveAttribute('data-typing-state','complete');
 const caret=buffer.locator(':scope > :last-child');const text=await buffer.innerText();
 const blink=await caret.evaluate(el=>{const style=getComputedStyle(el,'::after'),animation=el.getAnimations({subtree:true}).find(a=>a.animationName==='terminal-caret-blink');if(!animation)return {animation:style.animationName,content:style.content,width:style.width,view:document.body.dataset.view,all:document.getAnimations().map(a=>a.animationName)};animation.pause();animation.currentTime=100;const visible=getComputedStyle(el,'::after').opacity;animation.currentTime=700;const hidden=getComputedStyle(el,'::after').opacity;animation.play();return {width:style.width,visible,hidden};});
 expect(blink).toEqual({width:'1px',visible:'1',hidden:'0'});expect(await buffer.innerText()).toBe(text);
 await page.emulateMedia({reducedMotion:'reduce'});expect(await caret.evaluate(el=>getComputedStyle(el,'::after').animationName)).toBe('none');expect(await caret.evaluate(el=>getComputedStyle(el,'::after').opacity)).toBe('1');
 await ready(page,'/agents');await expect(buffer).toHaveAttribute('data-typing-state','complete');expect(await buffer.locator(':scope > :last-child').evaluate(el=>getComputedStyle(el,'::after').width)).toBe('1px');
 await page.emulateMedia({reducedMotion:'no-preference'});await ready(page,'/agents');await expect(buffer).toHaveAttribute('data-typing-state','complete',{timeout:20000});expect(await buffer.locator(':scope > :last-child').evaluate(el=>getComputedStyle(el,'::after').animationName)).toBe('terminal-caret-blink');
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
});

test('GitHub labels show a private-source notice without opening a repository',async({page,context})=>{
 await ready(page,'/about');
 const start=page.url();let popups=0;context.on('page',()=>popups++);
 const github=page.getByRole('button',{name:'github',exact:true});
 const source=page.getByRole('button',{name:'Source / github.com',exact:true});
 for(const button of [github,source]){
  const shown=page.waitForEvent('dialog');const clicked=button.click();const dialog=await shown;
  expect(dialog.type()).toBe('alert');expect(dialog.message()).toBe('GitHub account private, will be open sourced soon.');await dialog.accept();await clicked;expect(page.url()).toBe(start);
 }
 await github.focus();const keyboard=page.waitForEvent('dialog');const pressed=page.keyboard.press('Enter');const dialog=await keyboard;expect(dialog.message()).toContain('will be open sourced soon');await dialog.accept();await pressed;
 expect(popups).toBe(0);await expect(page.locator('a[href*="github.com"]')).toHaveCount(0);
});
