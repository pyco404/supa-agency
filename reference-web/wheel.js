/** Independently authored project wheel; no reference website source is vendored. */
// One decorative decoder at a time. No source is attached until a visible,
// selected card is allowed to move; the poster remains underneath throughout.
export function createCardMedia(entry) {
  const element = document.createElement("span");
  element.className = "wheel-media";
  element.setAttribute("aria-hidden", "true");
  const image = document.createElement("img");
  image.className = "wheel-thumb";
  image.src = entry.thumb;
  image.alt = "";
  image.draggable = false;
  image.decoding = "async";
  image.loading = "lazy";
  image.width = 320;
  image.height = 200;
  image.referrerPolicy = "no-referrer";
  element.append(image);
  // Only reviewed, same-origin public navigation assets belong in this layer.
  const sources = ["webm", "mp4"].flatMap(format => {
    const path = entry.media?.[format];
    return typeof path === "string" && /^\/public\/assets\/supa-(economy|social|trading|factory)-loop\.(webm|mp4)$/.test(path)
      && path.endsWith("." + format) ? [{ path, type: "video/" + format }] : [];
  });
  let video = null, startupTimer = null, failed = false, disposed = false;
  function release() {
    clearTimeout(startupTimer);
    startupTimer = null;
    if (!video) return;
    const previous = video;
    video = null;
    delete element.dataset.playing;
    previous.pause();
    previous.removeAttribute("src");
    previous.replaceChildren();
    previous.load();
    previous.remove();
  }
  function setActive(active) {
    if (!active || disposed) { release(); return; }
    if (video || failed || !sources.length) return;
    const current = document.createElement("video");
    video = current;
    current.className = "wheel-motion";
    current.muted = true;
    current.defaultMuted = true;
    current.autoplay = true;
    current.loop = true;
    current.playsInline = true;
    current.preload = "none";
    current.poster = entry.thumb;
    current.disablePictureInPicture = true;
    current.setAttribute("aria-hidden", "true");
    current.tabIndex = -1;
    const fallback = () => {
      if (video !== current) return;
      failed = true;
      release();
    };
    current.addEventListener("error", fallback);
    current.addEventListener("playing", () => {
      if (video === current) { clearTimeout(startupTimer); startupTimer = null; element.dataset.playing = "true"; }
    });
    let sourceFailures = 0;
    for (const source of sources) {
      const node = document.createElement("source");
      node.src = source.path;
      node.type = source.type;
      node.addEventListener("error", () => { if (++sourceFailures === sources.length) fallback(); });
      current.append(node);
    }
    element.append(current);
    startupTimer = setTimeout(fallback, 8000);
    current.play()?.catch(fallback); // Autoplay denial also leaves a usable poster.
  }
  return { element, setActive, destroy() { disposed = true; release(); } };
}

