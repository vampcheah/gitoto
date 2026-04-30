const fs = require("fs");
const os = require("os");
const path = require("path");
const { execFileSync, spawn } = require("child_process");

const ROOT = path.resolve(__dirname, "..");
const ASSETS = path.join(ROOT, "assets");
const DEMO = path.join(os.tmpdir(), "gitoto-screenshots-demo");
const ROWS = 40;
const COLS = 120;
const WIDTH = COLS * 10;
const HEIGHT = ROWS * 20 + 64;

function run(cmd, args, opts = {}) {
  execFileSync(cmd, args, {
    cwd: opts.cwd || ROOT,
    env: { ...process.env, ...(opts.env || {}) },
    stdio: opts.stdio || "inherit",
  });
}

function write(file, text) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
}

function git(repo, args) {
  run("git", args, {
    cwd: repo,
    env: {
      GIT_AUTHOR_NAME: "FrancisXiaobu",
      GIT_AUTHOR_EMAIL: "francis@example.com",
      GIT_COMMITTER_NAME: "FrancisXiaobu",
      GIT_COMMITTER_EMAIL: "francis@example.com",
    },
    stdio: "ignore",
  });
}

function makeRepo(name, dirty = false) {
  const repo = path.join(DEMO, name);
  fs.mkdirSync(repo, { recursive: true });
  git(repo, ["init", "-b", "main"]);
  git(repo, ["config", "user.name", "FrancisXiaobu"]);
  git(repo, ["config", "user.email", "francis@example.com"]);
  write(path.join(repo, "README.md"), `# ${name}\n\nDemo repository.\n`);
  write(path.join(repo, "src", "app.rs"), "pub fn app() {}\n");
  git(repo, ["add", "."]);
  git(repo, ["commit", "-m", "Initial import"]);
  write(path.join(repo, "src", "search.rs"), "pub fn search() {}\n");
  git(repo, ["add", "."]);
  git(repo, ["commit", "-m", "Upgrade Rust to 1.95"]);

  const remote = path.join(DEMO, `${name}.git`);
  run("git", ["init", "--bare", remote], { stdio: "ignore" });
  git(repo, ["remote", "add", "origin", remote]);
  git(repo, ["push", "-u", "origin", "main"]);

  write(path.join(repo, "src", "ui.rs"), "pub fn draw() {}\n");
  git(repo, ["add", "."]);
  git(repo, ["commit", "-m", "Fix binary file opening"]);
  write(path.join(repo, "src", "config.rs"), "pub fn config() {}\n");
  git(repo, ["add", "."]);
  git(repo, ["commit", "-m", "Simplify header title"]);

  if (dirty) {
    fs.appendFileSync(path.join(repo, "README.md"), "\nLocal edit.\n");
    fs.appendFileSync(path.join(repo, "src", "app.rs"), "// local edit\n");
    write(path.join(repo, "src", "app", "input.rs"), "pub fn input() {}\n");
    write(path.join(repo, "src", "app", "mod.rs"), "pub mod input;\n");
    write(path.join(repo, "src", "app", "ops.rs"), "pub fn ops() {}\n");
  }
}

function prepareDemo() {
  fs.rmSync(DEMO, { recursive: true, force: true });
  fs.mkdirSync(DEMO, { recursive: true });
  makeRepo("034_kudzu", true);
  makeRepo("atlas-api", true);
  makeRepo("folio-ui", false);
}

