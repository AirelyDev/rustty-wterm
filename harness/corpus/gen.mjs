#!/usr/bin/env node
// Generates the differential corpus: corpus/<name>.bin plus corpus/manifest.json.
// Every entry is small and deterministic; `random-fuzz-<seed>` entries mix the
// fragments below with a seeded PRNG, so regenerating gives identical bytes.
//
// Usage: node harness/corpus/gen.mjs
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const ESC = "\x1b";
const CSI = `${ESC}[`;
const OSC = `${ESC}]`;
const BEL = "\x07";
const ST = `${ESC}\\`;

const enc = (s) => Buffer.from(s, "utf8");

// -- Fragments (each a string; shared by the named entries and the fuzzer) --

const lines = (n, prefix = "line") =>
  Array.from({ length: n }, (_, i) => `${prefix} ${String(i).padStart(4, "0")}\r\n`).join("");

const sgrBasic = (() => {
  let s = "";
  for (let i = 30; i <= 37; i++) s += `${CSI}${i}mF${i}`;
  for (let i = 40; i <= 47; i++) s += `${CSI}${i}mB${i}`;
  for (let i = 90; i <= 97; i++) s += `${CSI}${i}mf${i}`;
  for (let i = 100; i <= 107; i++) s += `${CSI}${i}mb${i}`;
  s += `${CSI}0m\r\n`;
  for (const [n, label] of [
    [1, "bold"], [2, "faint"], [3, "italic"], [5, "blink"],
    [7, "inverse"], [8, "invisible"], [9, "strike"],
  ]) s += `${CSI}${n}m${label}${CSI}0m `;
  s += `${CSI}1;3;4;9mall${CSI}22;23;24;29moff${CSI}0m\r\n`;
  return s;
})();

const sgr256 = (() => {
  let s = "";
  for (let n = 0; n < 256; n += 17) s += `${CSI}38;5;${n}mx${CSI}48;5;${255 - n}my`;
  s += `${CSI}0m\r\n`;
  s += `${CSI}38;2;255;0;0mR${CSI}38;2;0;255;0mG${CSI}38;2;0;0;255mB`;
  s += `${CSI}48;2;10;20;30mbg${CSI}38:2::1:2:3mcolon${CSI}39;49mdef${CSI}0m\r\n`;
  return s;
})();

const underlines = (() => {
  let s = "";
  for (let i = 0; i <= 5; i++) s += `${CSI}4:${i}mu${i}`;
  s += `${CSI}4m${CSI}58;2;255;128;0morange${CSI}58;5;196mpal${CSI}59mnone`;
  s += `${CSI}21mdouble${CSI}24moff${CSI}0m\r\n`;
  return s;
})();

const wideCjk = "漢字テスト 한국어 中文\r\nab漢cd\r\n" + "字".repeat(45) + "\r\n";
const emojiZwj =
  "family:👨‍👩‍👧‍👦 flags:🇫🇷🇯🇵 skin:👍🏽 heart:❤️ text:☺︎\r\n";
const combining = "é ạ̀ ñ x⃝\r\n";
const longLine = "The quick brown fox jumps over the lazy dog. ".repeat(5) + "END\r\n";