export function createWheel({ host, canvas, items, onSelect, reducedMotion = false }) {
  if (!(host instanceof HTMLElement) || !(canvas instanceof HTMLCanvasElement)) {
    throw new TypeError("A wheel host and canvas are required.");
  }
  const entries = Array.isArray(items) ? items : [];
  const count = entries.length;
  const motion = typeof reducedMotion === "boolean"
    ? { matches: reducedMotion }
    : reducedMotion;
  const context = canvas.getContext("2d");
  const previousRole = host.getAttribute("role");
  host.setAttribute("role", "listbox");
  if (!host.hasAttribute("aria-label")) host.setAttribute("aria-label", "Supa Agency sections");
  host.style.touchAction = "pan-x pinch-zoom";
  canvas.setAttribute("aria-hidden", "true");
  canvas.style.pointerEvents = "none";
  const list = document.createElement("ul");
  list.className = "wheel-list";
  list.setAttribute("role", "presentation");
  const cardMedia = [];
  const buttons = entries.map((entry, index) => {
    const row = document.createElement("li");
    row.setAttribute("role", "presentation");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "wheel-item";
    button.dataset.index = String(index);
    button.setAttribute("role", "option");
    button.setAttribute("aria-label", entry.label);
    if (entry.thumb) {
      const media = createCardMedia(entry);
      cardMedia[index] = media;
      button.append(media.element);
    } else if(entry.preview) {
      const thumbnail=document.createElementNS('http://www.w3.org/2000/svg','svg');
      thumbnail.setAttribute('class','wheel-thumb wheel-preview');thumbnail.setAttribute('viewBox','0 0 200 80');thumbnail.setAttribute('aria-hidden','true');
      const use=document.createElementNS('http://www.w3.org/2000/svg','use');use.setAttribute('href',`/public/assets/navigation-previews.svg#${entry.preview}`);thumbnail.append(use);button.append(thumbnail);
    } else {
      const mark = document.createElement("span");
      mark.className = "wheel-thumb wheel-thumb-text";
      mark.setAttribute("aria-hidden", "true");
      mark.textContent = entry.mark ?? ">_";
      button.append(mark);
    }
    const label = document.createElement("span");
    label.className = "wheel-label";
    const name=document.createElement('span');name.className='wheel-name';name.textContent=entry.label.split(' · ')[0];label.append(name);
    if(entry.label.endsWith(' · planned')){const state=document.createElement('span');state.className='wheel-planned';state.textContent='planned';label.append(state);}
    button.append(label);
    row.append(button);
    list.append(row);
    return button;
  });
  host.append(list);

  let selected = 0;
  let position = 0;
  let target = 0;
  let width = 230;
  let height = 680;
  let frame = 0;
  let destroyed = false;
  let entrance = null;
  let entranceRequested = false;
  host.dataset.carouselState = "settled";
  let branchVisible=false,branchPaused=false,branchTime=0,branchPrevious=null,lastBranchPaint=0,needsRender=true;
  const branchControl=document.querySelector('.branch-motion');
  let wheelDelta = 0;
  let lastWheel = 0;
  let pointer = null;
  let suppressClick = false;
  const modulo = (value) => count ? ((value % count) + count) % count : 0;
  const wrappedDistance = (value) => count
    ? ((value + count / 2) % count + count) % count - count / 2
    : 0;
  const random = (seed) => {
    const value = Math.sin(seed * 127.1 + 311.7) * 43758.5453123;
    return value - Math.floor(value);
  };
  const dimensions = () => {
    const cardWidth = Math.min(200, Math.max(104, width * 0.88));
    const cardHeight = cardWidth * 0.64;
    // Leave room for both the enlarged selection and its tilted neighbours.
    const gap = Math.max(cardHeight * 0.88 + 6, Math.min(132, height / 6.5));
    return { cardWidth, cardHeight, gap };
  };
  // Shared elliptical path keeps the card and its branch attachment together.
  const placement = (index) => {
    const { cardWidth, cardHeight, gap } = dimensions();
    const distance = wrappedDistance(index - position), orbit = distance * .17;
    return {
      distance, cardWidth, cardHeight,
      x: 3 + (1 - Math.cos(orbit)) * 2 * Math.min(6, width / 38) / (.17 ** 2),
      y: height / 2 + Math.sin(orbit) * gap / .17,
      scale: .72 + Math.max(0, 1 - Math.abs(distance) * 2) * .28,
      angle: -orbit / 2,
    };
  };
  function cancelEntrance() {
    entranceRequested = false;
    if (entrance) position = target;
    entrance = null;
    host.dataset.carouselState = "settled";
  }
  function reveal() {
    if (!count || destroyed) return;
    cancelEntrance();
    entranceRequested = !motion?.matches;
    schedule();
  }
  const spine = (y) => {
    const normalized = (y - height / 2) / Math.max(height / 2, 1);
    return -16 + normalized * normalized * Math.min(90, width * 0.4)+Math.sin(y/Math.max(height,1)*13+position*.35)*4;
  };

  function drawBranch() {
    if (!context) return;
    const gutter = 110;
    const phase=motion?.matches?0:branchTime/1000;canvas.dataset.motionPhase=String(phase);
    const ink = getComputedStyle(host).color || "#ebe6db";
    context.clearRect(0, 0, width + gutter, height);
    context.save();
    context.translate(gutter + (matchMedia('(max-width:700px)').matches ? (width-dimensions().cardWidth)/2 : 0), 0);
    context.strokeStyle = ink;
    context.lineCap = "round";
    context.lineWidth = 1.45;
    context.globalAlpha = 0.72;
    context.beginPath();
    for (let y = 0; y <= height; y += 3) {
      const x = spine(y) + Math.sin(y * 0.012 + position * 0.5) * .6;
      if (y === 0) context.moveTo(x, y); else context.lineTo(x, y);
    }
    context.stroke();
    // Fibers lash in uneven gusts; spine/card attachments remain stable.
    for(let stem=0;stem<420;stem++){
      const seed=stem+Math.floor(position)*19,cluster=stem%5;
      const center=[.07,.29,.55,.76,.96][cluster]*height;
      const y=Math.max(0,Math.min(height,center+(random(seed+1)-.5)*height*.11));
      const x=spine(y),direction=random(seed+4)>.87?1:-1;
      const energy=motion?.matches?0:1;
      const gust=(.5+.5*Math.sin(phase*1.7+cluster*1.1))**3;
      const wave=phase*5.8-y*.026+stem*.13;
      const sway=energy*(Math.sin(wave)*(18+gust*22)+Math.sin(phase*11.7+stem*.37)*9);
      const lift=energy*(Math.cos(wave*.83)*(14+gust*16)+Math.sin(phase*8.3+stem*.21)*7);
      const bend=energy*Math.sin(wave-1.1)*(24+gust*30);
      const length=(12+random(seed+3)**.65*(65+random(cluster+21)*45))*(1+energy*Math.sin(wave*.7)*.16),tipX=x+direction*length+sway,tipY=y-(random(seed+5)-.2)*90+lift;
      context.globalAlpha=.18+random(seed+6)*.45;context.lineWidth=.45;
      context.beginPath();context.moveTo(x,y);context.bezierCurveTo(x+direction*length*.15+bend*.35,y-5-lift*.4,x+direction*length*.5+bend,tipY+20-lift*.5,tipX,tipY);context.stroke();
      if(stem%3===0){context.fillStyle=ink;context.globalAlpha=.65;context.fillRect(tipX,tipY,1.7,1.7);}
    }
    buttons.forEach((button,index)=>{
      const { y, x: cardX, cardWidth, scale, angle } = placement(index);
      if(y<0||y>height)return;
      const endX=cardX+cardWidth/2-Math.cos(angle)*cardWidth*scale/2,endY=y-Math.sin(angle)*cardWidth*scale/2;
      const chosen=index===selected;
      context.strokeStyle=chosen?getComputedStyle(host).getPropertyValue('--green').trim():ink;
      context.globalAlpha=chosen?.8:.18;context.lineWidth=chosen?.9:.6;
      context.beginPath();context.moveTo(spine(y),y);context.bezierCurveTo(spine(y)+12,y-12,endX-8,endY-8,endX,endY);context.stroke();
      if(chosen){context.globalAlpha=.08;context.lineWidth=3;context.stroke();}
    });
    context.restore();
  }

  function render() {
    buttons.forEach((button, index) => {
      const { distance, cardWidth, cardHeight, scale, angle, x: orbitX, y: centerY } = placement(index);
      const y = centerY - cardHeight / 2;
      const x = orbitX + (matchMedia('(max-width:700px)').matches ? (width-cardWidth)/2 : 0);
      button.style.position = "absolute";
      button.style.left = "0";
      button.style.top = "0";
      button.style.width = `${cardWidth}px`;
      button.style.height = `${cardHeight}px`;
      button.style.transform = `translate(${x.toFixed(2)}px, ${y.toFixed(2)}px) rotate(${angle}rad) scale(${scale.toFixed(3)})`;
      button.style.setProperty("--shade", String(Math.min(0.72, Math.abs(distance) * 0.08 + (Math.abs(distance) > 0.2 ? 0.26 : 0))));
      button.style.zIndex = String(100 - Math.round(Math.abs(distance) * 10));
      button.style.visibility = y < -cardHeight || y > height ? "hidden" : "visible";
      button.setAttribute("aria-selected", String(index === selected));
      button.tabIndex = index === selected ? 0 : -1;
      button.classList.toggle("is-selected", index === selected);
    });
    syncCardMedia();
    drawBranch();
  }

  function animate(time) {
    frame=0;if(destroyed||document.hidden)return;
    // Match the rendered preference even before its change event is delivered.
    if(branchControl&&branchControl.disabled!==!!motion?.matches)updateBranchControl();
    const flowing=branchVisible&&!branchPaused&&!motion?.matches;
    canvas.dataset.motionState=motion?.matches?'reduced':branchPaused?'paused':branchVisible?'running':'offscreen';
    if(flowing&&branchPrevious!==null)branchTime+=Math.max(0,Math.min(100,time-branchPrevious));
    branchPrevious=flowing?time:null;
    if (motion?.matches) cancelEntrance();
    if (entranceRequested && branchVisible) {
      entranceRequested = false;
      entrance = { start: time, from: target - count };
      host.dataset.carouselState = "entering";
    }
    const difference=target-position;
    const navigating=Math.abs(difference)>0;
    if (entrance) {
      const progress = Math.min(1, (time - entrance.start) / 780);
      position = entrance.from + (target - entrance.from) * (1 - (1 - progress) ** 3);
      if (progress === 1) { entrance = null; host.dataset.carouselState = "settled"; }
    } else position=motion?.matches||Math.abs(difference)<.002?target:position+difference*.18;
    if(needsRender||navigating||entrance){render();needsRender=false;lastBranchPaint=time;}
    else if(flowing&&time-lastBranchPaint>=33){drawBranch();lastBranchPaint=time;}
    if(entrance||position!==target||flowing)frame=requestAnimationFrame(animate);
  }

  function schedule() {
    needsRender=true;
    if(!frame&&!destroyed&&!document.hidden)frame=requestAnimationFrame(animate);
  }
  function updateBranchControl(){
    if(!branchControl)return;
    branchControl.disabled=!!motion?.matches;
    branchControl.textContent=motion?.matches?'Branch motion reduced':branchPaused?'Play branches':'Pause branches';
    branchControl.setAttribute('aria-pressed',String(!branchPaused&&!motion?.matches));
  }
  function syncCardMedia() {
    cardMedia.forEach((media, index) => media?.setActive(index === selected && branchVisible && buttons[index].style.visibility !== 'hidden'
      && !branchPaused && !motion?.matches && !document.hidden));
  }
  branchControl?.setAttribute('aria-description', 'Also pauses animated navigation card visuals.');
  function toggleBranches(){branchPaused=!branchPaused;syncCardMedia();branchPrevious=null;updateBranchControl();schedule();}
  function motionChange(){syncCardMedia();branchPrevious=null;updateBranchControl();schedule();}

  function select(index) {
    if (!count || !Number.isFinite(index) || destroyed) return;
    cancelEntrance();
    const next = modulo(Math.round(index));
    const changed = next !== selected;
    target += wrappedDistance(next - modulo(target));
    selected = next;
    // Keep accessibility state in sync before the visual interpolation finishes.
    buttons.forEach((button, itemIndex) => {
      button.setAttribute("aria-selected", String(itemIndex === selected));
      button.tabIndex = itemIndex === selected ? 0 : -1;
    });
    syncCardMedia();
    schedule();
    if (changed) onSelect?.(selected, entries[selected]);
  }

  function step(delta) {
    if (Number.isFinite(delta)) select(selected + Math.sign(delta));
  }

  function onClick(event) {
    if (suppressClick) {
      suppressClick = false;
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    const button = event.target.closest(".wheel-item");
    if (button && list.contains(button)) select(Number(button.dataset.index));
  }

  function onKey(event) {
    const directions = { ArrowDown: 1, j: 1, ArrowUp: -1, k: -1 };
    if (!(event.key in directions) && event.key !== "Home" && event.key !== "End") return;
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Home") select(0);
    else if (event.key === "End") select(count - 1);
    else step(directions[event.key]);
    buttons[selected]?.focus({ preventScroll: true });
  }

  function onWheel(event) {
    if (event.ctrlKey || !count) return;
    event.preventDefault();
    const now = performance.now();
    if (now - lastWheel > 180) wheelDelta = 0;
    lastWheel = now;
    wheelDelta += event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? height : 1);
    if (Math.abs(wheelDelta) >= 45) {
      step(wheelDelta);
      wheelDelta = 0;
    }
  }

  function onPointerDown(event) {
    if (!event.isPrimary || event.button !== 0) return;
    cancelEntrance();
    pointer = { id: event.pointerId, y: event.clientY, position, dragged: false };
    suppressClick = false;
  }

  function onPointerMove(event) {
    if (!pointer || pointer.id !== event.pointerId) return;
    const distance = pointer.y - event.clientY;
    if (!pointer.dragged && Math.abs(distance) < 7) return;
    if (!pointer.dragged) host.setPointerCapture(event.pointerId);
    pointer.dragged = true;
    event.preventDefault();
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    position = pointer.position + distance / dimensions().gap;
    render();
  }

  function onPointerEnd(event) {
    if (!pointer || pointer.id !== event.pointerId) return;
    const dragged = pointer.dragged;
    pointer = null;
    if (host.hasPointerCapture(event.pointerId)) host.releasePointerCapture(event.pointerId);
    if (dragged) {
      suppressClick = true;
      const next = modulo(Math.round(position));
      target = Math.round(position);
      const changed = selected !== next;
      selected = next;
      schedule();
      if (changed) onSelect?.(selected, entries[selected]);
    }
  }

  function resize() {
    const bounds = host.getBoundingClientRect();
    width = Math.max(1, bounds.width);
    height = Math.max(1, bounds.height);
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.ceil((width + 110) * ratio);
    canvas.height = Math.ceil(height * ratio);
    canvas.style.width = `${width + 110}px`;
    canvas.style.height = `${height}px`;
    canvas.style.left = "-110px";
    context?.setTransform(ratio, 0, 0, ratio, 0, 0);
    render();
  }

  function visibility() {
    branchPrevious=null;
    syncCardMedia();
    if (document.hidden && frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    } else if (!document.hidden) schedule();
  }

  const branchObserver=new IntersectionObserver(entries=>{branchVisible=entries[0].isIntersecting;syncCardMedia();branchPrevious=null;schedule();});branchObserver.observe(host);
  branchControl?.addEventListener('click',toggleBranches);updateBranchControl();
  const observer = new ResizeObserver(resize);
  observer.observe(host);
  host.addEventListener("click", onClick, true);
  host.addEventListener("keydown", onKey);
  host.addEventListener("wheel", onWheel, { passive: false });
  host.addEventListener("pointerdown", onPointerDown);
  host.addEventListener("pointermove", onPointerMove, { passive: false });
  host.addEventListener("pointerup", onPointerEnd);
  host.addEventListener("pointercancel", onPointerEnd);
  document.addEventListener("visibilitychange", visibility);
  motion?.addEventListener?.("change", motionChange);
  resize();

  return {
    reveal,
    select,
    step,
    get index() { return selected; },
    destroy() {
      destroyed = true;
      cardMedia.forEach(media => media?.destroy());
      if (frame) cancelAnimationFrame(frame);
      if (pointer && host.hasPointerCapture(pointer.id)) host.releasePointerCapture(pointer.id);
      pointer = null;
      observer.disconnect();branchObserver.disconnect();branchControl?.removeEventListener('click',toggleBranches);
      host.removeEventListener("click", onClick, true);
      host.removeEventListener("keydown", onKey);
      host.removeEventListener("wheel", onWheel);
      host.removeEventListener("pointerdown", onPointerDown);
      host.removeEventListener("pointermove", onPointerMove);
      host.removeEventListener("pointerup", onPointerEnd);
      host.removeEventListener("pointercancel", onPointerEnd);
      document.removeEventListener("visibilitychange", visibility);
      motion?.removeEventListener?.("change", motionChange);
      list.remove();
      if (previousRole === null) host.removeAttribute("role");
      else host.setAttribute("role", previousRole);
    },
  };
}
