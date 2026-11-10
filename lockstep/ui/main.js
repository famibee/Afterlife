const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

let role = null;
let target = null;
let sent = 0; // 親機が配った連番の最新
let stats = { ok: 0, fail: 0, missed: 0, first_fail_seq: null }; // この機器の累積（数え直すまで保持）
let wantedTarget = null; // 起動オプションで指定されたアプリ名

function el(tag, text, className) {
  const node = document.createElement(tag);
  node.textContent = text;
  if (className) node.className = className;
  return node;
}

function statsText(s) {
  let text = `成功 ${s.ok} 手`;
  if (s.fail) text += `・失敗 ${s.fail} 手（${s.first_fail_seq} 手目から）`;
  if (s.missed) text += `・取りこぼし ${s.missed} 手`;
  return text;
}

function showStatus(error) {
  const status = $("status");
  let text = statsText(stats);
  if (error) text += ` — 直近: ${error}`;
  status.textContent = text;
  status.classList.toggle("error", stats.fail > 0 || stats.missed > 0);
}

async function checkPermission() {
  $("permission").hidden = await invoke("check_permission");
}

async function refreshWindows() {
  const windows = await invoke("list_windows");
  if (wantedTarget) {
    const match = windows.find((w) => w.app_name.toLowerCase().includes(wantedTarget.toLowerCase()));
    if (match) await selectTarget(match);
    wantedTarget = null;
  }
  // 選択中のウィンドウが閉じられていたら選択を外す
  if (target && !windows.some((w) => w.id === target.id)) await selectTarget(null);

  const list = $("windows");
  list.replaceChildren(
    ...windows.map((w) => {
      const li = document.createElement("li");
      li.append(el("span", w.app_name || "(名前なし)"), el("span", `${w.width}×${w.height}`, "muted"));
      li.classList.toggle("selected", target?.id === w.id);
      li.onclick = async () => {
        await selectTarget(target?.id === w.id ? null : w);
        refreshWindows();
      };
      return li;
    }),
  );
  $("target-hint").textContent = target
    ? `選択中: ${target.app_name}`
    : role === "host"
      ? "選ぶと、この機器のゲームにも同じ操作を入れます（選ばなければ操作専用）。"
      : "操作を入れるゲームのウィンドウを選んでください。";
}

async function selectTarget(w) {
  target = w;
  await invoke("set_target", { window: w });
  if (w) $("pad").style.aspectRatio = `${w.width} / ${w.height}`;
}

function flashPad() {
  $("pad").classList.add("flash");
  setTimeout(() => $("pad").classList.remove("flash"), 120);
}

async function sendClick(x, y, right) {
  sent = await invoke("host_click", { x, y, right });
  flashPad();
  if (!target) showSent();
}

async function sendKey(code) {
  sent = await invoke("host_key", { code });
  flashPad();
  if (!target) showSent();
}

// 親機が操作専用（対象ウィンドウ未選択）のときは注入結果が来ないので、配った数をそのまま表示する
function showSent() {
  stats = { ...stats, ok: sent };
  showStatus(null);
}

function renderClients(clients) {
  $("client-count").textContent = `${clients.length} 台`;
  $("clients").replaceChildren(
    ...clients.map((c) => {
      const li = document.createElement("li");
      li.append(el("span", `${c.name} (${c.os})`));
      const delay = sent - c.seq;
      li.append(el("span", statsText(c) + (delay > 0 ? `・受信待ち ${delay} 手` : ""), c.fail || c.missed ? "error" : "muted"));
      return li;
    }),
  );
}

async function startHost(autosend) {
  $("host").hidden = false;
  try {
    $("address").textContent = await invoke("start_host");
  } catch (e) {
    $("address").textContent = String(e);
    $("address").classList.add("error");
    return;
  }
  let clients = [];
  await listen("clients", (e) => renderClients((clients = e.payload)));
  // 結果が返らない子機の遅れを表示するため、送信後しばらくして描き直す
  let lagTimer;
  const checkLag = () => {
    clearTimeout(lagTimer);
    lagTimer = setTimeout(() => renderClients(clients), 1500);
  };

  $("reset").onclick = () => {
    sent = 0;
    invoke("host_reset");
  };

  const pad = $("pad");
  pad.addEventListener("contextmenu", (e) => e.preventDefault());
  pad.addEventListener("mousedown", (e) => {
    if (e.button !== 0 && e.button !== 2) return;
    const r = pad.getBoundingClientRect();
    sendClick((e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height, e.button === 2).then(checkLag);
  });
  window.addEventListener("keydown", (e) => {
    // 押しっぱなしの連打とショートカット（Cmd+Q 等）は送らない
    if (e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
    if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return;
    e.preventDefault();
    sendKey(e.code).then(checkLag);
  });
  pad.focus();

  if (autosend) {
    const steps = autosend.split(",");
    let i = 0;
    setTimeout(function next() {
      if (i >= steps.length) return;
      const step = steps[i++].trim();
      (step === "click" ? sendClick(0.5, 0.5, false) : sendKey(step)).then(checkLag);
      setTimeout(next, 2000);
    }, 6000);
  }
}

async function startClient(host) {
  $("client").hidden = false;
  const input = $("host-input");
  input.value = host || localStorage.getItem("host") || "";

  await listen("conn", (e) => {
    const { state, host, seconds } = e.payload;
    const conn = $("conn");
    conn.classList.toggle("error", state === "retrying");
    conn.textContent =
      state === "connected"
        ? `${host} に接続中`
        : state === "connecting"
          ? `${host} に接続しています…`
          : `${host} に接続できません。${seconds} 秒後に再試行します`;
  });

  const connect = () => {
    const value = input.value.trim();
    if (!value) return;
    localStorage.setItem("host", value);
    invoke("client_connect", { host: value });
  };
  $("connect").addEventListener("submit", (e) => {
    e.preventDefault();
    connect();
  });
  connect();
}

async function start(chosen, config = {}) {
  role = chosen;
  wantedTarget = config.target || null;
  $("role").hidden = true;
  $("main").hidden = false;

  await listen("applied", (e) => {
    const { error, stats: s } = e.payload;
    stats = s;
    showStatus(error);
  });
  await listen("stats", (e) => {
    stats = e.payload;
    showStatus(null);
  });

  $("perm-open").onclick = () => invoke("open_permission_settings");
  $("perm-check").onclick = checkPermission;
  $("refresh").onclick = refreshWindows;
  await checkPermission();
  await refreshWindows();

  if (role === "host") await startHost(config.autosend);
  else await startClient(config.host);
}

for (const button of document.querySelectorAll("[data-role]")) {
  button.onclick = () => start(button.dataset.role);
}

invoke("initial_config").then((config) => {
  if (config.role === "host" || config.role === "client") start(config.role, config);
});