const fragments = {
  plain: "Hello, world!\r\nSecond line\r\nno newline at end",
  "sgr-basic": sgrBasic,
  "sgr-256-truecolor": sgr256,
  underlines,
  "wide-cjk": wideCjk,
  "emoji-zwj": emojiZwj,
  combining,
  "wrap-and-reflow": longLine + "short\r\n" + longLine,
  "alt-screen": `primary\r\n${CSI}?1049h${CSI}2J${CSI}Halt text${CSI}5;5Hmid${CSI}?1049lback\r\n`,
  "cursor-moves":
    `${CSI}5;10HA${CSI}2AB${CSI}3BC${CSI}4CD${CSI}2DE${ESC}7${CSI}1;1Hsaved` +
    `${ESC}8F${CSI}10;1H0123456789${CSI}10;3H${CSI}4X${CSI}12;20H${CSI}s${CSI}H${CSI}u!` +
    `${CSI}3G|${CSI}7d^${CSI}K${CSI}1K${CSI}2Kx\r\n`,
  "scroll-region":
    `${CSI}2J${CSI}H` + lines(8, "r") + `${CSI}3;6r${CSI}6;1H${ESC}Dind${ESC}Dind2` +
    `${CSI}3;1H${ESC}Mri${ESC}Mri2${CSI}2S${CSI}1T${CSI}r${CSI}20;1Hdone`,
  tabs: `a\tb\tc\r\n${ESC}H${CSI}3g\t${CSI}20G${ESC}H\r\tx\t y${CSI}0g${CSI}2I*${CSI}1Z+\r\n`,
  osc8:
    `${OSC}8;;https://example.com${ST}link${OSC}8;;${ST} ` +
    `${OSC}8;id=a1;https://example.com/a${BEL}idlink${OSC}8;;${BEL} ` +
    `${OSC}8;id=a1;https://example.com/a${BEL}same${OSC}8;;${BEL}\r\n`,
  "osc-title-pwd": `${OSC}0;first title${BEL}${OSC}2;Final ✓ title${ST}${OSC}7;file://host/home/user/dir${BEL}\r\n`,
  "osc-colors":
    `${OSC}4;1;rgb:ff/00/00${BEL}${OSC}4;1;?${BEL}${CSI}31mred${CSI}0m` +
    `${OSC}10;#112233${BEL}${OSC}11;rgb:44/55/66${BEL}${OSC}12;#778899${BEL}` +
    `${OSC}10;?${BEL}${OSC}11;?${BEL}${OSC}12;?${BEL}` +
    `${OSC}104;1${BEL}${OSC}110${BEL}${OSC}111${BEL}${OSC}112${BEL}${OSC}10;?${ST}\r\n`,
  "mouse-modes": `${CSI}?1000h${CSI}?1002h${CSI}?1003h${CSI}?1006h${CSI}?1016h`,
  "bracketed-paste-focus-sync": `${CSI}?2004h${CSI}?1004h${CSI}?2026h${CSI}?1h`,
  bell: `ding${BEL}${BEL}${BEL}\r\n`,
  osc133:
    `${OSC}133;A${BEL}$ ${OSC}133;B${BEL}ls${OSC}133;C${BEL}\r\nout\r\n${OSC}133;D;0${BEL}` +
    `${OSC}133;A${BEL}$ ${OSC}133;B${BEL}false${OSC}133;C${BEL}\r\n${OSC}133;D;1${BEL}` +
    `${OSC}133;A${BEL}$ `,
  decscusr: `${CSI}1 q${CSI}2 q${CSI}3 q${CSI}4 q${CSI}5 q${CSI}6 q${CSI}?25l${CSI}?25h${CSI}3 q`,
};

const decrqmModes = [
  1, 3, 4, 5, 6, 7, 8, 9, 12, 25, 40, 45, 47, 66, 67, 69, 117, 1000, 1001, 1002,
  1003, 1004, 1005, 1006, 1007, 1015, 1016, 1035, 1036, 1039, 1045, 1047, 1048,
  1049, 2004, 2026, 2027, 2031, 2048, 5522,
];
const queries =
  `${CSI}c${CSI}5n${CSI}3;7H${CSI}6n${CSI}?6n` +
  decrqmModes.map((m) => `${CSI}?${m}$p`).join("") +
  `${CSI}>1u${CSI}?u${CSI}<u${CSI}?u`;
// ANSI-mode DECRQM (IRM, LNM), separate so its replies do not shift the others.
const queriesAnsi = `${CSI}4$p${CSI}20$p${CSI}4h${CSI}4$p${CSI}4l`;
// Queries both engines answer identically, for the fuzzer (the DECRQM sweep
// is characterised by the `queries` entry alone).
const queriesCommon = `${CSI}c${CSI}5n${CSI}6n${CSI}?25$p${CSI}?2004$p${CSI}?u`;

// The 2x1 RGB image of Task 5 (f=24, direct, a=T) and a query.
const kittyImage = `${ESC}_Ga=T,f=24,s=2,v=1,i=7;/wAAAP8A${ST}${ESC}_Ga=q,i=9,s=1,v=1,f=24;AAAA${ST}`;

// -- Named entries --------------------------------------------------------

/** @type {{name: string, steps: object[], options?: object, skip?: string, chunkings?: string}[]} */
const entries = [];
const bins = new Map();
function bin(name, bytes) {
  bins.set(name, Buffer.isBuffer(bytes) ? bytes : enc(bytes));
  return { write: `${name}.bin` };
}
function entry(name, data, extra = {}) {
  entries.push({ name, steps: [bin(name, data)], ...extra });
}

for (const name of [
  "plain", "sgr-basic", "sgr-256-truecolor", "underlines", "wide-cjk",
  "emoji-zwj", "combining",
]) entry(name, fragments[name]);