function capture(name, events = [], durationMs = 5000) {
  const ansi = path.join(os.tmpdir(), `gitoto-${name}.ansi`);
  const html = path.join(os.tmpdir(), `gitoto-${name}.html`);
  const bin = path.join(ROOT, "target", "debug", "gitoto");
  const inner = [
    `stty rows ${ROWS} cols ${COLS}`,
    `exec "${bin}" --root "${DEMO}" --fast`,
  ].join("; ");

  return new Promise((resolve, reject) => {
    const child = spawn(
      "script",
      ["-q", "-f", "-e", "-c", `bash -lc ${JSON.stringify(inner)}`, ansi],
      {
        cwd: ROOT,
        env: process.env,
        detached: true,
        stdio: ["pipe", "ignore", "ignore"],
      },
    );

    const timers = events.map((event) =>
      setTimeout(() => child.stdin.write(event.data), event.delay),
    );
    const killer = setTimeout(() => {
      try {
        process.kill(-child.pid, "SIGINT");
      } catch (_) {
        child.kill("SIGINT");
      }
    }, durationMs);

    child.once("error", reject);
    child.once("exit", () => {
      for (const timer of timers) clearTimeout(timer);
      clearTimeout(killer);
      try {
        execFileSync("pkill", ["-f", `${bin} --root ${DEMO}`], { stdio: "ignore" });
      } catch (_) {
        // Process may already have exited.
      }
      try {
        renderAnsi(ansi, html);
        screenshotHtml(html, path.join(ASSETS, `screenshot-${name}.png`));
        resolve();
      } catch (error) {
        reject(error);
      }
    });
  });
}

function screenshotHtml(html, output) {
  const chrome =
    findCommand("google-chrome") ||
    findCommand("chromium") ||
    findCommand("chromium-browser");
  if (!chrome) {
    throw new Error("google-chrome or chromium is required for screenshots");
  }
  run(
    chrome,
    [
      "--headless=new",
      "--disable-gpu",
      "--no-sandbox",
      `--window-size=${WIDTH},${HEIGHT}`,
      `--screenshot=${output}`,
      `file://${html}`,
    ],
    { stdio: "ignore" },
  );
}

