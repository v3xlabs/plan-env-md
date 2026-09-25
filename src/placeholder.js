// Runs inside a rendered revision, after it settles, and reports what is painted
// in the first viewport as plain data. The document's own scripts share this
// realm and can tamper with anything here, so the worker treats the result as
// untrusted input and builds the SVG itself.
async () => {
  const VIEW_W = innerWidth;
  const VIEW_H = innerHeight;
  const MAX_BOXES = 1500;
  const CELL = 48;
  const boxes = [];

  const COLOR = /rgba?\([^)]*\)|color\([^)]*\)|oklch\([^)]*\)|oklab\([^)]*\)|lab\([^)]*\)|lch\([^)]*\)|hsla?\([^)]*\)/g;
  // Chromium reports oklch, lab and color() as authored, so the browser converts them to sRGB.
  const swatch = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
  const known = new Map();
  const parse = (value) => {
    const direct = value.match(/^rgba?\(([^)]+)\)$/);
    if (direct) {
      const parts = direct[1].split(/[ ,/]+/).filter(Boolean).map(Number);
      return { r: parts[0], g: parts[1], b: parts[2], a: parts.length > 3 ? parts[3] : 1 };
    }
    const found = value.match(COLOR);
    if (!found) return null;
    const key = found[0];
    if (!known.has(key)) {
      swatch.clearRect(0, 0, 1, 1);
      swatch.fillStyle = "rgba(0, 0, 0, 0)";
      swatch.fillStyle = key;
      swatch.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = swatch.getImageData(0, 0, 1, 1).data;
      known.set(key, a === 0 ? { r: 0, g: 0, b: 0, a: 0 } : { r, g, b, a: a / 255 });
    }
    return known.get(key);
  };
  const splitLayers = (value) => {
    const layers = [];
    let depth = 0;
    let start = 0;
    for (let i = 0; i < value.length; i++) {
      if (value[i] === "(") depth++;
      else if (value[i] === ")") depth--;
      else if (value[i] === "," && depth === 0) {
        layers.push(value.slice(start, i).trim());
        start = i + 1;
      }
    }
    layers.push(value.slice(start).trim());
    return layers;
  };
  const over = (top, under) => {
    if (!under) return top;
    const a = top.a + under.a * (1 - top.a);
    const mix = (k) => (top[k] * top.a + under[k] * under.a * (1 - top.a)) / (a || 1);
    return { r: mix("r"), g: mix("g"), b: mix("b"), a };
  };
  const ANGLES = { "to top": 0, "to right": 90, "to bottom": 180, "to left": 270 };
  // One plain linear gradient keeps its direction. Anything else, stacked layers,
  // stripes or radial, folds to its average colour over the background colour.
  const background = (cs) => {
    const base = parse(cs.backgroundColor);
    const layers = cs.backgroundImage === "none" ? [] : splitLayers(cs.backgroundImage).filter((l) => l.includes("gradient("));
    const stopsOf = (layer) => (layer.match(COLOR) ?? []).map(parse).filter(Boolean);
    if (layers.length === 1 && layers[0].startsWith("linear-gradient(")) {
      const stops = stopsOf(layers[0]);
      const head = layers[0].slice("linear-gradient(".length).split(",")[0].trim();
      const angle = head.endsWith("deg") ? parseFloat(head) : ANGLES[head] ?? 180;
      if (stops.length >= 2) {
        return {
          gradient: { angle, stops: stops.map((c) => ({ color: hex(c), alpha: +c.a.toFixed(2) })) },
          base: base && base.a > 0.04 ? { color: hex(base), alpha: +base.a.toFixed(2) } : null,
        };
      }
    }
    let result = base && base.a > 0.04 ? base : null;
    for (const layer of layers.toReversed()) {
      const stops = stopsOf(layer);
      if (!stops.length) continue;
      const a = stops.reduce((s, c) => s + c.a, 0) / stops.length;
      const avg = (k) => stops.reduce((s, c) => s + c[k] * c.a, 0) / (a * stops.length || 1);
      result = over({ r: avg("r"), g: avg("g"), b: avg("b"), a }, result);
    }
    return { fill: result && result.a > 0.04 ? { color: hex(result), alpha: +result.a.toFixed(2) } : null };
  };
  const hex = (c) => `#${[c.r, c.g, c.b].map((n) => Math.round(n).toString(16).padStart(2, "0")).join("")}`;
  const paint = (value) => {
    const c = parse(value);
    return c && c.a > 0.04 ? { color: hex(c), alpha: +c.a.toFixed(2) } : null;
  };
  const inView = (r) => r.width > 0.5 && r.height > 0.5 && r.bottom > 0 && r.top < VIEW_H && r.right > 0 && r.left < VIEW_W;
  const clip = (r) => {
    const x = Math.max(0, r.left);
    const y = Math.max(0, r.top);
    return {
      x: +x.toFixed(1),
      y: +y.toFixed(1),
      w: +(Math.min(VIEW_W, r.right) - x).toFixed(1),
      h: +(Math.min(VIEW_H, r.bottom) - y).toFixed(1),
    };
  };
  const push = (box) => {
    if (boxes.length < MAX_BOXES) boxes.push(box);
  };

  const rootStyle = getComputedStyle(document.documentElement);
  const bodyStyle = getComputedStyle(document.body);
  const canvas = background(bodyStyle).fill ?? background(rootStyle).fill
    ?? { color: matchMedia("(prefers-color-scheme: dark)").matches && rootStyle.colorScheme.includes("dark") ? "#121212" : "#ffffff", alpha: 1 };

  const textKind = (el) => {
    if (el.closest("pre, code, kbd, samp")) return "code";
    if (el.closest("h1, h2, h3")) return "heading";
    if (el.closest("a")) return "link";
    return "text";
  };

  // Block and box-drawing characters paint shapes, not words, so a text UI keeps its frame.
  const glyphClass = (ch) => {
    const c = ch.codePointAt(0);
    if (c === 0x2591) return "shade1";
    if (c === 0x2592) return "shade2";
    if (c === 0x2593) return "shade3";
    if (c >= 0x2580 && c <= 0x259f) return "solid";
    if (c >= 0x2500 && c <= 0x257f) {
      if ("─━═┄┅┈┉╌╍".includes(ch)) return "hline";
      if ("│┃║┆┇┊┋╎╏".includes(ch)) return "vline";
      return "cross";
    }
    return "text";
  };

  // A few average colours per image, so a picture keeps its own colours and rough layout.
  const sample = async (el, tag, rect, cs) => {
    const cols = Math.max(1, Math.min(8, Math.round(rect.width / CELL)));
    const rows = Math.max(1, Math.min(6, Math.round(rect.height / CELL)));
    const sub = 8;
    let source = el;
    try {
      if (tag === "picture") source = el.querySelector("img");
      if (tag === "img" || tag === "picture") {
        if (!source || !source.complete || !source.naturalWidth) return null;
      } else if (tag === "svg") {
        const copy = el.cloneNode(true);
        copy.setAttribute("xmlns", "http://www.w3.org/2000/svg");
        copy.setAttribute("width", String(rect.width));
        copy.setAttribute("height", String(rect.height));
        copy.style.color = cs.color;
        // the copy leaves the page's stylesheets behind, so each shape carries its computed paint
        const originals = el.querySelectorAll("*");
        const copies = copy.querySelectorAll("*");
        originals.forEach((node, i) => {
          const style = getComputedStyle(node);
          copies[i].setAttribute("style", `fill:${style.fill};stroke:${style.stroke};stroke-width:${style.strokeWidth};opacity:${style.opacity};fill-opacity:${style.fillOpacity};display:${style.display}`);
        });
        const defs = document.createElementNS("http://www.w3.org/2000/svg", "defs");
        for (const use of copy.querySelectorAll("use")) {
          const ref = (use.getAttribute("href") ?? use.getAttribute("xlink:href") ?? "").split("#")[1];
          const target = ref && document.getElementById(ref);
          if (target && !copy.querySelector(`[id="${ref}"]`)) defs.append(target.cloneNode(true));
        }
        if (defs.childNodes.length) copy.prepend(defs);
        const image = new Image();
        image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(copy))}`;
        await image.decode();
        source = image;
      } else if (tag !== "canvas") {
        return null;
      }
      const work = document.createElement("canvas");
      work.width = cols * sub;
      work.height = rows * sub;
      const g = work.getContext("2d");
      g.drawImage(source, 0, 0, work.width, work.height);
      const data = g.getImageData(0, 0, work.width, work.height).data;
      const cells = [];
      for (let row = 0; row < rows; row++) {
        for (let col = 0; col < cols; col++) {
          let r = 0, gg = 0, b = 0, a = 0;
          for (let y = 0; y < sub; y++) {
            for (let x = 0; x < sub; x++) {
              const i = ((row * sub + y) * work.width + col * sub + x) * 4;
              const alpha = data[i + 3] / 255;
              r += data[i] * alpha; gg += data[i + 1] * alpha; b += data[i + 2] * alpha; a += alpha;
            }
          }
          const n = sub * sub;
          cells.push(a < 0.01 ? null : { color: hex({ r: r / a, g: gg / a, b: b / a }), alpha: +(a / n).toFixed(2) });
        }
      }
      return cells.some(Boolean) ? { cols, rows, cells } : null;
    } catch {
      return null;
    }
  };

  const hidden = (cs) => cs.display === "none" || cs.visibility === "hidden" || Number(cs.opacity) < 0.05;

  const textRuns = (node, el, cs, ink) => {
    const text = node.textContent;
    const kind = textKind(el);
    const size = parseFloat(cs.fontSize);
    const own = paint(cs.color);
    // transparent text over background-clip: text is painted by that background
    const color = own?.color ?? ink?.fill?.color ?? "#888888";
    const gradient = own ? null : ink?.gradient ?? null;
    const weight = Number(cs.fontWeight) || 400;
    const range = document.createRange();
    const chars = [...text];
    let offset = 0;
    let start = 0;
    let current = null;
    const flush = (end, cls) => {
      if (end <= start || cls === "space") return;
      range.setStart(node, start);
      range.setEnd(node, end);
      const lines = [...range.getClientRects()].filter(inView);
      const count = [...text.slice(start, end)].length;
      for (const r of lines) {
        if (cls === "text") {
          if (!text.slice(start, end).trim()) continue;
          const box = { kind, ...clip(r), size, color, weight };
          if (gradient) box.gradient = gradient;
          if (kind === "heading") box.text = lines.length === 1 ? text.slice(start, end).trim() : null;
          push(box);
        } else {
          push({ kind: "glyph", glyph: cls, ...clip(r), size, color, count: lines.length === 1 ? count : Math.max(1, Math.round(r.width / (size * 0.6))) });
        }
      }
    };
    for (const ch of chars) {
      const cls = /\s/.test(ch) ? (current === "text" ? "text" : "space") : glyphClass(ch);
      if (current !== null && cls !== current) {
        flush(offset, current);
        start = offset;
      }
      current = cls;
      offset += ch.length;
    }
    flush(offset, current ?? "text");
  };

  const visit = async (el, inherited) => {
    const cs = getComputedStyle(el);
    if (hidden(cs)) return;
    const rect = el.getBoundingClientRect();
    const tag = el.tagName.toLowerCase();

    if (["img", "svg", "canvas", "video", "picture", "iframe"].includes(tag)) {
      if (inView(rect)) {
        const radius = parseFloat(cs.borderTopLeftRadius) || 0;
        const grid = await sample(el, tag, rect, cs);
        push({ kind: "media", ...clip(rect), radius, tint: paint(cs.color)?.color ?? "#888888", grid });
      }
      return;
    }

    // background-clip: text paints the glyphs, not a box, so it inks the text inside
    const clipsText = cs.backgroundClip === "text" || cs.webkitBackgroundClip === "text";
    const ink = clipsText ? background(cs) : inherited;

    if (!clipsText && inView(rect) && el !== document.body && el !== document.documentElement) {
      const bg = background(cs);
      const fill = bg.fill ?? bg.base ?? null;
      const radius = parseFloat(cs.borderTopLeftRadius) || 0;
      const sides = ["Top", "Right", "Bottom", "Left"].map((side) => ({
        side,
        width: parseFloat(cs[`border${side}Width`]) || 0,
        paint: cs[`border${side}Style`] === "none" ? null : paint(cs[`border${side}Color`]),
      }));
      const drawn = sides.filter((s) => s.width > 0 && s.paint);
      const shadow = cs.boxShadow !== "none";
      const closed = drawn.length === 4;

      if (fill || bg.gradient || closed || shadow) {
        push({
          kind: "surface", ...clip(rect), radius,
          fill: fill?.color ?? null, fillAlpha: fill?.alpha ?? 0, gradient: bg.gradient ?? null,
          stroke: closed ? drawn[0].paint.color : null, strokeWidth: closed ? drawn[0].width : 0,
        });
      }
      if (!closed) {
        for (const s of drawn) {
          const r = clip(rect);
          const w = s.width;
          const line = {
            Top: { x: r.x, y: rect.top, w: r.w, h: w },
            Bottom: { x: r.x, y: rect.bottom - w, w: r.w, h: w },
            Left: { x: rect.left, y: r.y, w, h: r.h },
            Right: { x: rect.right - w, y: r.y, w, h: r.h },
          }[s.side];
          push({ kind: "rule", ...line, color: s.paint.color, alpha: s.paint.alpha });
        }
      }
    }

    for (const node of el.childNodes) {
      if (node.nodeType === Node.ELEMENT_NODE) {
        await visit(node, ink);
      } else if (node.nodeType === Node.TEXT_NODE && node.textContent.trim()) {
        textRuns(node, el, cs, ink);
      }
    }
  };

  await visit(document.body);

  return { width: VIEW_W, height: VIEW_H, canvas: canvas.color, boxes };
}
