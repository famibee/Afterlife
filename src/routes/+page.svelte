<script lang="ts">
  import RoleSelect from "$lib/RoleSelect.svelte";
  import HostPad from "$lib/HostPad.svelte";
  import ClientSetup from "$lib/ClientSetup.svelte";

  let role = $state<"none" | "host" | "client">("none");
</script>

{#if role === "none"}
  <RoleSelect onselect={(r) => (role = r)} />
{:else if role === "host"}
  <HostPad onback={() => (role = "none")} />
{:else}
  <ClientSetup onback={() => (role = "none")} />
{/if}

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, sans-serif;
    background: #f6f6f6;
    color: #0f0f0f;
  }
  :global(button) {
    font-family: inherit;
    border-radius: 8px;
    border: 1px solid rgba(127, 127, 127, 0.3);
    background: white;
    padding: 0.5rem 0.9rem;
    cursor: pointer;
  }
  :global(button:hover) {
    border-color: #4da3ff;
  }
  :global(input) {
    font-family: inherit;
    border-radius: 8px;
    border: 1px solid rgba(127, 127, 127, 0.3);
    padding: 0.5rem 0.7rem;
  }
  @media (prefers-color-scheme: dark) {
    :global(html, body) {
      background: #1e1e1e;
      color: #f0f0f0;
    }
    :global(button) {
      background: #2a2a2a;
      color: #f0f0f0;
    }
    :global(input) {
      background: #2a2a2a;
      color: #f0f0f0;
      border-color: rgba(255, 255, 255, 0.2);
    }
  }
</style>
