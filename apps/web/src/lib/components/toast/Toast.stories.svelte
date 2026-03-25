<script module lang="ts">
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import { Toaster } from "svelte-sonner";
  import { toastClasses } from "./index.js";

  const { Story } = defineMeta({
    title: "UI/Toast",
    tags: ["autodocs"],
  });
</script>

<script>
  import { toast } from "./index.js";
  import { Button } from "$lib/components/ui/button/index.js";
</script>

<Toaster closeButton toastOptions={{ unstyled: true, classes: toastClasses }} />

<Story name="Default">
  <Button onclick={() => toast.success("Customer created successfully")}
    >Show toast</Button
  >
</Story>

<Story name="Error">
  <Button onclick={() => toast.error("Failed to save changes")}
    >Show toast</Button
  >
</Story>

<Story name="Warning">
  <Button onclick={() => toast.warning("Unsaved changes will be lost")}
    >Show toast</Button
  >
</Story>

<Story name="Info">
  <Button onclick={() => toast.info("Processing your request...")}
    >Show toast</Button
  >
</Story>

<Story name="Stacked">
  <Button
    onclick={() => {
      toast.success("First toast");
      toast.error("Second toast");
      toast.warning("Third toast");
    }}>Show toasts</Button
  >
</Story>
