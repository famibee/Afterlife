<script lang="ts">
  let { onselect }: { onselect: (role: "host" | "client") => void } = $props();
</script>

<div class="role-select">
  <h1>Afterlife</h1>
  <p class="sub">このマシンの役割を選んでください</p>
  <div class="cards">
    <button class="card" onclick={() => onselect("host")}>
      <span class="icon">🎮</span>
      <span class="label">親機</span>
      <span class="desc">自分が操作して、子機に同じ操作を配信する</span>
    </button>
    <button class="card" onclick={() => onselect("client")}>
      <span class="icon">📡</span>
      <span class="label">子機</span>
      <span class="desc">親機の操作を受け取って、別のウィンドウに反映する</span>
    </button>
  </div>
</div>

<style>
  .role-select {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 100vh;
    gap: 1rem;
  }
  h1 {
    margin: 0;
    font-size: 2.2rem;
  }
  .sub {
    margin: 0 0 1.5rem;
    opacity: 0.7;
  }
  .cards {
    display: flex;
    gap: 1.5rem;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    width: 220px;
    padding: 2rem 1.2rem;
    border-radius: 16px;
    border: 2px solid transparent;
    background: var(--panel-bg, #ffffff);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.12);
    cursor: pointer;
    transition: transform 0.15s, border-color 0.15s;
  }
  .card:hover {
    transform: translateY(-3px);
    border-color: #4da3ff;
  }
  .icon {
    font-size: 2.6rem;
  }
  .label {
    font-size: 1.3rem;
    font-weight: 700;
  }
  .desc {
    font-size: 0.85rem;
    opacity: 0.7;
    text-align: center;
  }

  @media (prefers-color-scheme: dark) {
    .card {
      background: #2a2a2a;
    }
  }
</style>
