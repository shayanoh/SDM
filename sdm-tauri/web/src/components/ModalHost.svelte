<script lang="ts">
  import { useUi } from "../lib/context";
  import DeleteConfirm from "./DeleteConfirm.svelte";
  import NewPackagePrompt from "./NewPackagePrompt.svelte";
  import PauseConfirm from "./PauseConfirm.svelte";

  /** The one slot every modal shows in: only one is ever open. */
  const ui = useUi();
  const modal = $derived(ui.modal);
</script>

{#if modal?.kind === "delete"}
  <DeleteConfirm target={modal.target} info={modal.info} />
{:else if modal?.kind === "pause"}
  <PauseConfirm ids={modal.ids} />
{:else if modal?.kind === "newPackage"}
  <NewPackagePrompt ids={modal.ids} />
{/if}
