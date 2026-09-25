import assert from "node:assert/strict";
import headless from "@xterm/headless";
let json = "";
for await (const chunk of process.stdin) json += chunk;
const frames = JSON.parse(json);
const term = new headless.Terminal({ cols: 10, rows: 3, scrollback: 0, allowProposedApi: true });
const line = row => term.buffer.active.getLine(row);
const text = row => line(row).translateToString().trimEnd();
const cell = (row, column) => line(row).getCell(column);
for (const [i, frame] of frames.entries()) {
  term.resize(frame.columns, frame.rows);
  await new Promise(resolve => term.write(frame.ansi, resolve));
  if (i === 0) {
    assert.equal(cell(0, 0).getChars(), "界");
    assert.equal(cell(0, 0).getWidth(), 2);
    assert.equal(cell(0, 1).getWidth(), 0);
    assert.equal(cell(0, 2).getChars(), "e\u0301");
    assert.ok(cell(1, 0).isInvisible());
    assert.ok(cell(1, 0).isBold());
    assert.equal(cell(1, 0).getFgColor(), 0x010203);
    assert.equal(cell(1, 0).getBgColor(), 42);
    assert.equal(text(2), "123456789Z");
    assert.equal(term.buffer.active.cursorX, 9);
    assert.equal(term.buffer.active.cursorY, 2);
  } else if (i === 1) {
    assert.equal(text(0), "ab界");
    assert.equal(text(1), "");
    assert.equal(text(2), "");
    assert.equal(cell(0, 0).isInvisible(), 0);
  } else if (i === 2) {
    assert.equal(text(0), "abc");
  } else {
    assert.equal(text(0), "grown");
    assert.equal(text(3), "");
  }
}
term.dispose();
console.log("4 real Rust frames passed headless xterm cell/style/cursor/resize assertions");
