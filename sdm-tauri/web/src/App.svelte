<script lang="ts">
  import { onMount } from "svelte";
  import Shell from "./components/Shell.svelte";
  import { errorMessage, useBridge, useStore, useUi } from "./lib/context";
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

    void guarded(async () => {
      const style = document.createElement("style");
      style.id = "theme";
      // `?theme=deep-purple` loads another bundled theme (the screenshots use it).
      const themeId =
        new URLSearchParams(location.search).get("theme") ?? undefined;
      style.textContent = await bridge.themeCss(themeId);
      document.head.append(style);
      await bridge.attach(store.apply.bind(store));
      await syncVibrancy(bridge);
    });

    return () => document.removeEventListener("visibilitychange", onVisibility);
  });
</script>

<Shell />
