<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import { Calendar } from "./index.js";

  const { Story } = defineMeta({
    title: "UI/Calendar",
    component: Calendar,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Calendar type="single" />
</Story>

<Story name="With Dropdown Selects">
  <Calendar type="single" captionLayout="dropdown" />
</Story>

<Story name="Multiple Selection">
  <Calendar type="multiple" />
</Story>
