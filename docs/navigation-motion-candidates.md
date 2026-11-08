# Original-art navigation motion: review candidates

Status: enabled in the local frontend following the request to use the four loops.
The eight exact checksummed candidates are copied to `reference-web/public/assets`;
`navigationMotion` in `platform.js` maps WebM first and MP4 second. Original stills
remain unchanged. No push, CI build, publication, Railway change or deployment is
part of this implementation. The candidate manifest retains its historical review
status; the validator separately checks the active mapping and public-copy hashes.

Only the selected, visible card loads/plays a video. Inactive cards, hidden tabs,
closed mobile navigation and off-screen navigation release their decoders. The
existing Pause branches control also pauses card media. Reduced motion loads no
video; loading errors, autoplay rejection or the eight-second startup timeout keep
the original still. Video controls and remote playback are disabled. Captions,
rectangular cover cropping, navigation and authentication are unchanged.

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
These checks complement visual review. Chrome mobile emulation is not physical
iOS/Safari certification.

The existing card/browser regression can explicitly load these candidates through
its local request interceptor with `SUPA_NAV_MOTION_CANDIDATES=1`. The default regression now reads the actual public copies and active mapping;
the optional interceptor remains available to review historical candidate files. It tests all four
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

1. Local assets are now connected; inspect the four cards in the existing preview.
2. After approval, synchronize the exact application source and pin frontend CI to
   that revision. Validate packaged clips, browser/authentication boundaries and
   full unsuppressed vulnerability and embedded-secret gates.
3. Private publication of the exact validated archive and one frontend deployment
   need separate approval. No Rust API or database changes are needed.

Agent execution, registration, inference, trading, signing, launches and X publishing
remain unchanged. Browser tests cover real WebM and MP4 decoding, fallback images,
reduced motion, selected-only playback, hidden/off-screen release, loop wrapping,
rectangular geometry, overflow and zero private API/mutation requests at desktop,
tablet and mobile sizes. Frame-level checks cover all eight loops and their seams.
