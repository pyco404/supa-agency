/** Local decode and source-detail regressions for the large Factory artwork. */
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,statSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
const root=resolve(import.meta.dirname,'..'),out=resolve(root,'artifacts/factory-artwork-motion');mkdirSync(out,{recursive:true});
const manifest=JSON.parse(readFileSync(resolve(root,'reference-web/factory-artwork-motion.json')));
const navigation=JSON.parse(readFileSync(resolve(root,'reference-web/navigation-motion.json')));
const hash=data=>createHash('sha256').update(data).digest('hex');
assert.equal(hash(readFileSync(resolve(root,manifest.source))),manifest.source_sha256,'source_artwork_unchanged');
for(const item of navigation.candidates)for(const format of ['webm','mp4'])assert.equal(hash(readFileSync(resolve(root,`reference-web/public/assets/supa-${item.section}-loop.${format}`))),item[format+'_sha256'],'small_navigation_assets_unchanged');
function run(tool,args){const r=spawnSync(tool,args,{timeout:120000,maxBuffer:64000000});assert.equal(r.status,0,`${tool}_decode_failed: ${String(r.stderr).slice(-500)}`);return r.stdout;}
function difference(a,b){assert.equal(a.length,b.length);let sum=0;for(let i=0;i<a.length;i++)sum+=Math.abs(a[i]-b[i]);return sum/a.length;}
function psnr(a,b){assert.equal(a.length,b.length);let sum=0;for(let i=0;i<a.length;i++)sum+=(a[i]-b[i])**2;return 10*Math.log10(255**2/(sum/a.length));}
// Match the 604x340 desktop cover frame and its existing 46% vertical position.
const rendered='scale=604:378:flags=lanczos,crop=604:340:0:17,format=gray';
const source=resolve(root,manifest.source);
const reference=run('ffmpeg',['-v','error','-threads','2','-filter_threads','2','-loop','1','-framerate','20','-i',source,'-vf',manifest.filter+','+rendered,'-frames:v','1','-f','rawvideo','pipe:1']);
const reports=[];
for(const asset of manifest.assets){
 const file=resolve(root,'reference-web'+asset.path),format=asset.path.split('.').pop(),data=readFileSync(file);
 assert.equal(hash(data),asset.sha256);assert.equal(data.length,asset.bytes);assert(data.length<1500000);
 const probe=JSON.parse(run('ffprobe',['-v','error','-show_streams','-show_format','-of','json',file]));
 assert.equal(probe.streams.length,1,'silent_video_only');const stream=probe.streams[0];
 assert.equal(stream.width,960);assert.equal(stream.height,600);assert.equal(stream.avg_frame_rate,'20/1');assert.equal(stream.codec_name,format==='webm'?'vp9':'h264');assert(Math.abs(Number(probe.format.duration)-4)<.02);
 if(format==='mp4')assert(data.indexOf('moov')>0&&data.indexOf('moov')<data.indexOf('mdat'),'mp4_faststart');
 const frames=run('ffmpeg',['-v','error','-threads','2','-i',file,'-pix_fmt','gray','-f','rawvideo','pipe:1']);const size=960*600;assert.equal(frames.length,size*80);
 const unique=new Set();let maxStep=0;
 for(let frame=0;frame<80;frame++){const pixels=frames.subarray(frame*size,(frame+1)*size);unique.add(hash(pixels));if(frame)maxStep=Math.max(maxStep,difference(pixels,frames.subarray((frame-1)*size,frame*size)));}
 assert(unique.size>=75,'genuine_continuous_motion');
 const seam=difference(frames.subarray(0,size),frames.subarray(79*size));assert(maxStep>.01&&maxStep<2&&seam<2&&seam<=maxStep+.5,'seam_matches_normal_frame_motion');
 const render=file=>run('ffmpeg',['-v','error','-threads','2','-filter_threads','2','-i',file,'-vf',rendered,'-frames:v','1','-f','rawvideo','pipe:1']);
 const large=render(file),small=render(resolve(root,`reference-web/public/assets/supa-factory-loop.${format}`));
 const largePsnr=psnr(large,reference),smallPsnr=psnr(small,reference);assert(largePsnr>smallPsnr+3,'higher_resolution_preserves_more_source_detail');
 reports.push({format,bytes:statSync(file).size,width:960,height:600,seconds:Number(probe.format.duration),frames:80,unique_frames:unique.size,max_adjacent_luma_difference:maxStep,seam_luma_difference:seam,desktop_source_psnr_db:largePsnr,old_upscaled_psnr_db:smallPsnr,sha256:asset.sha256});
}
const report={passed:true,navigation_assets_unchanged:8,method:'Decoded luma and rendered-size source comparison; visual review is still required',assets:reports};writeFileSync(resolve(out,'validation.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
