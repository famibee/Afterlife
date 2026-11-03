<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";

  let { onback }: { onback: () => void } = $props();

  type WindowInfo = {
    id: number;
    pid: number;
    app_name: string;
    x: number;
    y: number;
    width: number;
    height: number;
  };
  type DiscoveredHost = {
    fullname: string;
    host: string;
    port: number;
    addresses: string[];
  };
  type ConnectionStatus = "disconnected" | "connecting" | "connected" | "failed";

  let hasPermission = $state(true);
  let discovered = $state<DiscoveredHost[]>([]);
  let manualHost = $state("");
  let status = $state<ConnectionStatus>("disconnected");
  let windows = $state<WindowInfo[]>([]);
  let target = $state<WindowInfo | null>(null);
  let testResult = $state("");

  let unlistenFns: Array<() => void> = [];

  async function checkPermission() {
    hasPermission = await invoke<boolean>("client_check_permission");
  }

  async function refreshWindows() {
    windows = await invoke<WindowInfo[]>("client_list_windows");
  }

  function preferredAddress(d: DiscoveredHost): string {
    // IPv6(fe80::等)より先にIPv4アドレスを優先して接続する
    return d.addresses.find((a) => !a.includes(":")) ?? d.addresses[0] ?? d.host;
  }

  function hover(w: WindowInfo) {
    invoke("client_show_highlight", { x: w.x, y: w.y, width: w.width, height: w.height });
  }
  function unhover() {
    invoke("client_hide_highlight");
  }

  async function selectWindow(w: WindowInfo) {
    target = w;
    await invoke("client_set_target", { id: w.id, pid: w.pid, appName: w.app_name });
  }

  async function testClick() {
    testResult = "";
    try {
      await invoke("client_test_click");
      testResult = "クリックを送信しました";
    } catch (e) {
      testResult = `失敗: ${e}`;
    }
  }

  async function connectTo(host: string, port?: number) {
    await invoke("client_connect", { host, port });
  }

  onMount(async () => {
    await checkPermission();
    await invoke("client_start_discovery");
    await refreshWindows();

    unlistenFns.push(
      await listen<DiscoveredHost>("host-discovered", (e) => {
        if (!discovered.some((d) => d.fullname === e.payload.fullname)) {
          discovered = [...discovered, e.payload];
        }
      }),
    );
    unlistenFns.push(
      await listen<string>("host-removed", (e) => {
        discovered = discovered.filter((d) => d.fullname !== e.payload);
      }),
    );
    unlistenFns.push(
      await listen<ConnectionStatus>("client-connection-changed", (e) => {
        status = e.payload;
      }),
    );
  });

  onDestroy(async () => {
    unlistenFns.forEach((f) => f());
    await invoke("client_hide_highlight");
  });

  async function back() {
    await invoke("client_stop_discovery");
    await invoke("client_disconnect");
    onback();
  }
</script>

<div class="client">
  <header>
    <button class="back" onclick={back}>← 戻る</button>
    <h2>子機</h2>
  </header>

  {#if !hasPermission}
    <div class="panel">
      <h3>アクセシビリティ権限が必要です</h3>
      <p>
        このアプリが他のアプリをクリック操作するには、システム設定でアクセシビリティ権限を許可してください。
      </p>
      <button onclick={() => invoke("client_open_permission_settings")}>システム設定を開く</button>
      <button onclick={checkPermission}>権限を確認しなおす</button>
    </div>
  {:else}
    <div class="layout">
      <section class="panel">
        <h3>親機に接続</h3>
        <p class="status">
          状態:
          <span class="badge {status}">
            {status === "connected" ? "接続済み" : status === "connecting" ? "接続中..." : status === "failed" ? "失敗" : "未接続"}
          </span>
        </p>

        <h4>見つかった親機</h4>
        {#if discovered.length === 0}
          <p class="empty">探索中... 親機アプリが同じネットワーク上で起動しているか確認してください</p>
        {/if}
        <ul>
          {#each discovered as d (d.fullname)}
            <li>
              <button onclick={() => connectTo(preferredAddress(d), d.port)}>
                {d.fullname.split(".")[0]} ({preferredAddress(d)})
              </button>
            </li>
          {/each}
        </ul>

        <h4>手動でIPを入力</h4>
        <div class="manual">
          <input placeholder="192.168.1.10" bind:value={manualHost} />
          <button onclick={() => connectTo(manualHost)} disabled={!manualHost}>接続</button>
        </div>
      </section>

      <section class="panel">
        <h3>操作するウィンドウを選択</h3>
        <button class="refresh" onclick={refreshWindows}>一覧を更新</button>
        <ul class="windows">
          {#each windows as w (w.id)}
            <li
              class:selected={target?.id === w.id}
              onmouseenter={() => hover(w)}
              onmouseleave={unhover}
            >
              <button onclick={() => selectWindow(w)}>
                <span class="app">{w.app_name}</span>
                <span class="size">{w.width}×{w.height}</span>
              </button>
            </li>
          {/each}
        </ul>
        <p class="hint">項目にマウスを乗せると、該当ウィンドウが画面上でハイライトされます</p>

        {#if target}
          <div class="target-info">
            <p>選択中: <strong>{target.app_name}</strong></p>
            <button onclick={testClick}>テストクリック（中央）</button>
            {#if testResult}
              <p class="result">{testResult}</p>
            {/if}
          </div>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .client {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  header {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.8rem 1.2rem;
    border-bottom: 1px solid rgba(127, 127, 127, 0.25);
  }
  header h2 {
    margin: 0;
  }
  .back {
    background: none;
    border: none;
    cursor: pointer;
    font-size: 0.95rem;
  }
  .layout {
    flex: 1;
    display: flex;
    gap: 1rem;
    padding: 1.2rem;
    overflow: auto;
  }
  .panel {
    flex: 1;
    background: rgba(127, 127, 127, 0.06);
    border-radius: 12px;
    padding: 1.2rem;
  }
  .panel h3 {
    margin-top: 0;
  }
  .panel h4 {
    margin: 1rem 0 0.4rem;
    font-size: 0.85rem;
    opacity: 0.7;
  }
  .empty {
    opacity: 0.6;
    font-size: 0.85rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .manual {
    display: flex;
    gap: 0.5rem;
  }
  .manual input {
    flex: 1;
  }
  .badge {
    display: inline-block;
    padding: 0.1rem 0.6rem;
    border-radius: 999px;
    font-size: 0.8rem;
    background: #888;
    color: white;
  }
  .badge.connected {
    background: #4caf50;
  }
  .badge.connecting {
    background: #ffa726;
  }
  .badge.failed {
    background: #e53935;
  }
  .windows li button {
    width: 100%;
    display: flex;
    justify-content: space-between;
    text-align: left;
    padding: 0.5rem 0.7rem;
  }
  .windows li.selected button {
    border-color: #4da3ff;
    box-shadow: 0 0 0 2px #4da3ff inset;
  }
  .size {
    opacity: 0.6;
    font-size: 0.8rem;
  }
  .hint {
    font-size: 0.78rem;
    opacity: 0.6;
    margin-top: 0.6rem;
  }
  .target-info {
    margin-top: 1rem;
    padding-top: 1rem;
    border-top: 1px solid rgba(127, 127, 127, 0.25);
  }
  .result {
    font-size: 0.85rem;
    opacity: 0.8;
  }
  .refresh {
    margin-bottom: 0.6rem;
  }
</style>
