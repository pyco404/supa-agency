# Navigation card motion

Local frontend enhancement only. Existing imagery, card order, tilted framing,
branches, content, footer and private-route behavior are preserved. No API,
database, paid generation or execution capability is added.

## Asset mapping

`navigationMotion` in `reference-web/platform.js` contains optional `webm` and
`mp4` slots for Economy, Social, Trading and Factory. They are deliberately null
until final reviewed clips exist. No placeholder URL is requested. All other
sections keep their current static thumbnails.

For example, after adding the reviewed files, Economy can use:

```js
{ webm: '/public/assets/supa-economy-loop.webm',
  mp4: '/public/assets/supa-economy-loop.mp4' }
```

Use the corresponding section name for Social, Trading or Factory. The frontend
asset allowlists accept only these exact eight loop filenames; they do not admit
arbitrary uploads, paths, external URLs or private records. Static hosting adds
video MIME types and same-origin media CSP support. No new JS module needs to be
added to the Docker COPY list; the existing public-assets tree contains final
clips once separately supplied and reviewed.

Produce silent 3–5 second loops with matching first/last composition. Prefer
480×300 or another modest landscape resolution; target under 250 KiB WebM and
400 KiB MP4 per card. These are authoring budgets, not measured final asset sizes.
Economy: slow orbital rings/reflections. Social: delicate strand pulses. Trading:
slow optical movement/reflection drift. Factory: very small assembly movements.
Retain the near-black, ivory/olive art direction; avoid flashes, large camera
moves or suggestions that the imagery represents live execution.

## Playback behavior

The existing wheel creates a reusable poster/video layer. Only the selected,
visible card is eligible; media fills its clipped thumbnail slot with cover
cropping, leaving the label and card geometry intact. WebM is offered first, MP4
second. Video is decorative, muted, inline, looping and absent from keyboard focus.
The poster stays visible until actual playback starts.

Source attachment is lazy. Selection changes, hidden tabs, a hidden mobile drawer,
reduced motion, the existing Pause branches control or wheel destruction release
the decoder and sources. No new animation frame loop or media polling is added.
Navigation-motion pause is available through the existing terminal commands panel;
its accessible description now explains that it also pauses card media.

Unsupported sources, all-source HTTP errors, autoplay denial or an eight-second
startup deadline preserve the static image. Failed media is not retried every
render. Navigation remains usable with no video support or no configured clips.
Reduced-motion visitors do not attach/fetch video sources at all.

## Validation and release

`reference-web/tests/nav-motion.spec.mjs` uses four-second flat-color decoder
fixtures under `tests/fixtures`. These are tests, not final navigation artwork,
and are never copied into the public site. Browser regressions exercise actual
WebM playback/looping, MP4 fallback, failed downloads, autoplay rejection, selected
state, pause, hidden tabs/drawers, reduced motion, cleanup, responsive geometry
and protected routes. The public-landing unit test covers the narrow asset
allowlist, origin, method and staging-host boundaries. The local hosting test
verifies actual clip bytes/MIME, HEAD, rejected writes and private-route denial,
then removes its temporary public test clips.

Screenshots and local test results live under ignored `artifacts/nav-motion`.
Final cinematic assets still require visual/seam/performance review on target
browsers; fixture playback does not certify artwork quality or Safari behavior.
No push, release build, image publication or deployment is included here.
