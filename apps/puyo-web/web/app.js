"use strict";

const WIDTH = 6;
const HEIGHT = 13;
const EMPTY = ".";
const CELL_NAMES = { ".": "空", R: "赤", G: "緑", B: "青", Y: "黄", O: "おじゃま" };

const samples = {
  single: fromColumns(["RRRR", "", "", "", "", ""]),
  double: fromColumns(["RRRBG", "RBBBG", "GGG", "", "", ""]),
  garbage: fromColumns(["RRRR", "OB", "B", "", "", ""]),
};

let board = cloneBoard(samples.double);
let selectedCell = "R";
let painting = false;
let result = null;
let frameIndex = 0;
let animationGeneration = 0;
let animationPlaying = false;
let gamePlacements = 0;
let gameMaximumChain = 0;

const grid = document.querySelector("#board-grid");
const rowLabels = document.querySelector("#row-labels");
const simulateButton = document.querySelector("#simulate-button");
const frameSlider = document.querySelector("#frame-slider");
const notice = document.querySelector("#notice");

function fromColumns(columns) {
  const next = Array.from({ length: HEIGHT }, () => Array(WIDTH).fill(EMPTY));
  columns.forEach((column, x) => {
    [...column].forEach((cell, bottomY) => {
      next[HEIGHT - 1 - bottomY][x] = cell;
    });
  });
  return next;
}

function cloneBoard(source) {
  return source.map((row) => [...row]);
}

function parseBoard(text) {
  return text.trim().split(/\r?\n|\//).filter(Boolean).map((row) => [...row]);
}

function serializeBoard(source = board) {
  return source.map((row) => row.join("")).join("/");
}

function puyoMarkup(cell) {
  if (cell === EMPTY) return "";
  const className = { R: "red", G: "green", B: "blue", Y: "yellow", O: "garbage" }[cell];
  return `<span class="puyo ${className}" aria-hidden="true"></span>`;
}

function buildGrid() {
  const cells = document.createDocumentFragment();
  for (let row = 0; row < HEIGHT; row += 1) {
    for (let column = 0; column < WIDTH; column += 1) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = `board-cell${row === 0 ? " hidden-row" : ""}`;
      button.dataset.row = String(row);
      button.dataset.column = String(column);
      button.setAttribute("role", "gridcell");
      button.addEventListener("pointerdown", handlePaintStart);
      button.addEventListener("pointerenter", handlePaintMove);
      button.addEventListener("contextmenu", handleErase);
      cells.append(button);
    }
  }
  grid.replaceChildren(cells);

  rowLabels.replaceChildren(
    ...Array.from({ length: HEIGHT }, (_, row) => {
      const label = document.createElement("span");
      label.textContent = row === 0 ? "13H" : String(HEIGHT - row);
      return label;
    }),
  );
  renderBoard();
}

function renderBoard(source = board) {
  grid.querySelectorAll(".board-cell").forEach((cell) => {
    cell.classList.remove("is-erasing", "is-vanishing");
    const row = Number(cell.dataset.row);
    const column = Number(cell.dataset.column);
    const value = source[row][column];
    cell.innerHTML = puyoMarkup(value);
    cell.setAttribute("aria-label", `${HEIGHT - row}段 ${column + 1}列: ${CELL_NAMES[value]}`);
  });
}

function paintCell(target, value) {
  const row = Number(target.dataset.row);
  const column = Number(target.dataset.column);
  board[row][column] = value;
  target.innerHTML = puyoMarkup(value);
  target.setAttribute("aria-label", `${HEIGHT - row}段 ${column + 1}列: ${CELL_NAMES[value]}`);
  resetResult("盤面を変更しました。再実行すると結果が更新されます。");
}

function handlePaintStart(event) {
  if (event.button !== 0) return;
  detachFrameForEditing();
  painting = true;
  paintCell(event.currentTarget, selectedCell);
}

function handlePaintMove(event) {
  if (painting && event.buttons === 1) paintCell(event.currentTarget, selectedCell);
}

function handleErase(event) {
  event.preventDefault();
  detachFrameForEditing();
  paintCell(event.currentTarget, EMPTY);
}

function detachFrameForEditing() {
  if (!result) return;
  board = parseBoard(result.frames[frameIndex].board);
  result = null;
  stopPlayback();
}

function selectPalette(value) {
  selectedCell = value;
  document.querySelectorAll(".palette-button").forEach((button) => {
    button.classList.toggle("is-selected", button.dataset.cell === value);
  });
}

function applyGravity() {
  for (let column = 0; column < WIDTH; column += 1) {
    const cells = board.map((row) => row[column]).filter((cell) => cell !== EMPTY);
    const emptyCount = HEIGHT - cells.length;
    for (let row = 0; row < HEIGHT; row += 1) {
      board[row][column] = row < emptyCount ? EMPTY : cells[row - emptyCount];
    }
  }
  renderBoard();
  resetResult("各列を重力方向に詰めました。");
}

