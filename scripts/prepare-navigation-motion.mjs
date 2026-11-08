/** Offline, source-derived video candidates. Never enables production playback. */
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync,mkdirSync,writeFileSync,existsSync} from 'node:fs';
import {resolve} from 'node:path';
const root=resolve(import.meta.dirname,'..');
const refresh=process.argv.includes('--playback-refresh');
if(process.argv.slice(2).some(arg=>arg!=='--playback-refresh'))throw Error('unsupported_motion_argument');
const output=resolve(root,refresh?'artifacts/navigation-motion-playback-refresh':'reference-web/motion-candidates');
const work=resolve(root,refresh?'artifacts/navigation-motion-playback-refresh/work':'artifacts/navigation-motion-candidates');
mkdirSync(output,{recursive:true});mkdirSync(work,{recursive:true});
const sections=['factory','economy','social','trading'];
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
function run(args,timeout=300000){const r=spawnSync('ffmpeg',['-hide_banner','-loglevel','error','-nostdin',...args],{timeout,encoding:'utf8',maxBuffer:2000000});if(r.status!==0)throw Error('offline_video_stage_failed');}
const phase='sin(2*PI*T/4)';
const gaussian=(cx,cy,sx,sy)=>`exp(-pow((X-${cx})/${sx},2)-pow((Y-${cy})/${sy},2))`;
function recipe(id){
 let x='X',y='Y',light='1';
 if(id==='factory'){
  // Sub-pixel inverse warps around existing grippers; no new objects or surfaces.
  const left=gaussian(182,37,52,65),right=gaussian(390,115,70,45),cap=gaussian(238,90,57,38);
  x=`X+${phase}*(0.7*${left}-0.55*${right})`;
  y=`Y+${phase}*(0.65*${left}-0.45*${right}+0.35*${cap})`;
 }else if(id==='economy'){
  const ring=gaussian(240,130,205,155),angle=`0.007*${phase}*${ring}`;
  x=`X-(${angle})*(Y-130)`;y=`Y+(${angle})*(X-240)`;
  light=`1+0.025*pow(0.5+0.5*cos(atan2(Y-130,X-240)-2*PI*T/4),8)*${ring}`;
 }else if(id==='social'){
  // Traveling illumination follows only detail already present in the image.
  const edge='min(1,(abs(g(X+1,Y)-g(X-1,Y))+abs(g(X,Y+1)-g(X,Y-1)))/24)';
  light=`1+0.10*pow(0.5+0.5*cos(2*PI*(X/W+Y/H-T/4)),18)*${edge}`;
 }else if(id==='trading'){
  const lens=gaussian(240,171,66,66),angle=`0.005*${phase}*${lens}`;
  x=`X-(${angle})*(Y-171)`;y=`Y+(${angle})*(X-240)`;
  light=`1+0.055*pow(0.5+0.5*cos(atan2(Y-171,X-240)-2*PI*T/4),12)*${lens}`;
 }
 const channels=['r','g','b'].map(c=>`${c}='${c}(clip(${x},0,W-1),clip(${y},0,H-1))*(${light})'`).join(':');
 return `scale=480:480:flags=lanczos,crop=480:300:0:83,format=gbrp,geq=${channels}`;
}
const ffmpegVersion=spawnSync('ffmpeg',['-version'],{encoding:'utf8'}).stdout.split('\n')[0];
const records=[];
for(const id of sections){
 const source=resolve(root,`reference-web/public/assets/supa-${id}-960.webp`),master=resolve(work,`${id}-master.mkv`);
 const webm=resolve(output,`supa-${id}-loop.webm`),mp4=resolve(output,`supa-${id}-loop.mp4`);
 if(existsSync(webm)||existsSync(mp4))throw Error('existing_candidate_requires_separate_review');
 let filter=recipe(id);
 if(refresh){
  // Preserve source composition while making motion visible at 200-pixel card width.
  filter=filter.replaceAll('0.7*','3.5*').replaceAll('0.55*','2.8*').replaceAll('0.65*','3.2*').replaceAll('0.45*','2.2*').replaceAll('0.35*','2.0*')
   .replaceAll('0.007*','0.024*').replaceAll('1+0.025*','1+0.07*')
   .replaceAll('1+0.10*','1+0.24*').replaceAll('0.005*','0.036*').replaceAll('1+0.055*','1+0.18*');
 }
 writeFileSync(resolve(work,`${id}-filter.txt`),filter+'\n');
 // Four seconds at 20 fps. The periodic equations return to the initial state
 // at t=4; do not duplicate the first frame at the end and introduce a hold.
 run(['-loop','1','-framerate','20','-i',source,'-vf',filter,'-t','4','-an','-map_metadata','-1','-threads','2','-c:v','ffv1',master]);
 const sizing=refresh?['-vf','scale=320:200:flags=lanczos']:[];
 run(['-i',master,...sizing,'-an','-map_metadata','-1','-c:v','libvpx-vp9','-b:v','0','-crf','32','-row-mt','1','-threads','2','-pix_fmt','yuv420p',webm]);
 const mp4Encoding=refresh&&id==='economy'?['-qp','11','-preset','veryslow','-force_key_frames','0,3.95']:['-qp','12','-preset','slow'];
 run(['-i',master,...sizing,'-an','-map_metadata','-1','-c:v','libx264',...mp4Encoding,'-bf','0','-x264-params','ipratio=1:pbratio=1:mbtree=0:aq-mode=0','-threads','2','-pix_fmt','yuv420p','-movflags','+faststart',mp4]);
 records.push({section:id,mp4_encoding:mp4Encoding,source:`reference-web/public/assets/supa-${id}-960.webp`,source_sha256:hash(source),filter,webm_sha256:hash(webm),mp4_sha256:hash(mp4),width:refresh?320:480,height:refresh?200:300,fps:20,seconds:4,artwork_replaced:false,production_enabled:false});
 writeFileSync(resolve(output,'manifest.json'),JSON.stringify({status:refresh?'local_playback_refresh':'unapproved_review_candidates',ffmpeg_version:ffmpegVersion,encoding:{webm:'VP9 CRF32',mp4:refresh?'H.264, no B frames, uniform I/P quantization, faststart; exact settings per clip':'H.264 constant QP12, no B frames, uniform I/P quantization, faststart'},method:'source-derived 2D motion and lighting; no generative video or 3D reconstruction',candidates:records},null,2)+'\n');
 console.log(JSON.stringify({section:id,prepared:true}));
}
