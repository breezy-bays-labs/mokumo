<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import Spinner from "./spinner.svelte";

  const { Story } = defineMeta({
    title: "Components/Spinner",
    component: Spinner,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Spinner />
</Story>

<Story name="Small">
  <Spinner size="sm" />
</Story>

<Story name="Large">
  <Spinner size="lg" />
</Story>

<Story name="All Sizes">
  <div class="flex items-center gap-4">
    <Spinner size="sm" />
    <Spinner size="md" />
    <Spinner size="lg" />
  </div>
</Story>