entries.push({
  name: "wrap-and-reflow",
  steps: [
    bin("wrap-and-reflow", fragments["wrap-and-reflow"]),
    { resize: [40, 24] },
    { resize: [33, 10] },
    { resize: [100, 24] },
  ],
});
entry("scrollback-120", lines(120));
// 2000 lines through a small byte limit: pages are pruned, so the discarded
// count (approximated on the Rust side) is exercised.
entry("scrollback-overflow", lines(2000, "overflow"), { options: { scrollbackLimit: 20000 } });
entry("alt-screen", fragments["alt-screen"]);
for (const name of [
  "cursor-moves", "scroll-region", "tabs", "osc8", "osc-title-pwd", "osc-colors",
]) entry(name, fragments[name]);
entry("queries", queries);
entry("queries-ansi-modes", queriesAnsi);
entry("mouse-modes", fragments["mouse-modes"]);
entry("bracketed-paste-focus-sync", fragments["bracketed-paste-focus-sync"]);

// OSC 52: only the latest valid write is observable, so one entry per case.
const b64 = (s) => Buffer.from(s).toString("base64");
entry("osc52-valid", `${OSC}52;c;${b64("hello clipboard")}${BEL}`);
entry("osc52-invalid", `${OSC}52;c;${b64("kept")}${BEL}${OSC}52;c;!!!not*base64${BEL}`);
entry("osc52-query", `${OSC}52;c;${b64("kept")}${BEL}${OSC}52;c;?${BEL}`);
entry("osc52-oversize", `${OSC}52;c;${b64("kept")}${BEL}${OSC}52;c;${b64("x".repeat(70 * 1024))}${BEL}`);
entry("osc52-selector", `${OSC}52;0;${b64("selector zero")}${BEL}`);
entry("osc52-unpadded", `${OSC}52;c;${b64("ab").replace(/=+$/, "")}${BEL}`);

entry("bell", fragments.bell);
entry("osc133", fragments.osc133);
entry("decscusr", fragments.decscusr);
// DL on a prompt row: the recycled blank row at the bottom must not inherit
// the prompt mark (found by random-fuzz-4/18).
entry("prompt-delete-lines", `${OSC}133;A${BEL}$ ${CSI}2M\r\n`);
entry("kitty-image-rgb", kittyImage, {
  skip: "Task 5 outcome B: the Rust build has no kitty graphics; image replies and state exist only in the Zig build",
});

// A tracked position across a screen switch and return inside one write.
entries.push({
  name: "tracked-alt-screen",
  steps: [
    bin("tracked-alt-screen-0", "prompt$ \r\nsecond\r\n"),
    { track: [0, 3] },
    { track: [1, 2] },
    bin("tracked-alt-screen-1", `${CSI}?1049hfull screen app${CSI}?1049lafter\r\n`),
  ],
});

// Every two-part split of a stream rich in multi-byte UTF-8 and escapes.
entry(
  "chunked-split",
  `${CSI}1;38;2;1;2;3mé漢👍🏽${OSC}8;;https://x.y${ST}l${OSC}8;;${ST}${OSC}2;t✓${BEL}` +
    `${CSI}?2004h${CSI}6n${CSI}4:3mé${CSI}0m\r\n`,
  { chunkings: "splits" },
);

// -- Fuzz -----------------------------------------------------------------

function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
// Fragments for mixing; whole-screen clears and the 120/2000-line feeds are
// left out so that later fragments stay visible.
const pool = [
  ...Object.values(fragments),
  queriesCommon,
  `${OSC}52;c;${b64("fuzz")}${BEL}`,
  lines(5, "fz"),
  `${CSI}?1049h`, `${CSI}?1049l`, `${CSI}2;20r`, `${CSI}r`, `${CSI}H`, `${CSI}J`,
  `${CSI}?7l`, `${CSI}?7h`, `${CSI}?6h`, `${CSI}?6l`, `${CSI}4h`, `${CSI}4l`,
  `${CSI}5@`, `${CSI}3P`, `${CSI}2L`, `${CSI}2M`, `${ESC}c`, `${ESC}#8`,
  "\r\n", "\b\b", "\x0e\x0f", `${ESC}(0lqk${ESC}(B`,
];
const FUZZ_SEEDS = 20;
const FUZZ_MIXES = 50; // 20 seeds x 50 mixes = 1 000 mixes
for (let seed = 1; seed <= FUZZ_SEEDS; seed++) {
  const rand = mulberry32(seed);
  let s = "";
  for (let i = 0; i < FUZZ_MIXES; i++) s += pool[Math.floor(rand() * pool.length)];
  entry(`random-fuzz-${seed}`, s);
}

// -- Emit -----------------------------------------------------------------

for (const f of readdirSync(here)) if (f.endsWith(".bin")) rmSync(join(here, f));
mkdirSync(here, { recursive: true });
for (const [name, bytes] of bins) writeFileSync(join(here, `${name}.bin`), bytes);
writeFileSync(join(here, "manifest.json"), JSON.stringify({ entries }, null, 2) + "\n");
console.log(`corpus: ${entries.length} entries, ${bins.size} files`);
