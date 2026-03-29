<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import StatusDot from "./status-dot.svelte";

  const { Story } = defineMeta({
    title: "Components/StatusDot",
    component: StatusDot,
    tags: ["autodocs"],
  });
</script>

<Story name="Ready">
  <StatusDot status="ready" />
</Story>

<Story name="Building">
  <StatusDot status="building" />
</Story>

<Story name="Error">
  <StatusDot status="error" />
</Story>

<Story name="Queued">
  <StatusDot status="queued" />
</Story>

<Story name="Canceled">
  <StatusDot status="canceled" />
</Story>

<Story name="With Labels">
  <div class="flex flex-col gap-3">
    <StatusDot status="ready" label="Ready" />
    <StatusDot status="building" label="Building" />
    <StatusDot status="error" label="Error" />
    <StatusDot status="queued" label="Queued" />
    <StatusDot status="canceled" label="Canceled" />
  </div>
</Story>
