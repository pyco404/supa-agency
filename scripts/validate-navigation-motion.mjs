/** Decode-only candidate review: no image editing, services, credentials or CI. */
import {spawnSync} from 'node:child_process';
import {readFileSync,statSync,lstatSync,mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {navigationMotion} from '../reference-web/platform.js';
const root=resolve(import.meta.dirname,'..'),out=resolve(root,'artifacts/navigation-motion-candidates');mkdirSync(out,{recursive:true});
const current=resolve(root,'reference-web/navigation-motion.json');
const manifest=JSON.parse(readFileSync(current));
assert(['local_playback_refresh','local_articulated_review'].includes(manifest.status));
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
function run(tool,args){const r=spawnSync(tool,args,{timeout:90000,maxBuffer:50000000});assert.equal(r.status,0,'candidate_decode_failed');return r.stdout;}
function difference(a,b){let sum=0;for(let i=0;i<a.length;i++)sum+=Math.abs(a[i]-b[i]);return sum/a.length;}
assert.deepEqual(manifest.candidates.map(c=>c.section),['factory','economy','social','trading']);
const reports=[];
const enabled=Object.values(navigationMotion).some(media=>media.webm||media.mp4);
for(const candidate of manifest.candidates){
 assert.equal(candidate.production_enabled,false);assert.equal(candidate.artwork_replaced,false);assert.equal(candidate.width,320);assert.equal(candidate.height,200);
 const source=resolve(root,candidate.source);assert.equal(hash(source),candidate.source_sha256);
 const original=run('ffmpeg',['-v','error','-i',source,'-vf','scale=480:480:flags=lanczos,crop=480:300:0:83,scale=320:200:flags=lanczos,format=gray','-frames:v','1','-f','rawvideo','pipe:1']);
 for(const format of ['webm','mp4']){
  const file=resolve(root,`reference-web/public/assets/supa-${candidate.section}-loop.${format}`);assert(!lstatSync(file).isSymbolicLink());assert.equal(hash(file),candidate[format+'_sha256']);
  if(enabled){
   const publicPath=`/public/assets/supa-${candidate.section}-loop.${format}`;
   assert.equal(navigationMotion[candidate.section][format],publicPath,'exact_public_motion_mapping');
   const published=resolve(root,'reference-web'+publicPath);assert(!lstatSync(published).isSymbolicLink());
   assert.equal(hash(published),candidate[format+'_sha256'],'public_copy_matches_reviewed_candidate');
  }
  const probe=JSON.parse(run('ffprobe',['-v','error','-show_streams','-show_format','-of','json',file]));assert.equal(probe.streams.length,1,'silent_video_only');const stream=probe.streams[0];
  assert.equal(stream.codec_type,'video');assert.equal(stream.codec_name,format==='webm'?'vp9':'h264');assert.equal(stream.width,320);assert.equal(stream.height,200);assert.equal(stream.avg_frame_rate,'20/1');assert(Math.abs(Number(probe.format.duration)-4)<.02);
  const bytes=statSync(file).size;assert(bytes<(format==='webm'?250:400)*1024,'candidate_asset_budget');
  if(format==='mp4'){const data=readFileSync(file);assert(data.indexOf('moov')>0&&data.indexOf('moov')<data.indexOf('mdat'),'mp4_faststart');}
  const decoded=run('ffmpeg',['-v','error','-i',file,'-pix_fmt','gray','-f','rawvideo','pipe:1']);const size=320*200;assert.equal(decoded.length,size*80,'exact_frame_count');
  let maxStep=0,maxSourceDifference=0;const first=decoded.subarray(0,size),uniqueFrames=new Set();
  for(let frame=0;frame<80;frame++){
   const current=decoded.subarray(frame*size,(frame+1)*size);maxSourceDifference=Math.max(maxSourceDifference,difference(current,original));
   uniqueFrames.add(createHash('sha256').update(current).digest('hex'));
   if(frame)maxStep=Math.max(maxStep,difference(current,decoded.subarray((frame-1)*size,frame*size)));
  }
  const seam=difference(first,decoded.subarray(79*size,80*size));
  console.log(JSON.stringify({section:candidate.section,format,maxStep,seam,maxSourceDifference}));
  // Compare the wrap with the motion inside the clip, allowing <= 0.5 luma
  // levels for I/P-frame quantization. Both remain below 0.6% of full scale.
  assert(maxStep>.01,'nonstatic_candidate');assert(maxStep<1.5&&seam<1.5&&seam<=maxStep+.5,'no_large_frame_or_loop_jump');assert(maxSourceDifference<8,'preserve_source_composition');
  assert(uniqueFrames.size>=75,'continuous_motion_not_a_still_video_wrapper');
  // Stronger motion must be perceptible after downsampling to the real card size.
  const small=run('ffmpeg',['-v','error','-i',file,'-vf',"select='eq(n,0)+eq(n,20)',scale=200:125,format=gray",'-fps_mode','passthrough','-frames:v','2','-f','rawvideo','pipe:1']);
  assert.equal(small.length,50000);let changed=0;for(let i=0;i<25000;i++)if(Math.abs(small[i]-small[i+25000])>8)changed++;
  const changedPercent=100*changed/25000;assert(changedPercent>1,'visible_card_size_motion_required');
  // Brightening 8-bit channels must saturate, never wrap into cyan/magenta.
  const rgb=run('ffmpeg',['-v','error','-i',file,'-pix_fmt','rgb24','-f','rawvideo','pipe:1']);
  let cyan=0;for(let i=0;i<rgb.length;i+=3)if(rgb[i+2]>rgb[i]+20&&rgb[i+1]>rgb[i]+20)cyan++;
  assert(cyan/(rgb.length/3)<.0001,'warm_source_palette_without_channel_wrap');
  reports.push({section:candidate.section,format,bytes,width:320,height:200,seconds:Number(probe.format.duration),frames:80,unique_decoded_frames:uniqueFrames.size,fps:20,silent:true,codec:stream.codec_name,card_size_changed_pixels_percent:changedPercent,max_adjacent_luma_difference:maxStep,loop_seam_luma_difference:seam,max_source_luma_difference:maxSourceDifference,unexpected_cyan_pixels:cyan,sha256:hash(file)});
 }
}
const result={passed:true,status:enabled?'local_public_assets_enabled':'review_candidates_not_enabled',method:manifest.method,metrics_unit:'mean absolute luma difference on 0–255 scale; not a substitute for owner visual review',assets:reports};writeFileSync(resolve(out,'validation.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result));
