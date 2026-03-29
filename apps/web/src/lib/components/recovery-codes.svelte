<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { toast } from "$lib/components/toast";
  import Download from "@lucide/svelte/icons/download";
  import Printer from "@lucide/svelte/icons/printer";

  interface Props {
    codes: string[];
  }

  let { codes }: Props = $props();

  function downloadCodes() {
    const text = [
      "Mokumo Print — Recovery Codes",
      "================================",
      "Store these codes in a safe place.",
      "Each code can only be used once.",
      "",
      ...codes.map((code, i) => `${String(i + 1).padStart(2, " ")}. ${code}`),
      "",
      `Generated: ${new Date().toISOString()}`,
    ].join("\n");

    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "mokumo-recovery-codes.txt";
    a.click();
    URL.revokeObjectURL(url);
  }

  async function printCodes() {
    // In Tauri's WKWebView (macOS), window.print() is silently dropped
    // because WKWebView requires a print delegate that Tauri doesn't configure
    // by default. Delegate to the print_window Tauri command instead, which
    // calls WebviewWindow::print() on the Rust side to trigger the native dialog.
    if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("print_window");
      } catch (e) {
        console.error("print_window invoke failed:", e);
        toast.error(
          "Could not open the print dialog. Try downloading your codes instead.",
        );
      }
    } else {
      window.print();
    }
  }
</script>

<div class="print-codes space-y-4">
  <div
    class="grid grid-cols-2 gap-2 rounded-lg border bg-muted/50 p-4 font-mono text-sm print:border-black"
  >
    {#each codes as code, i}
      <div class="flex items-center gap-2">
        <span class="text-muted-foreground w-5 text-right text-xs"
          >{i + 1}.</span
        >
        <span class="select-all">{code}</span>
      </div>
    {/each}
  </div>
  <div class="flex gap-2 print:hidden">
    <Button variant="outline" size="sm" onclick={downloadCodes}>
      <Download class="mr-2 h-4 w-4" />
      Download
    </Button>
    <Button variant="outline" size="sm" onclick={printCodes}>
      <Printer class="mr-2 h-4 w-4" />
      Print
    </Button>
  </div>
</div>

<style>
  @media print {
    :global(body *) {
      visibility: hidden;
    }

    :global(.print-codes),
    :global(.print-codes *) {
      visibility: visible;
    }

    :global(.print-codes) {
      position: absolute;
      inset: 0;
    }
  }
</style>
