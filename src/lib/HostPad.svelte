<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";

  let { onback }: { onback: () => void } = $props();

  type ConnectedClient = {
    id: number;
    name: string;
    os: string;
    target: string | null;
  };

  type SnapshotPayload = { id: number; data: string };

  let clients = $state<ConnectedClient[]>([]);
  let flashing = $state(false);
  let padEl: HTMLDivElement;
  let localAddress = $state<string | null>(null);
  let snapshotUrl = $state<string | null>(null);
  // スナップショット到着前のデフォルト。届き次第、画像の実寸に合わせて更新する。
  let padAspect = $state(16 / 9);

  function flash() {
    flashing = true;
    setTimeout(() => {
      flashing = false;
    }, 120);
  }

  function handlePadClick(e: MouseEvent) {
    const rect = padEl.getBoundingClientRect();
    const x = (e.clientX - rect.left) / rect.width;
    const y = (e.clientY - rect.top) / rect.height;
    const button = e.button === 2 ? "right" : "left";
    invoke("host_send_click", { x, y, button });
    flash();
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.repeat) return;
    invoke("host_send_key", { code: e.code, direction: "press" });
  }
  function handleKeyUp(e: KeyboardEvent) {
    invoke("host_send_key", { code: e.code, direction: "release" });
  }

  let unlistenClients: (() => void) | undefined;
  let unlistenSnapshot: (() => void) | undefined;

  onMount(async () => {
    await invoke("start_host");
    localAddress = await invoke<string | null>("host_local_address");
    unlistenClients = await listen<ConnectedClient[]>("host-clients-changed", (event) => {
      clients = event.payload;
    });
    // 子機からの定期スナップショット(低頻度の確認用、リアルタイムではない)を
    // パッドの背景に表示する。複数子機が繋がっている場合は最後に届いたものを使う。
    unlistenSnapshot = await listen<SnapshotPayload>("host-snapshot", (event) => {
      const url = `data:image/jpeg;base64,${event.payload.data}`;
      snapshotUrl = url;
      // パッドの画面比を、スナップショットの実寸に合わせて自動調整する
      // （固定サイズ運用の操作対象アプリなら、初回到着分でほぼ決まる）。
      const img = new Image();
      img.onload = () => {
        if (img.naturalWidth > 0 && img.naturalHeight > 0) {
          padAspect = img.naturalWidth / img.naturalHeight;
        }
      };
      img.src = url;
    });
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeyDown);
    window.removeEventListener("keyup", handleKeyUp);
    unlistenClients?.();
    unlistenSnapshot?.();
  });

  async function back() {
    await invoke("stop_host");
    onback();
  }
</script>

<div class="host">
  <header>
    <button class="back" onclick={back}>← 戻る</button>
    <h2>親機</h2>
    {#if localAddress}
      <span class="address">{localAddress}</span>
    {/if}
  </header>

  <div class="layout">
    <div class="pad-area">
      <div
        class="pad"
        bind:this={padEl}
        style="aspect-ratio: {padAspect}; {snapshotUrl
          ? `background-image: url(${snapshotUrl}); background-size: contain; background-position: center; background-repeat: no-repeat;`
          : ''}"
        onmousedown={handlePadClick}
        oncontextmenu={(e) => e.preventDefault()}
        role="button"
        tabindex="0"
      >
        {#if !snapshotUrl}
          <div class="hint">ここをクリック / キー操作すると子機に配信されます</div>
        {/if}
        {#if flashing}
          <div class="flash"></div>
        {/if}
      </div>
    </div>

    <aside class="sidebar">
      <h3>接続中の子機（{clients.length}）</h3>
      {#if clients.length === 0}
        <p class="empty">まだ子機が接続していません</p>
      {/if}
      <ul>
        {#each clients as c (c.id)}
          <li>
            <span class="dot"></span>
            <div>
              <div class="name">{c.name}</div>
              <div class="meta">{c.os} / {c.target ?? "未選択"}</div>
            </div>
          </li>
        {/each}
      </ul>
    </aside>
  </div>
</div>

<style>
  .host {
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
    flex: 1;
  }
  .address {
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.85rem;
    opacity: 0.65;
    padding: 0.15rem 0.5rem;
    border-radius: 6px;
    background: rgba(127, 127, 127, 0.12);
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
    overflow: hidden;
  }
  .pad-area {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .pad {
    position: relative;
    max-width: 100%;
    max-height: 100%;
    width: 100%;
    background: #1b1f2a;
    border-radius: 12px;
    border: 2px solid #3a4258;
    cursor: crosshair;
    display: flex;
    align-items: center;
    justify-content: center;
    user-select: none;
  }
  .hint {
    color: #7d8aa3;
    font-size: 0.9rem;
  }
  .flash {
    position: absolute;
    inset: 0;
    background: rgba(77, 163, 255, 0.25);
    border-radius: 12px;
    pointer-events: none;
  }
  .sidebar {
    width: 240px;
    flex-shrink: 0;
  }
  .sidebar h3 {
    font-size: 0.95rem;
    margin: 0 0 0.6rem;
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
    gap: 0.5rem;
  }
  li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem;
    border-radius: 8px;
    background: rgba(127, 127, 127, 0.08);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #4caf50;
    flex-shrink: 0;
  }
  .name {
    font-weight: 600;
    font-size: 0.9rem;
  }
  .meta {
    font-size: 0.75rem;
    opacity: 0.65;
  }
</style>