async function checkBackend() {
  const status = document.querySelector("#engine-status");
  const label = document.querySelector("#engine-label");
  try {
    const response = await fetch("/api/health", { cache: "no-store" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const health = await response.json();
    status.className = "engine-status is-online";
    label.textContent = `Native SIMD · SSE2 · BMI2 ${health.bmi2 ? "ON" : "fallback"}`;
  } catch (_error) {
    status.className = "engine-status is-error";
    label.textContent = "バックエンド未接続";
    setNotice("Rustバックエンドに接続できません。サーバーを起動して再読み込みしてください。", "error");
  }
}

async function simulate() {
  stopPlayback();
  simulateButton.disabled = true;
  simulateButton.firstElementChild.textContent = "計算中…";
  const started = performance.now();
  try {
    const response = await fetch("/api/simulate", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ board: serializeBoard() }),
    });
    const payload = await response.json();
    if (!response.ok) throw new Error(payload.error || `HTTP ${response.status}`);
    result = payload;
    frameIndex = payload.frames.length - 1;
    document.querySelector("#latency").textContent = `${(performance.now() - started).toFixed(1)} ms round trip`;
    renderResult();
    setNotice(
      payload.chains === 0
        ? "4つ以上つながった色ぷよはありません。盤面は変化しません。"
        : payload.all_clear
          ? `${payload.chains}連鎖で全消しです。`
          : `${payload.chains}連鎖をネイティブSIMDバックエンドで解決しました。`,
      payload.chains > 0 ? "success" : "",
    );
    if (payload.chains > 0) void playAnimation(true);
  } catch (error) {
    setNotice(error.message, "error");
    document.querySelector("#latency").textContent = "実行失敗";
  } finally {
    simulateButton.disabled = false;
    simulateButton.firstElementChild.textContent = "SIMDで連鎖実行";
  }
}

const GAME_COLOURS = { red: "R", green: "G", blue: "B", yellow: "Y" };

function renderPieceWindow(pieces) {
  const windowElement = document.querySelector("#piece-window");
  windowElement.replaceChildren(...pieces.map((pair, index) => {
    const card = document.createElement("div");
    card.className = "piece-card";
    const title = document.createElement("strong");
    title.textContent = index === 0 ? "NOW" : `NEXT ${index}`;
    const child = document.createElement("span");
    child.innerHTML = puyoMarkup(GAME_COLOURS[pair.child]);
    const axis = document.createElement("span");
    axis.innerHTML = puyoMarkup(GAME_COLOURS[pair.axis]);
    card.append(title, child, axis);
    return card;
  }));
}

async function inspectGame(action = null) {
  const inspectButton = document.querySelector("#inspect-game");
  const playButton = document.querySelector("#play-action");
  const selector = document.querySelector("#legal-action");
  inspectButton.disabled = true;
  playButton.disabled = true;
  try {
    const seed = Number(document.querySelector("#game-seed").value) >>> 0;
    const response = await fetch("/api/game", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        board: serializeBoard(), seed, placements: gamePlacements,
        maximum_chain: gameMaximumChain, action,
      }),
    });
    const payload = await response.json();
    if (!response.ok) throw new Error(payload.error || `HTTP ${response.status}`);
    board = parseBoard(payload.board);
    gamePlacements = payload.placements;
    gameMaximumChain = payload.maximum_chain;
    renderBoard();
    renderPieceWindow(payload.pieces);
    selector.replaceChildren(...payload.legal_actions.map((candidate) => {
      const option = document.createElement("option");
      option.value = JSON.stringify({ column: candidate.column, rotation: candidate.rotation });
      option.textContent = `${candidate.column}列 ${candidate.rotation} · ${candidate.route_frames}f · ${candidate.route.join(" → ")}`;
      return option;
    }));
    selector.disabled = payload.legal_actions.length === 0;
    playButton.disabled = payload.legal_actions.length === 0;
    const outcome = payload.outcome;
    document.querySelector("#game-meta").textContent =
      `手数 ${payload.placements} · 最大 ${payload.maximum_chain}連鎖 · queue ${payload.queue_position}/128 · 合法手 ${payload.legal_actions.length}`
      + (outcome ? ` · 今回 ${outcome.chains}連鎖 ${outcome.score}点` : "")
      + (payload.dead ? " · GAME OVER" : "");
  } catch (error) {
    setNotice(error.message, "error");
  } finally {
    inspectButton.disabled = false;
  }
}

