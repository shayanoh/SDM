<script lang="ts">
  /** A one-line name cut in the middle when too long, so the extension stays visible. */
  let { text }: { text: string } = $props();

  const TAIL = 12;
  const head = $derived(
    text.length > TAIL ? text.slice(0, text.length - TAIL) : text,
  );
  const tail = $derived(
    text.length > TAIL ? text.slice(text.length - TAIL) : "",
  );
</script>

<span class="name" title={text}
  ><span class="head">{head}</span><span class="tail">{tail}</span></span
>

<style>
  .name {
    display: flex;
    min-width: 0;
    white-space: pre;
  }
  .head {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tail {
    flex: none;
  }
</style>
