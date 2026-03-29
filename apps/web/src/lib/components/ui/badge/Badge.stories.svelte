<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import { Badge } from "./index.js";

  const { Story } = defineMeta({
    title: "UI/Badge",
    component: Badge,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Badge>Default</Badge>
</Story>

<Story name="Secondary">
  <Badge variant="secondary">Secondary</Badge>
</Story>

<Story name="Outline">
  <Badge variant="outline">Outline</Badge>
</Story>

<Story name="Destructive">
  <Badge variant="destructive">Destructive</Badge>
</Story>

<Story name="Success">
  <Badge variant="success">Ready</Badge>
</Story>

<Story name="Warning">
  <Badge variant="warning">Building</Badge>
</Story>

<Story name="Error">
  <Badge variant="error">Failed</Badge>
</Story>

<Story name="Info">
  <Badge variant="info">Queued</Badge>
</Story>

<Story name="Status Variants">
  <div class="flex flex-wrap gap-2">
    <Badge variant="success">Ready</Badge>
    <Badge variant="warning">Building</Badge>
    <Badge variant="error">Failed</Badge>
    <Badge variant="info">Queued</Badge>
  </div>
</Story>
