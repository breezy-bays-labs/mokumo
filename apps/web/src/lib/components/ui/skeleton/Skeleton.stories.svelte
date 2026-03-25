<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import { Skeleton } from "./index.js";

  const { Story } = defineMeta({
    title: "UI/Skeleton",
    component: Skeleton,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <div class="bg-card p-6 rounded-lg">
    <div class="flex items-center space-x-4">
      <Skeleton class="h-12 w-12 rounded-full" />
      <div class="space-y-2">
        <Skeleton class="h-4 w-[250px]" />
        <Skeleton class="h-4 w-[200px]" />
      </div>
    </div>
  </div>
</Story>