async function searchGame() {
  const button = document.querySelector("#search-game");
  const selector = document.querySelector("#legal-action");
  button.disabled = true;
  const started = performance.now();
  try {
    const response = await fetch("/api/search", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        board: serializeBoard(),
        seed: Number(document.querySelector("#game-seed").value) >>> 0,
        placements: gamePlacements,
        maximum_chain: gameMaximumChain,
        simulations: 800,
        max_depth: 8,
      }),
    });
    const payload = await response.json();
    if (!response.ok) throw new Error(payload.error || `HTTP ${response.status}`);
    selector.replaceChildren(...payload.actions.map((candidate) => {
      const option = document.createElement("option");
      option.value = JSON.stringify({ column: candidate.column, rotation: candidate.rotation });
      option.textContent = `${candidate.visits}訪問 · Q ${candidate.mean_value.toFixed(3)} · ${candidate.column}列 ${candidate.rotation} · ${candidate.route_frames}f`;
      return option;
    }));
    selector.disabled = payload.actions.length === 0;
    document.querySelector("#play-action").disabled = payload.actions.length === 0;
    document.querySelector("#game-meta").textContent =
      `Chance-PUCT 800回 · ${payload.nodes} nodes · root ${payload.root_value.toFixed(3)} · ${(performance.now() - started).toFixed(1)} ms round trip`;
  } catch (error) {
    setNotice(error.message, "error");
  } finally {
    button.disabled = false;
  }
}

function renderResult() {
  document.querySelector("#metric-chains").textContent = result.chains.toLocaleString("ja-JP");
  document.querySelector("#metric-score").textContent = result.score.toLocaleString("ja-JP");
  document.querySelector("#metric-erased").textContent = result.colored_erased.toLocaleString("ja-JP");
  document.querySelector("#metric-drop").textContent = result.max_drop_distance.toLocaleString("ja-JP");

  frameSlider.disabled = result.frames.length <= 1;
  frameSlider.max = String(result.frames.length - 1);
  frameSlider.value = String(frameIndex);
  document.querySelector("#previous-frame").disabled = frameIndex === 0;
  document.querySelector("#next-frame").disabled = frameIndex >= result.frames.length - 1;
  document.querySelector("#play-frames").disabled = result.frames.length <= 1;

  const track = document.querySelector("#timeline-track");
  track.replaceChildren(
    ...result.frames.map((frame) => {
      const point = document.createElement("span");
      point.textContent = frame.chain === 0 ? "START" : `${frame.chain} CHAIN`;
      return point;
    }),
  );
  renderFrame();
}

function renderFrame() {
  const frame = result.frames[frameIndex];
  renderBoard(parseBoard(frame.board));
  document.querySelector("#frame-title").textContent = frame.chain === 0 ? "開始盤面" : `${frame.chain}連鎖後の盤面`;
  document.querySelector("#previous-frame").disabled = frameIndex === 0;
  document.querySelector("#next-frame").disabled = frameIndex >= result.frames.length - 1;
  frameSlider.value = String(frameIndex);

  renderStepDetails(frame.step);
}

function changeFrame(offset) {
  if (!result) return;
  frameIndex = Math.max(0, Math.min(result.frames.length - 1, frameIndex + offset));
  renderFrame();
}

function togglePlayback() {
  if (animationPlaying) {
    stopPlayback();
    return;
  }
  void playAnimation(frameIndex >= result.frames.length - 1);
}

function stopPlayback() {
  animationGeneration += 1;
  animationPlaying = false;
  grid.classList.remove("is-dropping");
  grid.querySelectorAll(".is-erasing, .is-vanishing").forEach((cell) => {
    cell.classList.remove("is-erasing", "is-vanishing");
  });
  document.querySelector("#play-frames").textContent = "再生";
}

async function playAnimation(fromStart = false) {
  stopPlayback();
  const generation = animationGeneration;
  animationPlaying = true;
  document.querySelector("#play-frames").textContent = "停止";

  if (fromStart) {
    frameIndex = 0;
    renderFrame();
    if (!(await waitForAnimation(350, generation))) return;
  }

  while (frameIndex < result.frames.length - 1) {
    const nextIndex = frameIndex + 1;
    const nextFrame = result.frames[nextIndex];
    const step = nextFrame.step;
    renderBoard(parseBoard(result.frames[frameIndex].board));
    renderStepDetails(step);
    document.querySelector("#frame-title").textContent = `${nextFrame.chain}連鎖 · 消去予告`;
    markErasedCells(step.erased_cells, "is-erasing");
    if (!(await waitForAnimation(760, generation))) return;

    grid.querySelectorAll(".is-erasing").forEach((cell) => {
      cell.classList.remove("is-erasing");
      cell.classList.add("is-vanishing");
    });
    document.querySelector("#frame-title").textContent = `${nextFrame.chain}連鎖 · 消去`;
    if (!(await waitForAnimation(300, generation))) return;

    frameIndex = nextIndex;
    renderFrame();
    grid.classList.add("is-dropping");
    document.querySelector("#frame-title").textContent = `${nextFrame.chain}連鎖 · 落下`;
    if (!(await waitForAnimation(480, generation))) return;
    grid.classList.remove("is-dropping");
    document.querySelector("#frame-title").textContent = `${nextFrame.chain}連鎖後の盤面`;
    if (!(await waitForAnimation(360, generation))) return;
  }

  if (generation === animationGeneration) {
    animationPlaying = false;
    document.querySelector("#play-frames").textContent = "再生";
  }
}