function findCommand(command) {
  try {
    return execFileSync("bash", ["-lc", `command -v ${command}`], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  } catch (_) {
    return null;
  }
}

function blank() {
  return { ch: " ", bold: false, italic: false, fg: null, bg: null };
}

function renderAnsi(input, outputHtml) {
  const data = fs.readFileSync(input, "utf8");
  let screen = Array.from({ length: ROWS }, () =>
    Array.from({ length: COLS }, blank),
  );
  let row = 0;
  let col = 0;
  let alt = false;
  let style = { bold: false, italic: false, fg: null, bg: null };

  const colors = {
    30: "#45475a",
    31: "#f38ba8",
    32: "#a6e3a1",
    33: "#f9e2af",
    34: "#89b4fa",
    35: "#cba6f7",
    36: "#94e2d5",
    37: "#cdd6f4",
    90: "#585b70",
    91: "#f38ba8",
    92: "#a6e3a1",
    93: "#f9e2af",
    94: "#89b4fa",
    95: "#cba6f7",
    96: "#94e2d5",
    97: "#f5e0dc",
  };

  function resetScreen() {
    screen = Array.from({ length: ROWS }, () =>
      Array.from({ length: COLS }, blank),
    );
    row = 0;
    col = 0;
  }

  function put(ch) {
    if (!alt) return;
    if (ch === "\n") {
      row = Math.min(ROWS - 1, row + 1);
      return;
    }
    if (ch === "\r") {
      col = 0;
      return;
    }
    if (row < 0 || row >= ROWS || col < 0 || col >= COLS) return;
    screen[row][col] = { ch, ...style };
    col = Math.min(COLS - 1, col + 1);
  }

  function sgr(params) {
    if (params.length === 0) params = [0];
    for (let i = 0; i < params.length; i += 1) {
      const p = params[i];
      if (p === 0) style = { bold: false, italic: false, fg: null, bg: null };
      else if (p === 1) style.bold = true;
      else if (p === 3) style.italic = true;
      else if (p === 22) style.bold = false;
      else if (p === 23) style.italic = false;
      else if ((p >= 30 && p <= 37) || (p >= 90 && p <= 97)) style.fg = colors[p];
      else if (p === 39) style.fg = null;
      else if (p >= 40 && p <= 47) style.bg = colors[p - 10];
      else if (p === 49) style.bg = null;
      else if ((p === 38 || p === 48) && params[i + 1] === 2) {
        const value = `rgb(${params[i + 2]},${params[i + 3]},${params[i + 4]})`;
        if (p === 38) style.fg = value;
        else style.bg = value;
        i += 4;
      }
    }
  }

  for (let i = 0; i < data.length; i += 1) {
    const ch = data[i];
    if (ch !== "\x1b") {
      put(ch);
      continue;
    }
    if (data[i + 1] !== "[") {
      i += 1;
      continue;
    }
    let j = i + 2;
    while (j < data.length && !/[A-Za-z~]/.test(data[j])) j += 1;
    if (j >= data.length) break;
    const body = data.slice(i + 2, j);
    const cmd = data[j];
    i = j;

    if (body === "?1049" && cmd === "h") {
      alt = true;
      resetScreen();
      continue;
    }
    if (body === "?1049" && cmd === "l") {
      alt = false;
      continue;
    }
    if (!alt) continue;

    if (cmd === "H" || cmd === "f") {
      const parts = body
        .split(";")
        .filter(Boolean)
        .map((n) => parseInt(n, 10));
      row = Math.max(0, Math.min(ROWS - 1, (parts[0] || 1) - 1));
      col = Math.max(0, Math.min(COLS - 1, (parts[1] || 1) - 1));
    } else if (cmd === "m") {
      const parts = body
        .split(";")
        .filter((x) => x !== "")
        .map((n) => parseInt(n, 10))
        .filter((n) => !Number.isNaN(n));
      sgr(parts);
    } else if (cmd === "J") {
      if (body === "2" || body === "") resetScreen();
    } else if (cmd === "K") {
      for (let x = col; x < COLS; x += 1) screen[row][x] = blank();
    }
  }

  const esc = (s) =>
    s.replace(/[&<>]/g, (m) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[m]);
  for (let i = 0; i < screen.length; i += 1) {
    const line = screen[i].map((cell) => cell.ch).join("");
    if (line.includes("Script started on") || line.includes("Script done on") || line.includes("Script")) {
      screen[i] = Array.from({ length: COLS }, blank);
    }
  }
  let htmlRows = "";
  for (const cells of screen) {
    let line = "";
    let last = null;
    let buf = "";
    const flush = () => {
      if (!buf) return;
      const css = [];
      if (last.bold) css.push("font-weight:700");
      if (last.italic) css.push("font-style:italic");
      if (last.fg) css.push(`color:${last.fg}`);
      if (last.bg) css.push(`background:${last.bg}`);
      line += css.length
        ? `<span style="${css.join(";")}">${esc(buf)}</span>`
        : esc(buf);
      buf = "";
    };
    for (const cell of cells) {
      const key = JSON.stringify({
        bold: cell.bold,
        italic: cell.italic,
        fg: cell.fg,
        bg: cell.bg,
      });
      if (last && key !== last.key) flush();
      if (!last || key !== last.key) last = { key, ...cell };
      buf += cell.ch;
    }
    flush();
    htmlRows += `<div class="row">${line}</div>`;
  }

  const html = `<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;background:#11111b;width:${WIDTH}px;height:${HEIGHT}px;overflow:hidden}
.terminal{box-sizing:border-box;width:${WIDTH}px;height:${HEIGHT}px;padding:16px;background:#1e1e2e;color:#cdd6f4;font-family:"Fira Code","JetBrains Mono","DejaVu Sans Mono",monospace;font-size:16px;line-height:19.2px;white-space:pre}
.row{height:19.2px}
</style><div class="terminal">${htmlRows}</div>`;
  fs.writeFileSync(outputHtml, html);
}

async function main() {
  prepareDemo();
  run("cargo", ["build"]);
  await capture("overview");
  await capture("commit-input", [
    { delay: 2200, data: "j" },
    { delay: 2500, data: "c" },
    { delay: 2800, data: "Ship gitoto workflow" },
  ]);
  await capture("context-menu", [
    { delay: 2200, data: "\x1b[<2;50;2M\x1b[<2;50;2m" },
  ]);
  await capture("repo-focus", [{ delay: 2200, data: "\r" }]);
  await capture("github-repo-input", [
    { delay: 2200, data: "\x1b[<2;50;2M\x1b[<2;50;2m" },
    { delay: 2600, data: "jjjjjj\r" },
  ]);
  await capture(
    "operation-log",
    [
      { delay: 2200, data: "p" },
      { delay: 4400, data: "o" },
    ],
    7000,
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
