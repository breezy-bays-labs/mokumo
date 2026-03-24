<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import { Select, SelectTrigger, SelectContent, SelectItem } from "./index.js";

  const { Story } = defineMeta({
    title: "UI/Select",
    component: Select,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Select type="single">
    <SelectTrigger class="w-[180px]">Select a fruit</SelectTrigger>
    <SelectContent>
      <SelectItem value="apple">Apple</SelectItem>
      <SelectItem value="banana">Banana</SelectItem>
      <SelectItem value="orange">Orange</SelectItem>
    </SelectContent>
  </Select>
</Story>