function markErasedCells(cells, className) {
  cells.forEach(({ row, column }) => {
    grid.querySelector(`[data-row="${row}"][data-column="${column}"]`)?.classList.add(className);
  });
}

function waitForAnimation(milliseconds, generation) {
  const duration = window.matchMedia("(prefers-reduced-motion: reduce)").matches
    ? Math.min(milliseconds, 80)
    : milliseconds;
  return new Promise((resolve) => {
    window.setTimeout(() => resolve(generation === animationGeneration), duration);
  });
}

function renderStepDetails(step) {
  document.querySelector("#step-score").textContent = step ? step.score.toLocaleString("ja-JP") : "—";
  document.querySelector("#step-colors").textContent = step ? step.colors_erased : "—";
  document.querySelector("#step-garbage").textContent = step ? step.garbage_erased : "—";
  document.querySelector("#step-bonus").textContent = step ? step.group_bonus : "—";
}

function setNotice(message, kind = "") {
  notice.className = `notice${kind ? ` is-${kind}` : ""}`;
  notice.querySelector("p").textContent = message;
}

function resetResult(message) {
  stopPlayback();
  result = null;
  frameIndex = 0;
  frameSlider.disabled = true;
  frameSlider.max = "0";
  frameSlider.value = "0";
  ["#previous-frame", "#play-frames", "#next-frame"].forEach((selector) => {
    document.querySelector(selector).disabled = true;
  });
  renderBoard();
  setNotice(message);
  gamePlacements = 0;
  gameMaximumChain = 0;
  const selector = document.querySelector("#legal-action");
  if (selector) {
    selector.replaceChildren(new Option("合法手を取得してください", ""));
    selector.disabled = true;
    document.querySelector("#play-action").disabled = true;
    document.querySelector("#piece-window").replaceChildren();
    document.querySelector("#game-meta").textContent = "公開情報だけを表示します。未来のツモ列は返しません。";
  }
}

document.addEventListener("pointerup", () => { painting = false; });
document.addEventListener("pointermove", (event) => {
  if (!painting || event.buttons !== 1) return;
  const cell = document.elementFromPoint(event.clientX, event.clientY)?.closest(".board-cell");
  if (cell) paintCell(cell, selectedCell);
});
document.addEventListener("keydown", (event) => {
  const shortcuts = { 0: ".", 1: "R", 2: "G", 3: "B", 4: "Y", 5: "O" };
  if (shortcuts[event.key]) selectPalette(shortcuts[event.key]);
});

document.querySelectorAll(".palette-button").forEach((button) => {
  button.addEventListener("click", () => selectPalette(button.dataset.cell));
});
document.querySelectorAll("[data-sample]").forEach((button) => {
  button.addEventListener("click", () => {
    board = cloneBoard(samples[button.dataset.sample]);
    renderBoard();
    resetResult(`${button.textContent}のサンプルを読み込みました。`);
  });
});
document.querySelector("#clear-button").addEventListener("click", () => {
  board = Array.from({ length: HEIGHT }, () => Array(WIDTH).fill(EMPTY));
  renderBoard();
  resetResult("盤面を消去しました。");
});
document.querySelector("#gravity-button").addEventListener("click", applyGravity);
simulateButton.addEventListener("click", simulate);
document.querySelector("#inspect-game").addEventListener("click", () => inspectGame());
document.querySelector("#search-game").addEventListener("click", searchGame);
document.querySelector("#play-action").addEventListener("click", () => {
  const value = document.querySelector("#legal-action").value;
  if (value) void inspectGame(JSON.parse(value));
});
document.querySelector("#game-seed").addEventListener("change", () => {
  gamePlacements = 0;
  gameMaximumChain = 0;
  void inspectGame();
});
document.querySelector("#previous-frame").addEventListener("click", () => {
  stopPlayback();
  changeFrame(-1);
});
document.querySelector("#next-frame").addEventListener("click", () => {
  stopPlayback();
  changeFrame(1);
});
document.querySelector("#play-frames").addEventListener("click", togglePlayback);
frameSlider.addEventListener("input", () => {
  stopPlayback();
  frameIndex = Number(frameSlider.value);
  renderFrame();
});

buildGrid();
checkBackend();
