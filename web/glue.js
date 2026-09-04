/* Raw WebAssembly, no bindgen. The host is a pipe, not a state machine.
 *
 * Every call that returns data writes into a buffer inside the module and hands
 * back a pointer; we read result_len() and copy the bytes out immediately.
 * memory.buffer is re-read on every access because growing the heap detaches
 * the old ArrayBuffer -- caching it is the classic way hand-rolled glue breaks
 * a few hundred allocations in. */
class Core {
  constructor(exports) {
    this.m = exports;
    this.enc = new TextEncoder();
    this.dec = new TextDecoder();
    // Scratch buffers for the per-frame input copy, allocated once. Allocating
    // every frame would churn the wasm heap sixty times a second.
    this.btnPtr = this.m.alloc(16 * 4);
    this.axsPtr = this.m.alloc(8 * 4);
  }
  #u8(ptr, len) { return new Uint8Array(this.m.memory.buffer, ptr, len); }
  #f32(ptr, len) { return new Float32Array(this.m.memory.buffer, ptr, len); }
  #put(s) {
    const b = this.enc.encode(s), p = this.m.alloc(b.length);
    this.#u8(p, b.length).set(b);
    return [p, b.length];
  }
  #read(ptr) { return this.dec.decode(this.#u8(ptr, this.m.result_len()).slice()); }

  start(task, saved) {
    const [tp, tl] = this.#put(JSON.stringify(task));
    const [sp, sl] = this.#put(JSON.stringify(saved ?? {}));
    const ok = this.m.session_new(tp, tl, sp, sl) === 1;
    this.m.dealloc(tp, tl); this.m.dealloc(sp, sl);
    return ok;
  }

  /* The whole host-side input contract: copy this frame's raw values in.
   * Edge detection, deadzones and bindings are all on the far side. */
  frame(pad) {
    const b = this.#f32(this.btnPtr, 16), a = this.#f32(this.axsPtr, 8);
    b.fill(0); a.fill(0);
    if (pad) {
      for (let i = 0; i < Math.min(16, pad.buttons.length); i++) {
        const x = pad.buttons[i];
        b[i] = typeof x === "object" ? (x.pressed ? 1 : x.value) : x;
      }
      for (let i = 0; i < Math.min(8, pad.axes.length); i++) a[i] = pad.axes[i];
    }
    return JSON.parse(this.#read(this.m.input_frame(this.btnPtr, 16, this.axsPtr, 8)));
  }
  key(name) {
    const [p, l] = this.#put(name);
    const r = JSON.parse(this.#read(this.m.key_frame(p, l)));
    this.m.dealloc(p, l);
    return r;
  }
  resetInput() { this.m.input_reset(); }
  view()      { return JSON.parse(this.#read(this.m.session_view())); }
  output()    { return this.#read(this.m.session_output()); }
  decisions() { return this.#read(this.m.session_decisions()); }
}

function b64ToBytes(b64) {
  const bin = atob(b64), out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/* Find any connected pad. Chrome returns nulls until the user presses
 * something -- there is no way to ask first, so this is polled rather than
 * resolved once. */
function livePad() {
  const pads = navigator.getGamepads ? navigator.getGamepads() : [];
  for (const p of pads) if (p && p.connected) return p;
  return null;
}
