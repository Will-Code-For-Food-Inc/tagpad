#!/usr/bin/env python3
"""Inline the wasm and the front end into one self-contained HTML file."""
import base64, json, pathlib, sys

root = pathlib.Path(__file__).resolve().parent.parent
shell = (root / "web/shell.html").read_text()
glue = (root / "web/glue.js").read_text()
app = (root / "web/app.js").read_text()
wasm = base64.b64encode(
    (root / "target/wasm32-unknown-unknown/release/tagpad_wasm.wasm").read_bytes()
).decode()
task = json.loads(pathlib.Path(sys.argv[1]).read_text())

out = shell.replace("/*SCRIPT*/", lambda_body := (
    'const WASM_B64="' + wasm + '";\n'
    + "const TASK=" + json.dumps(task) + ";\n"
    + glue + app
))
dest = root / "dist/index.html"
dest.parent.mkdir(exist_ok=True)
dest.write_text(out)
print(f"{dest}  {len(out)/1024:.0f} KB  ({len(task['cards'])} cards)")
