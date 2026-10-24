<script lang="ts">
  import { onMount } from "svelte";
  import Shell from "./components/Shell.svelte";
  import { errorMessage, useBridge, useStore, useUi } from "./lib/context";
  import { wireNavigation } from "./lib/navigation";
  import { startTheming } from "./lib/theme";
  import { syncVibrancy } from "./lib/vibrancy";

  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();

  async function guarded(action: () => Promise<void>) {
    try {
      await action();
    } catch (error) {
      ui.report(errorMessage(error));
    }
  }

  onMount(() => {
    const onVisibility = () => {
      guarded(() => bridge.setListVisible(!document.hidden));
    };
    document.addEventListener("visibilitychange", onVisibility);

    const stopNavigation = wireNavigation(bridge, ui);
    let stopTheming: (() => void) | undefined;
    void guarded(async () => {
      // `?theme=deep-purple` shows another bundled theme (the screenshots use it).
      stopTheming = await startTheming(
        bridge,
        new URLSearchParams(location.search).get("theme"),
      );
      await bridge.attach(store.apply.bind(store));
      await syncVibrancy(bridge);
    });

    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      stopTheming?.();
      stopNavigation();
    };
  });
</script>

<Shell />
