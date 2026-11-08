# Original-art navigation motion: review candidates

Status: local review only. The active navigation mapping remains null/static.
No public file was replaced, source artwork changed, CI run dispatched or website
deployed. Original Factory, Economy, Social and Trading artwork is reused directly.

## What was produced

Eight silent videos live under `reference-web/motion-candidates`, outside the
public-assets tree and existing Docker COPY paths. Each is 480×300, four seconds,
20 fps, with WebM/VP9 first and MP4/H.264 faststart fallback. Existing cover
cropping and the 46% vertical focal point are retained.

- Factory: very small gripper/cap movements using bounded local pixel displacement.
- Economy: slow bounded ring movement and soft traveling reflections.
- Social: light pulses across detail already present in the connected strands.
- Trading: small lens movement and rotating reflection drift.

These are restrained **2D source-derived loops**, not AI video generation, a new
scene, a fully rigged robotic animation or a reconstructed 360-degree mechanism.
No unavailable back-facing surfaces or new objects are invented. The small motion
is intentional for roughly 200-pixel navigation cards, not full-screen playback.
No paid service, model or new dependency is used.

`manifest.json` binds each candidate to its original image SHA-256, exact motion
filter, codec recipe and final video checksums. `prepare-navigation-motion.mjs`
authors them offline with FFmpeg and refuses to overwrite existing candidates.
The mathematical phase repeats after four seconds without appending a duplicate
last frame. Fixed MP4 quantization/no B frames avoids the first-pass loop-boundary
compression difference. Original failed/review encodes remain ignored local evidence.

## Local validation

`node scripts/validate-navigation-motion.mjs` verifies source/video hashes, codecs,
single video stream with no audio, duration, 80 frames, dimensions, frame rate,
faststart and per-file budgets (250 KiB WebM / 400 KiB MP4). It decodes all frames
and checks nonstatic motion, bounded source differences and small adjacent/loop
luma changes. Numeric limits are unchanged after the first MP4 exports failed;
the encodes were corrected instead. Measured loop differences are retained in
ignored `artifacts/navigation-motion-candidates/validation.json`.

The WebM files are 21,901–30,054 bytes; MP4 files are 69,342–187,178 bytes.
These measurements describe the actual candidates, not hypothetical budgets.
Frame strips and desktop/tablet/mobile screenshots supplement browser playback;
owner visual approval remains necessary. Chrome mobile emulation is not physical
iOS/Safari certification.

The existing card/browser regression can explicitly load these candidates through
its local request interceptor with `SUPA_NAV_MOTION_CANDIDATES=1`. This does not
change the production mapping or copy files into public assets. It tests all four
clips, actual WebM and MP4 fallback, selected-only playback, reduced motion,
hidden tabs/drawers, pause, cleanup, unauthorized requests and private-route denial.
Performance sampling begins after carousel handover and records both startup and
a two-second sustained frame window; it retains the drop threshold and requires
at least 20 observed frames rather than accepting an unavailable sample. All 30
actual-asset and editorial browser checks passed across desktop, tablet and mobile.
The 12 two-second samples recorded 492 decoded frames and zero steady-state drops;
startup recorded 19 drops across those samples and is reported separately rather
than hidden. These shared-host measurements do not certify physical devices.

## Review and release boundaries

Open `reference-web/motion-candidates/review.html` for native video controls and
original posters. The local loopback-only review server, when running, exposes
only this page, these eight clips and the four public source images. It has no
API, database, credential or application-record access. Nothing autoplays there.

1. Owner reviews and approves the four visual candidates (or requests adjustments).
2. Only after approval, copy the exact eight checksummed clips to the existing
   approved public filenames and fill the four existing `navigationMotion` slots.
   Keep all other sections static and preserve reduced-motion behavior.
3. Pin one frontend validation to the resulting exact source; test actual packaged
   clips, browser/authentication boundaries and full unsuppressed vulnerability
   and embedded-secret gates. Avoid building the unchanged current static version
   just to repeat work before the candidate review.
4. Private publication of the exact validated archive and one frontend deployment
   require their separate approvals. No Rust API/database changes are needed for
   navigation motion. Supervised API validation remains a separate pending gate.

Static images remain active until step 2 is explicitly authorized. Agent execution,
registration, inference, trading, signing, launches and X publishing remain disabled.
