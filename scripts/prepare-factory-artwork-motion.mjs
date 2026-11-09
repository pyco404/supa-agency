/** Offline high-resolution export from the approved Factory source and motion. */
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync,statSync} from 'node:fs';
import {resolve} from 'node:path';
import assert from 'node:assert/strict';
const root=resolve(import.meta.dirname,'..');
const output=resolve(root,'artifacts/factory-artwork-motion');mkdirSync(output,{recursive:true});
const approved=JSON.parse(readFileSync(resolve(root,'reference-web/navigation-motion.json'))).candidates.find(c=>c.section==='factory');
const source=resolve(root,approved.source),hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
assert.equal(hash(source),approved.source_sha256,'original_artwork_integrity');
assert.equal(approved.width,320);assert.equal(approved.height,200);assert.equal(approved.seconds,4);
const prefix='scale=480:480:flags=lanczos,crop=480:300:0:83,format=gbrp,geq=';
assert(approved.filter.startsWith(prefix),'approved_motion_coordinate_space');
// Evaluate the original 480x300 displacement field in normalized coordinates,
// then sample the original 960px artwork. Never upscale the small encoded clip.
const channels=approved.filter.slice(prefix.length).split(':').map((expression,index)=>{
 const channel=['r','g','b'][index],start=`${channel}='${channel}(`,end=")*(1)'";
 assert(expression.startsWith(start)&&expression.endsWith(end),'factory_motion_schema');
 const coordinates=expression.slice(start.length,-end.length);let depth=0,separator=-1;
 for(let i=0;i<coordinates.length;i++){if(coordinates[i]==='(')depth++;else if(coordinates[i]===')')depth--;else if(coordinates[i]===','&&depth===0){assert.equal(separator,-1);separator=i;}}
 assert(separator>0&&depth===0,'coordinate_pair');
 const normalize=value=>value.replace(/\b[XYWH]\b/g,symbol=>`(${symbol}/2)`);
 const x=normalize(coordinates.slice(0,separator)),y=normalize(coordinates.slice(separator+1));
 return `${channel}='${channel}(2*(${x}),2*(${y}))*(1)'`;
});
assert.equal(channels.length,3);
const filter='scale=960:960:flags=lanczos,crop=960:600:0:166,format=gbrp,geq='+channels.join(':');
function run(tool,args,maxBuffer=2000000){const result=spawnSync(tool,args,{timeout:600000,maxBuffer});if(result.status!==0)throw Error(`${tool}_offline_export_failed: ${String(result.stderr).slice(-2000)}`);return result.stdout;}
const work=mkdtempSync('/dev/shm/supa-factory-artwork-');
try{
 const master=resolve(work,'factory-master.mkv');
 run('ffmpeg',['-v','error','-nostdin','-loop','1','-framerate','20','-i',source,'-vf',filter,'-t','4','-an','-map_metadata','-1','-threads','2','-filter_threads','2','-c:v','ffv1',master]);
 const assets=[];
 for(const format of ['webm','mp4']){
  const file=resolve(output,`supa-factory-artwork-loop.${format}`);
  const encoding=format==='webm'?['-c:v','libvpx-vp9','-b:v','0','-crf','26','-row-mt','1']:['-c:v','libx264','-qp','18','-preset','slow','-force_key_frames','0,3.95','-bf','0','-x264-params','ipratio=1:pbratio=1:mbtree=0:aq-mode=0','-movflags','+faststart'];
  run('ffmpeg',['-v','error','-nostdin','-i',master,'-an','-map_metadata','-1',...encoding,'-threads','2','-pix_fmt','yuv420p','-y',file]);
  const probe=JSON.parse(run('ffprobe',['-v','error','-show_streams','-show_format','-of','json',file]));
  assert.equal(probe.streams.length,1);assert.equal(probe.streams[0].width,960);assert.equal(probe.streams[0].height,600);assert.equal(probe.streams[0].avg_frame_rate,'20/1');assert(Math.abs(Number(probe.format.duration)-4)<.02);assert(statSync(file).size<1500000,'large_artwork_asset_budget');
  assets.push({path:`/public/assets/supa-factory-artwork-loop.${format}`,sha256:hash(file),bytes:statSync(file).size,codec:probe.streams[0].codec_name});
 }
 const manifest={source:approved.source,source_sha256:approved.source_sha256,approved_navigation_filter:approved.filter,filter,width:960,height:600,fps:20,seconds:4,method:'Original source pixels with the approved displacement field scaled to twice its coordinate resolution; unchanged camera, crop and periodic motion',assets};
 writeFileSync(resolve(output,'manifest.json'),JSON.stringify(manifest,null,2)+'\n');console.log(JSON.stringify(manifest.assets));
}finally{rmSync(work,{recursive:true,force:true});}
