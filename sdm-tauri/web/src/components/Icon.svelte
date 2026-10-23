<script lang="ts">
  import { ICONS, type IconName } from "../lib/icons";

  let {
    name,
    size = 16,
    strokeWidth = 2,
  }: { name: IconName; size?: number; strokeWidth?: number } = $props();

  const Glyph = $derived(ICONS[name]);
</script>

<Glyph {size} {strokeWidth} aria-hidden="true" />
