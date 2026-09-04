
const KEY = "tagpad-wasm:" + TASK.version;
const FACE = ["A","B","X","Y"], GCOL = ["a","b","c","d"];
const esc = s => s.replace(/&/g,"&amp;").replace(/</g,"&lt;");
const $ = id => document.getElementById(id);
let core = null, hadPad = false;

function saved(){ try { return JSON.parse(localStorage.getItem(KEY) || "{}"); } catch { return {}; } }
function persist(){ try { localStorage.setItem(KEY, core.decisions()); } catch {} }

/* Rumble is a Web API, so it stays host-side -- but *whether* to rumble is
   decided in Rust and arrives in the frame result. */
function rumble(ms, strength){
  const p = livePad();
  if (!p) return;
  try {
    const a = p.vibrationActuator;
    if (a?.playEffect) a.playEffect("dual-rumble",
      { duration: ms, strongMagnitude: strength, weakMagnitude: strength }).catch(()=>{});
  } catch {}
}

function settle(r){
  if (r.assigned) rumble(30, .4);
  if (r.recorded){
    persist();
    const flash = r.recorded === "same" ? "keep" : r.recorded;
    $("card").classList.add("flash-" + flash);
    rumble(55, .55);
    setTimeout(() => $("card").classList.remove("flash-" + flash), 190);
  }
  if (r.acted) draw();
}

function draw(){
  const v = core.view(); if (!v) return;
  const c = v.card;
  $("gid").textContent = c.id;
  $("qtext").textContent = c.question;

  if (v.mode === "partition"){
    $("flag").textContent = "tap an item, then a group";
    $("items").innerHTML = c.items.map((x,n) => {
      const g = v.assigned[n] ?? 0;
      return '<li class="asg g' + GCOL[g] + (v.cursor === n ? " sel" : "") + '">'
           + '<span class="badge">' + (g+1) + '</span>' + esc(x) + '</li>';
    }).join("");
    const k = new Set(v.assigned).size;
    $("opts").innerHTML =
      '<div class="grp">' + GCOL.map((_,g) =>
        '<div class="gbtn g' + GCOL[g] + '" data-g="' + g + '"><b>' + (g+1) + '</b><i>' + FACE[g] + '</i></div>'
      ).join("") + '</div>'
      + '<div class="opt act" data-key="enter"><span class="glyph g-start">&#9655;</span>'
      + '<span>Confirm<small>' + k + ' group' + (k===1?"":"s") + '</small></span></div>'
      + '<div class="opt act" data-key="escape"><span class="glyph g-sel">&#9723;</span><span>Cancel</span></div>';
    // Pointer input routes through the same key bindings the keyboard uses, so
    // a tap and a keypress cannot diverge.
    [...$("items").children].forEach((el,n) => el.onclick = () => settle(core.key("select:" + n)));
    $("opts").querySelectorAll(".gbtn").forEach(el =>
      el.onclick = () => settle(core.key(String(+el.dataset.g + 1))));
  } else {
    $("flag").textContent = c.flag ?? "";
    $("items").innerHTML = c.items.map(x => "<li>" + esc(x) + "</li>").join("");
    $("opts").innerHTML = c.options.map((o,n) => {
      const glyph = ["g-south","g-east","g-north"][n] ?? "";
      const on = v.recorded && v.recorded.verdict === o.id ? " on" : "";
      const hint = o.hint ? "<small>" + esc(o.hint) + "</small>" : "";
      return '<div class="opt act o-' + o.id + on + '" data-key="' + ["k","s","u"][n] + '">'
           + '<span class="glyph ' + glyph + '">' + (FACE[n] ?? "") + '</span>'
           + '<span>' + esc(o.label) + hint + '</span></div>';
    }).join("");
  }
  $("opts").querySelectorAll(".act").forEach(el =>
    el.onclick = () => settle(core.key(el.dataset.key)));

  $("pos").textContent = (v.position + 1) + " / " + v.total;
  $("fill").style.width = (v.done / v.total * 100) + "%";
  $("saved").textContent = v.done + " of " + v.total + " recorded";
  $("blob").value = core.output();
}

/* The entire input loop. No edge detection, no deadzones, no bindings -- copy
   the frame in and act on what comes back. */
function loop(){
  requestAnimationFrame(loop);
  if (!core) return;
  const pad = livePad();
  if (pad && !hadPad){
    hadPad = true;
    $("dot").classList.add("on");
    $("padtxt").textContent = pad.id.slice(0, 26);
  } else if (!pad && hadPad){
    hadPad = false;
    core.resetInput();
    $("dot").classList.remove("on");
    $("padtxt").textContent = "disconnected";
  }
  settle(core.frame(pad));
}

addEventListener("keydown", e => {
  if (e.target.tagName === "TEXTAREA" || !core) return;
  settle(core.key(e.key.length === 1 ? e.key.toLowerCase() : e.key.toLowerCase()));
});

WebAssembly.instantiate(b64ToBytes(WASM_B64), {}).then(({ instance }) => {
  core = new Core(instance.exports);
  if (!core.start(TASK, saved())){
    $("qtext").textContent = "could not load the task file";
    return;
  }
  $("engine").textContent = "rust/wasm";
  draw();
  loop();
}).catch(err => { $("qtext").textContent = "wasm failed: " + err.message; });

$("show").onclick  = () => $("dlg").showModal();
$("close").onclick = () => $("dlg").close();
$("copy").onclick  = async () => {
  try { await navigator.clipboard.writeText($("blob").value); $("copy").textContent = "Copied"; }
  catch { $("blob").select(); $("copy").textContent = "Select + copy"; }
  setTimeout(() => { $("copy").textContent = "Copy JSON"; }, 1300);
};
